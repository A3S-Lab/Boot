use super::cache::{ProviderCache, ProviderCacheKey, ProviderInstanceKey};
use super::module_ref::ModuleRef;
use super::resolution::{
    ensure_not_resolving, enter_resolution_stack, new_resolution_stack, resolution_chain_with,
    ProviderResolutionStack,
};
use super::{AnyProvider, ContextId, ProviderDefinition, ProviderScope, ProviderToken};
use crate::{BootError, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct ProviderResolutionPlan {
    scope: ProviderScope,
    contextual: bool,
}

impl ProviderResolutionPlan {
    fn declared(scope: ProviderScope) -> Self {
        Self {
            scope,
            contextual: scope == ProviderScope::Request,
        }
    }

    pub(crate) fn scope(self) -> ProviderScope {
        self.scope
    }

    pub(crate) fn is_contextual(self) -> bool {
        self.contextual
    }
}

#[derive(Default)]
struct ProviderPlanState {
    visiting: BTreeSet<ProviderCacheKey>,
    memo: BTreeMap<ProviderCacheKey, ProviderResolutionPlan>,
}

#[derive(Clone)]
pub(crate) struct ProviderEntry {
    cache_key: ProviderCacheKey,
    definition: ProviderDefinition,
    singleton: ProviderCache,
    owner: Option<ModuleRef>,
}

impl ProviderEntry {
    pub(crate) fn new(definition: ProviderDefinition) -> Self {
        Self {
            cache_key: ProviderCacheKey::next(),
            definition,
            singleton: super::cache::new_provider_cache(),
            owner: None,
        }
    }

    pub(crate) fn with_owner(mut self, owner: ModuleRef) -> Self {
        if self.owner.is_none() {
            self.owner = Some(owner);
        }
        self
    }

    pub(crate) fn scope(&self) -> ProviderScope {
        self.definition.scope()
    }

    pub(crate) fn cache_key(&self) -> ProviderCacheKey {
        self.cache_key
    }

    pub(crate) fn token(&self) -> &ProviderToken {
        self.definition.token()
    }

    pub(crate) fn is_alias(&self) -> bool {
        self.definition.is_alias()
    }

    pub(crate) fn is_async_factory(&self) -> bool {
        self.definition.is_async_factory()
    }

    pub(crate) fn has_lifecycle_hooks(&self) -> bool {
        self.definition.lifecycle().has_hooks()
    }

    pub(crate) fn resolution_plan(&self, module_ref: &ModuleRef) -> Result<ProviderResolutionPlan> {
        self.resolution_plan_inner(module_ref, &mut ProviderPlanState::default())
    }

    pub(crate) fn owner_module_ref(&self, fallback: &ModuleRef) -> ModuleRef {
        self.owner.clone().unwrap_or_else(|| fallback.clone())
    }

    /// Resolve the eager edges used to order async singleton factories.
    ///
    /// Alias targets are authoritative even if callers replaced the alias's
    /// dependency metadata. Required lazy edges are still checked for
    /// visibility by resolution planning, but are intentionally excluded here.
    pub(crate) fn eager_dependency_entries(
        &self,
        module_ref: &ModuleRef,
    ) -> Result<Vec<ProviderEntry>> {
        let base_ref = self.owner.as_ref().unwrap_or(module_ref);
        if let Some(target) = self.definition.alias_target() {
            return base_ref
                .get_entry(target)?
                .map(|entry| vec![entry])
                .ok_or_else(|| BootError::MissingProvider(target.to_string()));
        }

        let mut entries = Vec::new();
        if let Some(dependencies) = self.definition.dependencies() {
            for dependency in dependencies {
                let Some(entry) = base_ref.get_entry(dependency.token())? else {
                    if dependency.is_optional() {
                        continue;
                    }
                    return Err(BootError::MissingProvider(dependency.token().to_string()));
                };
                if !dependency.is_lazy() {
                    entries.push(entry);
                }
            }
        }
        Ok(entries)
    }

    fn resolution_plan_inner(
        &self,
        module_ref: &ModuleRef,
        state: &mut ProviderPlanState,
    ) -> Result<ProviderResolutionPlan> {
        if let Some(plan) = state.memo.get(&self.cache_key) {
            return Ok(*plan);
        }

        let declared = ProviderResolutionPlan::declared(self.scope());
        if !state.visiting.insert(self.cache_key) {
            return Ok(declared);
        }

        let base_ref = self.owner.as_ref().unwrap_or(module_ref);
        let result = if let Some(target) = self.definition.alias_target() {
            let target = base_ref
                .get_entry(target)?
                .ok_or_else(|| BootError::MissingProvider(target.to_string()))?;
            target.resolution_plan_inner(base_ref, state)
        } else {
            let mut plan = declared;
            if let Some(dependencies) = self.definition.dependencies() {
                for dependency in dependencies {
                    let Some(entry) = base_ref.get_entry(dependency.token())? else {
                        if dependency.is_optional() {
                            continue;
                        }
                        return Err(BootError::MissingProvider(dependency.token().to_string()));
                    };
                    if !dependency.is_lazy()
                        && entry.resolution_plan_inner(base_ref, state)?.contextual
                    {
                        plan.contextual = true;
                    }
                }
            }
            Ok(plan)
        };

        state.visiting.remove(&self.cache_key);
        if let Ok(plan) = result {
            state.memo.insert(self.cache_key, plan);
        }
        result
    }

    pub(crate) fn resolve(
        &self,
        module_ref: &ModuleRef,
        context_id: Option<ContextId>,
        transient_cache: ProviderCache,
        inquirer: Option<ProviderCacheKey>,
        resolution_stack: &ProviderResolutionStack,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Arc<AnyProvider>> {
        let base_ref = self.owner.as_ref().unwrap_or(module_ref);
        if self.definition.alias_target().is_some() {
            return self.resolve_alias(
                base_ref,
                context_id,
                transient_cache,
                inquirer,
                resolution_stack,
                alias_path,
            );
        }

        let plan = self.resolution_plan(base_ref)?;
        match plan.scope() {
            ProviderScope::Singleton if !plan.is_contextual() => {
                self.resolve_singleton(base_ref, transient_cache, resolution_stack)
            }
            ProviderScope::Singleton | ProviderScope::Request => {
                let factory_ref = self.factory_ref(
                    base_ref,
                    context_id.clone(),
                    transient_cache,
                    resolution_stack,
                );
                self.resolve_contextual(&factory_ref, context_id, resolution_stack)
            }
            ProviderScope::Transient => {
                if plan.is_contextual() && context_id.is_none() {
                    return Err(self.missing_request_context_error(resolution_stack));
                }
                let factory_ref = self.factory_ref(
                    base_ref,
                    context_id.clone(),
                    transient_cache.clone(),
                    resolution_stack,
                );
                self.resolve_transient(
                    &factory_ref,
                    context_id,
                    transient_cache,
                    inquirer,
                    resolution_stack,
                )
            }
        }
    }

    fn resolve_alias(
        &self,
        module_ref: &ModuleRef,
        context_id: Option<ContextId>,
        transient_cache: ProviderCache,
        inquirer: Option<ProviderCacheKey>,
        resolution_stack: &ProviderResolutionStack,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Arc<AnyProvider>> {
        let target = self.definition.alias_target().ok_or_else(|| {
            BootError::Internal("provider alias is missing its target".to_string())
        })?;
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
        let value = module_ref.get_any_with_context_inner(
            target,
            context_id,
            transient_cache,
            inquirer,
            resolution_stack,
            alias_path,
        )?;
        alias_path.pop();

        value.ok_or_else(|| BootError::MissingProvider(target.to_string()))
    }

    pub(crate) fn resolve_singleton(
        &self,
        module_ref: &ModuleRef,
        transient_cache: ProviderCache,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        ensure_not_resolving(resolution_stack, self.cache_key, self.definition.token())?;
        self.singleton.get_or_try_insert_with(self.cache_key, || {
            let factory_ref = self.factory_ref(module_ref, None, transient_cache, resolution_stack);
            self.build_with_resolution_stack(&factory_ref, resolution_stack)
        })
    }

    fn resolve_contextual(
        &self,
        module_ref: &ModuleRef,
        context_id: Option<ContextId>,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        let Some(context_id) = context_id else {
            return Err(self.missing_request_context_error(resolution_stack));
        };
        ensure_not_resolving(resolution_stack, self.cache_key, self.definition.token())?;
        context_id
            .cache()?
            .get_or_try_insert_with(self.cache_key, || {
                self.build_with_resolution_stack(module_ref, resolution_stack)
            })
    }

    fn resolve_transient(
        &self,
        module_ref: &ModuleRef,
        context_id: Option<ContextId>,
        transient_cache: ProviderCache,
        inquirer: Option<ProviderCacheKey>,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        let inquirer = match (context_id.as_ref(), inquirer) {
            (Some(_), None) => Some(self.cache_key),
            (_, inquirer) => inquirer,
        };
        let Some(inquirer) = inquirer else {
            return self.build_with_resolution_stack(module_ref, resolution_stack);
        };

        ensure_not_resolving(resolution_stack, self.cache_key, self.definition.token())?;
        let cache = match context_id.as_ref() {
            Some(context_id) => context_id.cache()?,
            None => transient_cache,
        };
        cache.get_or_try_insert_with(
            ProviderInstanceKey::Transient {
                provider: self.cache_key,
                inquirer,
            },
            || self.build_with_resolution_stack(module_ref, resolution_stack),
        )
    }

    fn missing_request_context_error(
        &self,
        resolution_stack: &ProviderResolutionStack,
    ) -> BootError {
        let chain = resolution_chain_with(resolution_stack, self.definition.token());
        BootError::Internal(format!(
            "contextual provider chain `{chain}` requires an active request scope; use ModuleRef::resolve(...) for an isolated context or declare factory dependencies so request scope can propagate"
        ))
    }

    pub(crate) async fn seed_singleton_async(&self, module_ref: ModuleRef) -> Result<()> {
        if self.singleton.contains(self.cache_key)? {
            return Ok(());
        }
        let (resolution_stack, _guard) = enter_resolution_stack(
            &new_resolution_stack(),
            self.cache_key,
            self.definition.token(),
        )?;
        let module_ref = self.factory_ref(
            &module_ref,
            None,
            module_ref.transient_cache(),
            &resolution_stack,
        );
        let value = self.definition.build_async(module_ref).await?;
        self.seed_singleton(value)
    }

    fn seed_singleton(&self, value: Arc<AnyProvider>) -> Result<()> {
        self.singleton.insert(self.cache_key, value)
    }

    pub(crate) fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_module_init() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value =
            self.resolve_singleton(module_ref, module_ref.transient_cache(), &resolution_stack)?;
        hook(value, module_ref)
    }

    pub(crate) async fn on_application_bootstrap(&self, module_ref: ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_application_bootstrap() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value =
            self.resolve_singleton(&module_ref, module_ref.transient_cache(), &resolution_stack)?;
        hook(value, module_ref).await
    }

    pub(crate) async fn on_module_destroy(
        &self,
        module_ref: ModuleRef,
        signal: Option<String>,
    ) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_module_destroy() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value =
            self.resolve_singleton(&module_ref, module_ref.transient_cache(), &resolution_stack)?;
        hook(value, module_ref, signal).await
    }

    pub(crate) async fn before_application_shutdown(
        &self,
        module_ref: ModuleRef,
        signal: Option<String>,
    ) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().before_application_shutdown() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value =
            self.resolve_singleton(&module_ref, module_ref.transient_cache(), &resolution_stack)?;
        hook(value, module_ref, signal).await
    }

    pub(crate) async fn on_application_shutdown(
        &self,
        module_ref: ModuleRef,
        signal: Option<String>,
    ) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_application_shutdown() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value =
            self.resolve_singleton(&module_ref, module_ref.transient_cache(), &resolution_stack)?;
        hook(value, module_ref, signal).await
    }

    fn factory_ref(
        &self,
        module_ref: &ModuleRef,
        context_id: Option<ContextId>,
        transient_cache: ProviderCache,
        resolution_stack: &ProviderResolutionStack,
    ) -> ModuleRef {
        let factory_ref = module_ref
            .with_transient_cache(transient_cache)
            .with_inquirer(self.cache_key)
            .with_resolution_stack(Arc::clone(resolution_stack));
        match context_id {
            Some(context_id) => factory_ref.weak_context_scope(&context_id),
            None => factory_ref.without_context(),
        }
    }

    fn build_with_resolution_stack(
        &self,
        module_ref: &ModuleRef,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        let (resolution_stack, _guard) =
            enter_resolution_stack(resolution_stack, self.cache_key, self.definition.token())?;
        self.definition
            .build(&module_ref.with_resolution_stack(resolution_stack))
    }
}
