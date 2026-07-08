use super::{AnyProvider, ProviderDefinition, ProviderScope, ProviderToken};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

/// Runtime provider container. This is Boot's Rust equivalent of Nest's ModuleRef.
#[derive(Clone, Default)]
pub struct ModuleRef {
    providers: Arc<RwLock<BTreeMap<ProviderToken, ProviderEntry>>>,
    visible_scopes: Arc<RwLock<Vec<ModuleRef>>>,
    request_cache: Option<ProviderCache>,
}

type ProviderCache = Arc<RwLock<BTreeMap<ProviderCacheKey, Arc<AnyProvider>>>>;

static NEXT_PROVIDER_CACHE_KEY: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ProviderCacheKey(u64);

impl ProviderCacheKey {
    fn next() -> Self {
        Self(NEXT_PROVIDER_CACHE_KEY.fetch_add(1, Ordering::Relaxed))
    }
}

#[derive(Clone)]
struct ProviderEntry {
    cache_key: ProviderCacheKey,
    definition: ProviderDefinition,
    singleton: ProviderCache,
    owner: Option<ModuleRef>,
}

impl ProviderEntry {
    fn new(definition: ProviderDefinition) -> Self {
        Self {
            cache_key: ProviderCacheKey::next(),
            definition,
            singleton: Arc::new(RwLock::new(BTreeMap::new())),
            owner: None,
        }
    }

    fn with_owner(mut self, owner: ModuleRef) -> Self {
        if self.owner.is_none() {
            self.owner = Some(owner);
        }
        self
    }

    fn scope(&self) -> ProviderScope {
        self.definition.scope()
    }

    fn resolve(
        &self,
        module_ref: &ModuleRef,
        request_cache: Option<ProviderCache>,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Arc<AnyProvider>> {
        let base_ref = self.owner.as_ref().unwrap_or(module_ref);
        if let Some(target) = self.definition.alias_target() {
            return self.resolve_alias(base_ref, target, request_cache, alias_path);
        }

        match self.scope() {
            ProviderScope::Singleton => self.resolve_singleton(base_ref),
            ProviderScope::Transient => {
                let scoped_ref;
                let factory_ref = match request_cache.clone() {
                    Some(request_cache) => {
                        scoped_ref = base_ref.with_request_cache(request_cache);
                        &scoped_ref
                    }
                    None => base_ref,
                };
                self.definition.build(factory_ref)
            }
            ProviderScope::Request => {
                let scoped_ref;
                let factory_ref = match request_cache.clone() {
                    Some(request_cache) => {
                        scoped_ref = base_ref.with_request_cache(request_cache);
                        &scoped_ref
                    }
                    None => base_ref,
                };
                self.resolve_request(factory_ref, request_cache)
            }
        }
    }

    fn resolve_alias(
        &self,
        module_ref: &ModuleRef,
        target: &ProviderToken,
        request_cache: Option<ProviderCache>,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Arc<AnyProvider>> {
        if alias_path.contains(self.definition.token()) {
            alias_path.push(self.definition.token().clone());
            let chain = alias_path
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" -> ");
            return Err(BootError::Internal(format!(
                "cyclic provider alias detected: {chain}"
            )));
        }

        alias_path.push(self.definition.token().clone());
        let value =
            module_ref.get_any_with_request_cache_inner(target, request_cache, alias_path)?;
        alias_path.pop();

        value.ok_or_else(|| BootError::MissingProvider(target.to_string()))
    }

    fn resolve_singleton(&self, module_ref: &ModuleRef) -> Result<Arc<AnyProvider>> {
        if let Some(value) = self
            .read_cache(&self.singleton)?
            .get(&self.cache_key)
            .cloned()
        {
            return Ok(value);
        }

        let value = self.definition.build(module_ref)?;
        self.write_cache(&self.singleton)?
            .insert(self.cache_key, Arc::clone(&value));
        Ok(value)
    }

    fn resolve_request(
        &self,
        module_ref: &ModuleRef,
        request_cache: Option<ProviderCache>,
    ) -> Result<Arc<AnyProvider>> {
        let Some(request_cache) = request_cache else {
            return self.definition.build(module_ref);
        };

        if let Some(value) = self
            .read_cache(&request_cache)?
            .get(&self.cache_key)
            .cloned()
        {
            return Ok(value);
        }

        let value = self.definition.build(module_ref)?;
        self.write_cache(&request_cache)?
            .insert(self.cache_key, Arc::clone(&value));
        Ok(value)
    }

    async fn seed_singleton_async(&self, module_ref: ModuleRef) -> Result<()> {
        let value = self.definition.build_async(module_ref).await?;
        self.seed_singleton(value)
    }

    fn seed_singleton(&self, value: Arc<AnyProvider>) -> Result<()> {
        self.write_cache(&self.singleton)?
            .insert(self.cache_key, value);
        Ok(())
    }

    fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_module_init() else {
            return Ok(());
        };

        let value = self.resolve_singleton(module_ref)?;
        hook(value, module_ref)
    }

