use super::cache::{ProviderCache, ProviderCacheKey};
use super::module_ref::ModuleRef;
use super::resolution::{
    enter_resolution_stack, exit_resolution_stack, new_resolution_stack, ProviderResolutionStack,
};
use super::{AnyProvider, ProviderDefinition, ProviderScope, ProviderToken};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::sync::Arc;

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

    pub(crate) fn is_local_singleton(&self) -> bool {
        self.scope() == ProviderScope::Singleton && !self.definition.is_alias()
    }

    pub(crate) fn is_async_factory(&self) -> bool {
        self.definition.is_async_factory()
    }

    pub(crate) fn resolve(
        &self,
        module_ref: &ModuleRef,
        request_cache: Option<ProviderCache>,
        resolution_stack: &ProviderResolutionStack,
        alias_path: &mut Vec<ProviderToken>,
    ) -> Result<Arc<AnyProvider>> {
        let base_ref = self.owner.as_ref().unwrap_or(module_ref);
        if let Some(target) = self.definition.alias_target() {
            return self.resolve_alias(
                base_ref,
                target,
                request_cache,
                resolution_stack,
                alias_path,
            );
        }

        match self.scope() {
            ProviderScope::Singleton => self.resolve_singleton(base_ref, resolution_stack),
            ProviderScope::Transient => {
                let factory_ref =
                    self.factory_ref(base_ref, request_cache.clone(), resolution_stack);
                self.build_with_resolution_stack(&factory_ref, resolution_stack)
            }
            ProviderScope::Request => {
                let factory_ref =
                    self.factory_ref(base_ref, request_cache.clone(), resolution_stack);
                self.resolve_request(&factory_ref, request_cache, resolution_stack)
            }
        }
    }

    fn resolve_alias(
        &self,
        module_ref: &ModuleRef,
        target: &ProviderToken,
        request_cache: Option<ProviderCache>,
        resolution_stack: &ProviderResolutionStack,
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
        let value = module_ref.get_any_with_request_cache_inner(
            target,
            request_cache,
            resolution_stack,
            alias_path,
        )?;
        alias_path.pop();

        value.ok_or_else(|| BootError::MissingProvider(target.to_string()))
    }

    pub(crate) fn resolve_singleton(
        &self,
        module_ref: &ModuleRef,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        if let Some(value) = self
            .read_cache(&self.singleton)?
            .get(&self.cache_key)
            .cloned()
        {
            return Ok(value);
        }

        let factory_ref = module_ref.with_resolution_stack(Arc::clone(resolution_stack));
        let value = self.build_with_resolution_stack(&factory_ref, resolution_stack)?;
        self.write_cache(&self.singleton)?
            .insert(self.cache_key, Arc::clone(&value));
        Ok(value)
    }

    fn resolve_request(
        &self,
        module_ref: &ModuleRef,
        request_cache: Option<ProviderCache>,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        let Some(request_cache) = request_cache else {
            return self.build_with_resolution_stack(module_ref, resolution_stack);
        };

        if let Some(value) = self
            .read_cache(&request_cache)?
            .get(&self.cache_key)
            .cloned()
        {
            return Ok(value);
        }

        let value = self.build_with_resolution_stack(module_ref, resolution_stack)?;
        self.write_cache(&request_cache)?
            .insert(self.cache_key, Arc::clone(&value));
        Ok(value)
    }

    pub(crate) async fn seed_singleton_async(&self, module_ref: ModuleRef) -> Result<()> {
        let resolution_stack = new_resolution_stack();
        let module_ref = module_ref.with_resolution_stack(Arc::clone(&resolution_stack));
        enter_resolution_stack(&resolution_stack, self.definition.token())?;
        let result = self.definition.build_async(module_ref).await;
        let exit_result = exit_resolution_stack(&resolution_stack);
        let value = match (result, exit_result) {
            (Ok(value), Ok(())) => value,
            (Err(error), _) => return Err(error),
            (Ok(_), Err(error)) => return Err(error),
        };
        self.seed_singleton(value)
    }

    fn seed_singleton(&self, value: Arc<AnyProvider>) -> Result<()> {
        self.write_cache(&self.singleton)?
            .insert(self.cache_key, value);
        Ok(())
    }

    pub(crate) fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_module_init() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value = self.resolve_singleton(module_ref, &resolution_stack)?;
        hook(value, module_ref)
    }

    pub(crate) async fn on_application_bootstrap(&self, module_ref: ModuleRef) -> Result<()> {
        let Some(hook) = self.definition.lifecycle().on_application_bootstrap() else {
            return Ok(());
        };

        let resolution_stack = new_resolution_stack();
        let value = self.resolve_singleton(&module_ref, &resolution_stack)?;
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
        let value = self.resolve_singleton(&module_ref, &resolution_stack)?;
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
        let value = self.resolve_singleton(&module_ref, &resolution_stack)?;
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
        let value = self.resolve_singleton(&module_ref, &resolution_stack)?;
        hook(value, module_ref, signal).await
    }

    fn factory_ref(
        &self,
        module_ref: &ModuleRef,
        request_cache: Option<ProviderCache>,
        resolution_stack: &ProviderResolutionStack,
    ) -> ModuleRef {
        let factory_ref = module_ref.with_resolution_stack(Arc::clone(resolution_stack));
        match request_cache {
            Some(request_cache) => factory_ref.with_request_cache(request_cache),
            None => factory_ref,
        }
    }

    fn build_with_resolution_stack(
        &self,
        module_ref: &ModuleRef,
        resolution_stack: &ProviderResolutionStack,
    ) -> Result<Arc<AnyProvider>> {
        enter_resolution_stack(resolution_stack, self.definition.token())?;
        let result = self.definition.build(module_ref);
        let exit_result = exit_resolution_stack(resolution_stack);
        match (result, exit_result) {
            (Ok(value), Ok(())) => Ok(value),
            (Err(error), _) => Err(error),
            (Ok(_), Err(error)) => Err(error),
        }
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
