use super::cache::{new_provider_cache, ProviderCache, ProviderCacheKey};
use super::entry::ProviderEntry;
use super::provider_ref::ProviderRef;
use super::resolution::{
    enter_resolution_stack, new_resolution_stack, resolution_stack_is_empty,
    ProviderResolutionStack,
};
use super::{
    AnyProvider, ContextId, ContextIdFactory, FromModuleRef, ProviderDefinition, ProviderScope,
    ProviderToken,
};
use crate::{BootError, BoxFuture, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::sync::{Arc, RwLock};

/// Runtime provider container. This is Boot's Rust equivalent of Nest's ModuleRef.
#[derive(Clone, Default)]
pub struct ModuleRef {
    providers: Arc<RwLock<BTreeMap<ProviderToken, ProviderEntry>>>,
    provider_order: Arc<RwLock<Vec<ProviderToken>>>,
    visible_scopes: Arc<RwLock<Vec<ModuleRef>>>,
    context_id: Option<ContextId>,
    transient_cache: ProviderCache,
    inquirer: Option<ProviderCacheKey>,
    resolution_stack: Option<ProviderResolutionStack>,
}

#[derive(Default)]
struct AsyncProviderSeedState {
    complete: BTreeSet<ProviderCacheKey>,
    visiting: Vec<(ProviderCacheKey, ProviderToken)>,
}

impl AsyncProviderSeedState {
    fn enter(&mut self, entry: &ProviderEntry) -> Result<bool> {
        let cache_key = entry.cache_key();
        if self.complete.contains(&cache_key) {
            return Ok(false);
        }
        if let Some(index) = self
            .visiting
            .iter()
            .position(|(active, _)| *active == cache_key)
        {
            let mut chain = self.visiting[index..]
                .iter()
                .map(|(_, token)| token.to_string())
                .collect::<Vec<_>>();
            chain.push(entry.token().to_string());
            return Err(BootError::Internal(format!(
                "cyclic async provider dependency detected: {}",
                chain.join(" -> ")
            )));
        }

        self.visiting.push((cache_key, entry.token().clone()));
        Ok(true)
    }

    fn exit(&mut self, cache_key: ProviderCacheKey, complete: bool) -> Result<()> {
        let Some((active, _)) = self.visiting.pop() else {
            return Err(BootError::Internal(
                "async provider dependency stack underflow".to_string(),
            ));
        };
        if active != cache_key {
            return Err(BootError::Internal(
                "async provider dependency stack is inconsistent".to_string(),
            ));
        }
        if complete {
            self.complete.insert(cache_key);
        }
        Ok(())
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
        self.context_scope(&ContextIdFactory::create())
    }

    /// Bind this module view to an existing dependency-injection context.
    pub fn context_scope(&self, context_id: &ContextId) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: Some(context_id.clone()),
            transient_cache: self.transient_cache.clone(),
            inquirer: self.inquirer,
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    pub(crate) fn weak_context_scope(&self, context_id: &ContextId) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: Some(context_id.downgrade()),
            transient_cache: self.transient_cache.clone(),
            inquirer: self.inquirer,
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    /// Return the dependency-injection context attached to this module view.
    pub fn context_id(&self) -> Option<&ContextId> {
        self.context_id.as_ref()
    }

    pub(crate) fn with_resolution_stack(&self, resolution_stack: ProviderResolutionStack) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: self.context_id.clone(),
            transient_cache: self.transient_cache.clone(),
            inquirer: self.inquirer,
            resolution_stack: Some(resolution_stack),
        }
    }

    pub(crate) fn without_resolution_stack(&self) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: self.context_id.clone(),
            transient_cache: self.transient_cache.clone(),
            inquirer: self.inquirer,
            resolution_stack: None,
        }
    }

    pub(crate) fn with_inquirer(&self, inquirer: ProviderCacheKey) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: self.context_id.clone(),
            transient_cache: self.transient_cache.clone(),
            inquirer: Some(inquirer),
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    pub(crate) fn without_context(&self) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: None,
            transient_cache: self.transient_cache.clone(),
            inquirer: self.inquirer,
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    pub(crate) fn with_transient_cache(&self, transient_cache: ProviderCache) -> Self {
        Self {
            providers: Arc::clone(&self.providers),
            provider_order: Arc::clone(&self.provider_order),
            visible_scopes: Arc::clone(&self.visible_scopes),
            context_id: self.context_id.clone(),
            transient_cache,
            inquirer: self.inquirer,
            resolution_stack: self.resolution_stack.clone(),
        }
    }

    pub(crate) fn transient_cache(&self) -> ProviderCache {
        self.transient_cache.clone()
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

    /// Resolve a typed provider in a caller-supplied resolution context.
    pub fn resolve_with_context<T>(&self, context_id: &ContextId) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_token_with_context::<T>(&ProviderToken::of::<T>(), context_id)
    }

    /// Resolve a named provider in a fresh resolution context.
    pub fn resolve_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_token::<T>(&ProviderToken::named(token))
    }

    /// Resolve a named provider in a caller-supplied resolution context.
    pub fn resolve_named_with_context<T>(
        &self,
        token: &str,
        context_id: &ContextId,
    ) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_token_with_context::<T>(&ProviderToken::named(token), context_id)
    }

    /// Resolve a typed provider in a fresh resolution context when it exists.
    pub fn resolve_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token::<T>(&ProviderToken::of::<T>())
    }

    /// Resolve an optional typed provider in a caller-supplied context.
    pub fn resolve_optional_with_context<T>(&self, context_id: &ContextId) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token_with_context::<T>(&ProviderToken::of::<T>(), context_id)
    }

    /// Resolve a named provider in a fresh resolution context when it exists.
    pub fn resolve_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token::<T>(&ProviderToken::named(token))
    }

    /// Resolve an optional named provider in a caller-supplied context.
    pub fn resolve_optional_named_with_context<T>(
        &self,
        token: &str,
        context_id: &ContextId,
    ) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token_with_context::<T>(&ProviderToken::named(token), context_id)
    }

    /// Create an injectable value without registering it in the provider graph.
    pub fn create<T>(&self) -> Result<T>
    where
        T: FromModuleRef,
    {
        let inquirer = ProviderCacheKey::next();
        let token = ProviderToken::of::<T>();
        let (resolution_stack, _guard) =
            enter_resolution_stack(&new_resolution_stack(), inquirer, &token)?;
        let factory_ref = self
            .with_transient_cache(new_provider_cache())
            .with_inquirer(inquirer)
            .with_resolution_stack(resolution_stack);
        T::from_module_ref(&factory_ref)
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

    /// Return whether a typed provider requires a request-resolution context.
    ///
    /// This includes explicitly request-scoped providers and singleton or
    /// transient providers whose declared dependency tree reaches one.
    pub fn provider_is_contextual<T>(&self) -> Result<bool>
    where
        T: Send + Sync + 'static,
    {
        self.token_is_contextual(&ProviderToken::of::<T>())
    }

    /// Return the provider's declared cache scope after following aliases.
    pub fn provider_scope<T>(&self) -> Result<ProviderScope>
    where
        T: Send + Sync + 'static,
    {
        self.token_scope(&ProviderToken::of::<T>())
    }

    /// Return whether a named provider requires a request-resolution context.
    pub fn named_provider_is_contextual(&self, token: &str) -> Result<bool> {
        self.token_is_contextual(&ProviderToken::named(token))
    }

    /// Return a named provider's declared cache scope after following aliases.
    pub fn named_provider_scope(&self, token: &str) -> Result<ProviderScope> {
        self.token_scope(&ProviderToken::named(token))
    }

    pub(crate) fn token_is_contextual(&self, token: &ProviderToken) -> Result<bool> {
        let entry = self
            .get_entry(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;
        Ok(entry.resolution_plan(self)?.is_contextual())
    }

    pub(crate) fn token_scope(&self, token: &ProviderToken) -> Result<ProviderScope> {
        let entry = self
            .get_entry(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;
        Ok(entry.resolution_plan(self)?.scope())
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
            let plan = entry.resolution_plan(self)?;
            self.validate_resolution_plan(&entry, plan)?;
            if plan.scope() == ProviderScope::Singleton
                && !plan.is_contextual()
                && !entry.is_alias()
            {
                let resolution_stack = new_resolution_stack();
                entry.resolve_singleton(self, self.transient_cache(), &resolution_stack)?;
            }
        }
        Ok(())
    }

    pub(crate) fn validate_local_resolution_plans(&self) -> Result<()> {
        for entry in self.local_entries()? {
            let plan = entry.resolution_plan(self)?;
            self.validate_resolution_plan(&entry, plan)?;
        }
        Ok(())
    }

    pub(crate) async fn seed_local_async_singletons(&self) -> Result<()> {
        let entries = self.local_entries()?;
        for entry in &entries {
            let plan = entry.resolution_plan(self)?;
            self.validate_resolution_plan(entry, plan)?;
        }

        let mut state = AsyncProviderSeedState::default();
        for entry in entries {
            let plan = entry.resolution_plan(self)?;
            if plan.scope() == ProviderScope::Singleton
                && !plan.is_contextual()
                && !entry.is_alias()
                && entry.is_async_factory()
            {
                self.seed_async_dependency_tree(entry, &mut state).await?;
            }
        }
        Ok(())
    }

    fn seed_async_dependency_tree<'a>(
        &'a self,
        entry: ProviderEntry,
        state: &'a mut AsyncProviderSeedState,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let cache_key = entry.cache_key();
            if !state.enter(&entry)? {
                return Ok(());
            }

            let owner = entry.owner_module_ref(self);
            let result = async {
                let plan = entry.resolution_plan(&owner)?;
                owner.validate_resolution_plan(&entry, plan)?;
                for dependency in entry.eager_dependency_entries(&owner)? {
                    owner.seed_async_dependency_tree(dependency, state).await?;
                }
                if entry.is_async_factory() {
                    entry.seed_singleton_async(owner).await?;
                }
                Ok(())
            }
            .await;
            let exit_result = state.exit(cache_key, result.is_ok());
            match (result, exit_result) {
                (Ok(()), Ok(())) => Ok(()),
                (Err(error), _) => Err(error),
                (Ok(()), Err(error)) => Err(error),
            }
        })
    }

    fn validate_resolution_plan(
        &self,
        entry: &ProviderEntry,
        plan: super::entry::ProviderResolutionPlan,
    ) -> Result<()> {
        if plan.is_contextual() && entry.is_async_factory() {
            return Err(BootError::Internal(format!(
                "async provider `{}` cannot depend on a request-context provider",
                entry.token()
            )));
        }
        if plan.is_contextual() && entry.has_lifecycle_hooks() {
            return Err(BootError::Internal(format!(
                "provider `{}` cannot use singleton lifecycle hooks because request scope propagated through its dependencies",
                entry.token()
            )));
        }
        Ok(())
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
        self.resolve_token_with_context(token, &ContextIdFactory::create())
    }

    pub(crate) fn resolve_token_with_context<T>(
        &self,
        token: &ProviderToken,
        context_id: &ContextId,
    ) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.context_scope(context_id).get_token(token)
    }

    pub(crate) fn resolve_optional_token<T>(&self, token: &ProviderToken) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.resolve_optional_token_with_context(token, &ContextIdFactory::create())
    }

    pub(crate) fn resolve_optional_token_with_context<T>(
        &self,
        token: &ProviderToken,
        context_id: &ContextId,
    ) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.context_scope(context_id).get_optional_token(token)
    }

    fn get_any(&self, token: &ProviderToken) -> Result<Option<Arc<AnyProvider>>> {
        let mut alias_path = Vec::new();
        let resolution_stack = match &self.resolution_stack {
            Some(resolution_stack) if !resolution_stack_is_empty(resolution_stack) => {
                Arc::clone(resolution_stack)
            }
            Some(_) | None => new_resolution_stack(),
        };
        self.get_any_with_context_inner(
            token,
            self.context_id.clone(),
            self.transient_cache.clone(),
            self.inquirer,
            &resolution_stack,
            &mut alias_path,
        )
    }

    pub(crate) fn get_any_with_context_inner(
        &self,
        token: &ProviderToken,
        context_id: Option<ContextId>,
        transient_cache: ProviderCache,
        inquirer: Option<ProviderCacheKey>,
        resolution_stack: &ProviderResolutionStack,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Option<Arc<AnyProvider>>> {
        if let Some(entry) = self.read_providers()?.get(token).cloned() {
            return entry
                .resolve(
                    self,
                    context_id,
                    transient_cache,
                    inquirer,
                    resolution_stack,
                    alias_path,
                )
                .map(Some);
        }

        for scope in self.visible_scopes()? {
            if let Some(value) = scope.get_any_with_context_inner(
                token,
                context_id.clone(),
                transient_cache.clone(),
                inquirer,
                resolution_stack,
                alias_path,
            )? {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    pub(crate) fn get_entry(&self, token: &ProviderToken) -> Result<Option<ProviderEntry>> {
        if let Some(entry) = self.read_providers()?.get(token).cloned() {
            return Ok(Some(entry.with_owner(self.clone())));
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