    async fn on_application_bootstrap(&self, module_ref: ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_application_bootstrap() else {
            return Ok(());
        };

        let value = self.resolve_singleton(&module_ref)?;
        hook(value, module_ref).await
    }

    async fn on_application_shutdown(&self, module_ref: ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_application_shutdown() else {
            return Ok(());
        };

        let value = self.resolve_singleton(&module_ref)?;
        hook(value, module_ref).await
    }

    fn read_cache<'a>(
        &self,
        cache: &'a ProviderCache,
    ) -> Result<std::sync::RwLockReadGuard<'a, BTreeMap<ProviderCacheKey, Arc<AnyProvider>>>> {
        cache
            .read()
            .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))
    }

    fn write_cache<'a>(
        &self,
        cache: &'a ProviderCache,
    ) -> Result<std::sync::RwLockWriteGuard<'a, BTreeMap<ProviderCacheKey, Arc<AnyProvider>>>> {
        cache
            .write()
            .map_err(|_| BootError::Internal("provider cache lock is poisoned".to_string()))
    }
}

impl fmt::Debug for ModuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self
            .providers
            .read()
            .map(|providers| providers.len())
            .unwrap_or(0);
        let visible = self
            .visible_scopes
            .read()
            .map(|scopes| scopes.len())
            .unwrap_or(0);
        f.debug_struct("ModuleRef")
            .field("providers", &len)
            .field("visible_scopes", &visible)
            .finish()
    }
}

impl ModuleRef {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_scope(&self) -> Self {
        self.with_request_cache(Arc::new(RwLock::new(BTreeMap::new())))
    }

