use super::cache::{new_provider_cache, ProviderCache};
use super::entry::ProviderEntry;
use super::provider_ref::ProviderRef;
use super::resolution::{new_resolution_stack, ProviderResolutionStack};
use super::{AnyProvider, FromModuleRef, ProviderDefinition, ProviderScope, ProviderToken};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Runtime provider container. This is Boot's Rust equivalent of Nest's ModuleRef.
#[derive(Clone, Default)]
pub struct ModuleRef {
    providers: Arc<RwLock<BTreeMap<ProviderToken, ProviderEntry>>>,
    provider_order: Arc<RwLock<Vec<ProviderToken>>>,
    visible_scopes: Arc<RwLock<Vec<ModuleRef>>>,
    request_cache: Option<ProviderCache>,
    resolution_stack: Option<ProviderResolutionStack>,
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
        self.with_request_cache(new_provider_cache())
    }

    pub(crate) fn with_request_cache(&self, request_cache: ProviderCache) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            request_cache: Some(request_cache),
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    pub(crate) fn with_resolution_stack(&self, resolution_stack: ProviderResolutionStack) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            request_cache: self.request_cache.clone(),
            resolution_stack: Some(resolution_stack),
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
        self.insert_entry(token, entry)
    }

    pub async fn register_async(&self, definition: ProviderDefinition) -> Result<()> {
        let token = definition.token().clone();
        self.validate_registration(&token, &definition)?;

        let entry = ProviderEntry::new(definition);
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

    /// Create a lazy provider handle for a typed dependency.
    pub fn provider_ref<T>(&self) -> ProviderRef<T>
    where
        T: Send + Sync + 'static,
    {
        ProviderRef::new(self.clone(), ProviderToken::of::<T>())
    }

    /// Create a lazy provider handle for a named dependency.
    pub fn named_provider_ref<T>(&self, token: &str) -> ProviderRef<T>
    where
        T: Send + Sync + 'static,
    {
        ProviderRef::new(self.clone(), ProviderToken::named(token))
    }

    /// Create a lazy provider handle only when the typed dependency is visible.
    pub fn optional_provider_ref<T>(&self) -> Result<Option<ProviderRef<T>>>
    where
        T: Send + Sync + 'static,
    {
        if self.contains_provider::<T>()? {
            Ok(Some(self.provider_ref::<T>()))
        } else {
            Ok(None)
        }
    }

    /// Create a lazy provider handle only when the named dependency is visible.
    pub fn optional_named_provider_ref<T>(&self, token: &str) -> Result<Option<ProviderRef<T>>>
    where
        T: Send + Sync + 'static,
    {
        if self.contains_named(token)? {
            Ok(Some(self.named_provider_ref::<T>(token)))
        } else {
            Ok(None)
        }
    }

    /// Resolve a typed provider in a fresh resolution context.
    ///
    /// This mirrors Nest's `ModuleRef.resolve(...)`: singleton providers reuse
    /// their application instance, while request-scoped dependencies share one
    /// temporary context for this resolution and transient providers are rebuilt.
    pub fn resolve<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_token::<T>(&ProviderToken::of::<T>())
    }

    /// Resolve a named provider in a fresh resolution context.
    pub fn resolve_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_token::<T>(&ProviderToken::named(token))
    }

    /// Resolve a typed provider in a fresh resolution context when it exists.
    pub fn resolve_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token::<T>(&ProviderToken::of::<T>())
    }

    /// Resolve a named provider in a fresh resolution context when it exists.
    pub fn resolve_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token::<T>(&ProviderToken::named(token))
    }

    /// Create an injectable value without registering it in the provider graph.
    pub fn create<T>(&self) -> Result<T>
    where
        T: FromModuleRef,
    {
        T::from_module_ref(self)
    }

    /// Create an injectable `Arc<T>` without registering it in the provider graph.
    pub fn create_arc<T>(&self) -> Result<Arc<T>>
    where
        T: FromModuleRef,
    {
        Ok(Arc::new(self.create::<T>()?))
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
        let mut provider_order = self.write_provider_order()?;
        let mut providers = self.write_providers()?;
        if providers.contains_key(&token) {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
        providers.insert(token.clone(), entry);
        provider_order.push(token);
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
        Ok(self.read_provider_order()?.clone())
    }

    pub(crate) fn initialize_local_singletons(&self) -> Result<()> {
        for entry in self.local_entries()? {
            if entry.is_local_singleton() {
                let resolution_stack = new_resolution_stack();
                entry.resolve_singleton(self, &resolution_stack)?;
            }
        }
        Ok(())
    }

    pub(crate) async fn initialize_local_singletons_async(&self) -> Result<()> {
        for entry in self.local_entries()? {
            if entry.is_local_singleton() && entry.is_async_factory() {
                entry.seed_singleton_async(self.clone()).await?;
            }
        }

        self.initialize_local_singletons()
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

    pub(crate) async fn destroy_local_providers(&self, signal: Option<String>) -> Result<()> {
        let mut entries = self.local_entries()?;
        entries.reverse();
        for entry in entries {
            entry
                .on_module_destroy(self.clone(), signal.clone())
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn before_application_shutdown_local_providers(
        &self,
        signal: Option<String>,
    ) -> Result<()> {
        let mut entries = self.local_entries()?;
        entries.reverse();
        for entry in entries {
            entry
                .before_application_shutdown(self.clone(), signal.clone())
                .await?;
        }
        Ok(())
    }

    pub(crate) async fn shutdown_local_providers(&self, signal: Option<String>) -> Result<()> {
        let mut entries = self.local_entries()?;
        entries.reverse();
        for entry in entries {
            entry
                .on_application_shutdown(self.clone(), signal.clone())
                .await?;
        }
        Ok(())
    }

    pub(crate) fn get_token<T>(&self, token: &ProviderToken) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let value = self
            .get_any(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;

        Arc::downcast::<T>(value).map_err(|_| BootError::ProviderTypeMismatch(token.to_string()))
    }

    pub(crate) fn get_optional_token<T>(&self, token: &ProviderToken) -> Result<Option<Arc<T>>>
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

    pub(crate) fn resolve_token<T>(&self, token: &ProviderToken) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.request_scope().get_token(token)
    }

    pub(crate) fn resolve_optional_token<T>(&self, token: &ProviderToken) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.request_scope().get_optional_token(token)
    }

    fn get_any(&self, token: &ProviderToken) -> Result<Option<Arc<AnyProvider>>> {
        let mut alias_path = Vec::new();
        let resolution_stack = self
            .resolution_stack
            .clone()
            .unwrap_or_else(new_resolution_stack);
        self.get_any_with_request_cache_inner(
            token,
            self.request_cache.clone(),
            &resolution_stack,
            &mut alias_path,
        )
    }

    pub(crate) fn get_any_with_request_cache_inner(
        &self,
        token: &ProviderToken,
        request_cache: Option<ProviderCache>,
        resolution_stack: &ProviderResolutionStack,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Option<Arc<AnyProvider>>> {
        if let Some(entry) = self.read_providers()?.get(token).cloned() {
            return entry
                .resolve(self, request_cache, resolution_stack, alias_path)
                .map(Some);
        }

        for scope in self.visible_scopes()? {
            if let Some(value) = scope.get_any_with_request_cache_inner(
                token,
                request_cache.clone(),
                resolution_stack,
                alias_path,
            )? {
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
        let provider_order = self.read_provider_order()?.clone();
        let providers = self.read_providers()?;
        let mut entries = Vec::with_capacity(provider_order.len());
        for token in provider_order {
            if let Some(entry) = providers.get(&token) {
                entries.push(entry.clone());
            }
        }
        Ok(entries)
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

    fn read_provider_order(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<ProviderToken>>> {
        self.provider_order
            .read()
            .map_err(|_| BootError::Internal("provider order lock is poisoned".to_string()))
    }

    fn write_provider_order(&self) -> Result<std::sync::RwLockWriteGuard<'_, Vec<ProviderToken>>> {
        self.provider_order
            .write()
            .map_err(|_| BootError::Internal("provider order lock is poisoned".to_string()))
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