    fn with_request_cache(&self, request_cache: ProviderCache) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            visible_scopes: Arc::clone(&self.visible_scopes),
            request_cache: Some(request_cache),
        }
    }

    pub fn register(&self, definition: ProviderDefinition) -> Result<()> {
        let token = definition.token().clone();
        self.validate_registration(&token, &definition)?;
        if definition.is_async_factory() {
            return Err(BootError::Internal(format!(
                "async provider factory requires async registration: {token}"
            )));
        }

        let entry = ProviderEntry::new(definition);
        if entry.scope() == ProviderScope::Singleton && !entry.definition.is_alias() {
            let value = entry.definition.build(self)?;
            entry.seed_singleton(value)?;
        }
        self.insert_entry(token, entry)
    }

    pub async fn register_async(&self, definition: ProviderDefinition) -> Result<()> {
        let token = definition.token().clone();
        self.validate_registration(&token, &definition)?;

        let entry = ProviderEntry::new(definition);
        if entry.scope() == ProviderScope::Singleton && !entry.definition.is_alias() {
            entry.seed_singleton_async(self.clone()).await?;
        }
        self.insert_entry(token, entry)
    }

    pub fn insert<T>(&self, value: T) -> Result<()>
    where
        T: Send + Sync + 'static,
    {
        self.insert_arc(Arc::new(value))
    }

    pub fn insert_arc<T>(&self, value: Arc<T>) -> Result<()>
    where
        T: Send + Sync + 'static,
    {
        let token = ProviderToken::of::<T>();
        let entry = ProviderEntry::new(ProviderDefinition::from_arc(value));
        self.insert_entry(token, entry)
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_token::<T>(&ProviderToken::of::<T>())
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_token::<T>(&ProviderToken::named(token))
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional_token::<T>(&ProviderToken::of::<T>())
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional_token::<T>(&ProviderToken::named(token))
    }

    pub fn contains(&self, token: &ProviderToken) -> Result<bool> {
        Ok(self.get_entry(token)?.is_some())
    }

    pub fn contains_provider<T>(&self) -> Result<bool>
    where
        T: Send + Sync + 'static,
    {
        self.contains(&ProviderToken::of::<T>())
    }

    pub fn contains_named(&self, token: &str) -> Result<bool> {
        self.contains(&ProviderToken::named(token))
    }

    pub fn tokens(&self) -> Result<Vec<ProviderToken>> {
        let mut tokens = BTreeMap::new();
        self.collect_tokens(&mut tokens)?;
        Ok(tokens.into_keys().collect())
    }

    fn insert_entry(&self, token: ProviderToken, entry: ProviderEntry) -> Result<()> {
        let mut providers = self.write_providers()?;
        if providers.contains_key(&token) {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
        providers.insert(token, entry);
        Ok(())
    }

    fn validate_registration(
        &self,
        token: &ProviderToken,
        definition: &ProviderDefinition,
    ) -> Result<()> {
        if self.contains_local(token)? {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
        if definition.is_async_factory() && definition.scope() != ProviderScope::Singleton {
            return Err(BootError::Internal(format!(
                "async provider factories require singleton scope: {token}"
            )));
        }
        if definition.lifecycle().has_hooks() && definition.scope() != ProviderScope::Singleton {
            return Err(BootError::Internal(format!(
                "provider lifecycle hooks require singleton scope: {token}"
            )));
        }
        if definition.lifecycle().has_hooks() && definition.is_alias() {
            return Err(BootError::Internal(format!(
                "provider aliases cannot define lifecycle hooks: {token}"
            )));
        }
        Ok(())
    }

    pub(crate) fn add_visible_scope(&self, module_ref: ModuleRef) -> Result<()> {
        self.write_visible_scopes()?.push(module_ref);
        Ok(())
    }

    pub(crate) fn export_from(&self, module_ref: &ModuleRef, token: &ProviderToken) -> Result<()> {
        let entry = module_ref
            .get_entry(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;
        self.insert_entry(token.clone(), entry.with_owner(module_ref.clone()))
    }

    pub(crate) fn local_tokens(&self) -> Result<Vec<ProviderToken>> {
        Ok(self.read_providers()?.keys().cloned().collect())
    }

    pub(crate) fn initialize_local_providers(&self) -> Result<()> {
        for entry in self.local_entries()? {
            entry.on_module_init(self)?;
        }
        Ok(())
    }

    pub(crate) async fn bootstrap_local_providers(&self) -> Result<()> {
        for entry in self.local_entries()? {
            entry.on_application_bootstrap(self.clone()).await?;
        }
        Ok(())
    }

    pub(crate) async fn shutdown_local_providers(&self) -> Result<()> {
        let mut entries = self.local_entries()?;
        entries.reverse();
        for entry in entries {
            entry.on_application_shutdown(self.clone()).await?;
        }
        Ok(())
    }

    fn get_token<T>(&self, token: &ProviderToken) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let value = self
            .get_any(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;

        Arc::downcast::<T>(value).map_err(|_| BootError::ProviderTypeMismatch(token.to_string()))
    }

    fn get_optional_token<T>(&self, token: &ProviderToken) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        let value = self.get_any(token)?;
        match value {
            Some(value) => Arc::downcast::<T>(value)
                .map(Some)
                .map_err(|_| BootError::ProviderTypeMismatch(token.to_string())),
            None => Ok(None),
        }
    }

    fn get_any(&self, token: &ProviderToken) -> Result<Option<Arc<AnyProvider>>> {
        let mut alias_path = Vec::new();
        self.get_any_with_request_cache_inner(token, self.request_cache.clone(), &mut alias_path)
    }

    fn get_any_with_request_cache_inner(
        &self,
        token: &ProviderToken,
        request_cache: Option<ProviderCache>,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Option<Arc<AnyProvider>>> {
        if let Some(entry) = self.read_providers()?.get(token).cloned() {
            return entry.resolve(self, request_cache, alias_path).map(Some);
        }

        for scope in self.visible_scopes()? {
            if let Some(value) =
                scope.get_any_with_request_cache_inner(token, request_cache.clone(), alias_path)?
            {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    fn get_entry(&self, token: &ProviderToken) -> Result<Option<ProviderEntry>> {
        if let Some(entry) = self.read_providers()?.get(token).cloned() {
            return Ok(Some(entry));
        }

        for scope in self.visible_scopes()? {
            if let Some(entry) = scope.get_entry(token)? {
                return Ok(Some(entry));
            }
        }

        Ok(None)
    }

    fn contains_local(&self, token: &ProviderToken) -> Result<bool> {
        Ok(self.read_providers()?.contains_key(token))
    }

    fn collect_tokens(&self, tokens: &mut BTreeMap<ProviderToken, ()>) -> Result<()> {
        for token in self.read_providers()?.keys() {
            tokens.insert(token.clone(), ());
        }
        for scope in self.visible_scopes()? {
            scope.collect_tokens(tokens)?;
        }
        Ok(())
    }

    fn local_entries(&self) -> Result<Vec<ProviderEntry>> {
        Ok(self.read_providers()?.values().cloned().collect())
    }

    fn visible_scopes(&self) -> Result<Vec<ModuleRef>> {
        Ok(self.read_visible_scopes()?.clone())
    }

    fn read_providers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<ProviderToken, ProviderEntry>>> {
        self.providers
            .read()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }

    fn write_providers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<ProviderToken, ProviderEntry>>> {
        self.providers
            .write()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }

    fn read_visible_scopes(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<ModuleRef>>> {
        self.visible_scopes
            .read()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }

    fn write_visible_scopes(&self) -> Result<std::sync::RwLockWriteGuard<'_, Vec<ModuleRef>>> {
        self.visible_scopes
            .write()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }
}
