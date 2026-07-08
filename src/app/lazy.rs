use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderToken, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Provider-only reference returned by [`LazyModuleLoader`].
#[derive(Clone)]
pub struct LazyLoadedModule {
    name: String,
    module_ref: ModuleRef,
    exports: ModuleRef,
}

impl LazyLoadedModule {
    fn new(name: String, module_ref: ModuleRef, exports: ModuleRef) -> Self {
        Self {
            name,
            module_ref,
            exports,
        }
    }

    /// Module name reported by the loaded module.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Provider container for the loaded module.
    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    /// Resolve a typed provider from the loaded module.
    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get::<T>()
    }

    /// Resolve a named provider from the loaded module.
    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_named::<T>(token)
    }

    /// Resolve a typed provider when it is present in the loaded module graph.
    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_optional::<T>()
    }

    /// Resolve a named provider when it is present in the loaded module graph.
    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_optional_named::<T>(token)
    }
}

impl fmt::Debug for LazyLoadedModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyLoadedModule")
            .field("name", &self.name)
            .field("module_ref", &self.module_ref)
            .finish()
    }
}

/// Nest-style lazy module loader for provider-only module graphs.
#[derive(Clone)]
pub struct LazyModuleLoader {
    registry: Arc<LazyModuleRegistry>,
}

impl LazyModuleLoader {
    pub(crate) fn new(global_ref: ModuleRef) -> Self {
        Self {
            registry: Arc::new(LazyModuleRegistry::new(global_ref)),
        }
    }

    pub(crate) fn seed_module(
        &self,
        name: String,
        module_ref: ModuleRef,
        exports: ModuleRef,
    ) -> Result<()> {
        self.registry
            .seed_module(LazyLoadedModule::new(name, module_ref, exports))
    }

    /// Load a module on demand and return its provider container.
    ///
    /// Lazy-loaded modules are provider-only: controllers, routes, gateways,
    /// middleware, message patterns, and lifecycle hooks are not registered.
    pub fn load<M>(&self, module: M) -> Result<LazyLoadedModule>
    where
        M: Module,
    {
        self.load_arc(Arc::new(module))
    }

    /// Load a shared module on demand and return its provider container.
    pub fn load_arc(&self, module: Arc<dyn Module>) -> Result<LazyLoadedModule> {
        let mut visiting = Vec::new();
        self.load_arc_inner(module, &mut visiting)
    }

    /// Load a module with async singleton provider factories on demand.
    pub async fn load_async<M>(&self, module: M) -> Result<LazyLoadedModule>
    where
        M: Module,
    {
        self.load_arc_async(Arc::new(module)).await
    }

    /// Load a shared module with async singleton provider factories on demand.
    pub async fn load_arc_async(&self, module: Arc<dyn Module>) -> Result<LazyLoadedModule> {
        let mut visiting = Vec::new();
        self.load_arc_async_inner(module, &mut visiting).await
    }

    fn load_arc_inner(
        &self,
        module: Arc<dyn Module>,
        visiting: &mut Vec<String>,
    ) -> Result<LazyLoadedModule> {
        let name = validate_lazy_module_name(module.name())?;
        if let Some(cached) = self.registry.cached(name)? {
            return Ok(cached);
        }

        enter_lazy_module(visiting, name)?;
        let result = self.build_lazy_module(module, name, visiting);
        visiting.pop();
        result
    }

    fn build_lazy_module(
        &self,
        module: Arc<dyn Module>,
        name: &str,
        visiting: &mut Vec<String>,
    ) -> Result<LazyLoadedModule> {
        let mut imported_modules = Vec::new();
        for imported in module.imports() {
            imported_modules.push(self.load_arc_inner(imported, visiting)?);
        }

        let module_ref = self.create_module_ref(&imported_modules)?;
        for provider in module.providers()? {
            module_ref.register(provider)?;
        }
        module_ref.initialize_local_singletons()?;

        let exports = self.create_exports(&module_ref, module.exports()?)?;
        if module.is_global() {
            self.export_global(&exports)?;
        }

        let loaded = LazyLoadedModule::new(name.to_string(), module_ref, exports);
        self.registry.cache_module(loaded)
    }

    fn load_arc_async_inner<'a>(
        &'a self,
        module: Arc<dyn Module>,
        visiting: &'a mut Vec<String>,
    ) -> BoxFuture<'a, Result<LazyLoadedModule>> {
        Box::pin(async move {
            let name = validate_lazy_module_name(module.name())?;
            if let Some(cached) = self.registry.cached(name)? {
                return Ok(cached);
            }

            enter_lazy_module(visiting, name)?;
            let result = self.build_lazy_module_async(module, name, visiting).await;
            visiting.pop();
            result
        })
    }

    fn build_lazy_module_async<'a>(
        &'a self,
        module: Arc<dyn Module>,
        name: &'a str,
        visiting: &'a mut Vec<String>,
    ) -> BoxFuture<'a, Result<LazyLoadedModule>> {
        Box::pin(async move {
            let mut imported_modules = Vec::new();
            for imported in module.imports() {
                imported_modules.push(self.load_arc_async_inner(imported, visiting).await?);
            }

            let module_ref = self.create_module_ref(&imported_modules)?;
            for provider in module.providers()? {
                module_ref.register_async(provider).await?;
            }
            module_ref.initialize_local_singletons_async().await?;

            let exports = self.create_exports(&module_ref, module.exports()?)?;
            if module.is_global() {
                self.export_global(&exports)?;
            }

            let loaded = LazyLoadedModule::new(name.to_string(), module_ref, exports);
            self.registry.cache_module(loaded)
        })
    }

    fn create_module_ref(&self, imported_modules: &[LazyLoadedModule]) -> Result<ModuleRef> {
        let module_ref = ModuleRef::new();
        module_ref.add_visible_scope(self.registry.global_ref.clone())?;
        for imported in imported_modules {
            module_ref.add_visible_scope(imported.exports.clone())?;
        }
        Ok(module_ref)
    }

    fn create_exports(
        &self,
        module_ref: &ModuleRef,
        tokens: Vec<ProviderToken>,
    ) -> Result<ModuleRef> {
        let exports = ModuleRef::new();
        for token in tokens {
            exports.export_from(module_ref, &token)?;
        }
        Ok(exports)
    }

    fn export_global(&self, exports: &ModuleRef) -> Result<()> {
        for token in exports.local_tokens()? {
            self.registry.global_ref.export_from(exports, &token)?;
        }
        Ok(())
    }
}

impl fmt::Debug for LazyModuleLoader {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LazyModuleLoader").finish_non_exhaustive()
    }
}

struct LazyModuleRegistry {
    global_ref: ModuleRef,
    modules: RwLock<BTreeMap<String, LazyLoadedModule>>,
}

impl LazyModuleRegistry {
    fn new(global_ref: ModuleRef) -> Self {
        Self {
            global_ref,
            modules: RwLock::new(BTreeMap::new()),
        }
    }

    fn cached(&self, name: &str) -> Result<Option<LazyLoadedModule>> {
        Ok(self.read_modules()?.get(name).cloned())
    }

    fn seed_module(&self, module: LazyLoadedModule) -> Result<()> {
        self.write_modules()?.insert(module.name.clone(), module);
        Ok(())
    }

    fn cache_module(&self, module: LazyLoadedModule) -> Result<LazyLoadedModule> {
        let mut modules = self.write_modules()?;
        if let Some(cached) = modules.get(module.name()).cloned() {
            return Ok(cached);
        }
        modules.insert(module.name.clone(), module.clone());
        Ok(module)
    }

    fn read_modules(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, LazyLoadedModule>>> {
        self.modules
            .read()
            .map_err(|_| BootError::Internal("lazy module registry lock is poisoned".to_string()))
    }

    fn write_modules(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, LazyLoadedModule>>> {
        self.modules
            .write()
            .map_err(|_| BootError::Internal("lazy module registry lock is poisoned".to_string()))
    }
}

fn validate_lazy_module_name(name: &'static str) -> Result<&'static str> {
    if name.trim().is_empty() {
        return Err(BootError::EmptyModuleName);
    }
    Ok(name)
}

fn enter_lazy_module(visiting: &mut Vec<String>, name: &str) -> Result<()> {
    if let Some(index) = visiting.iter().position(|active| active == name) {
        let mut chain = visiting[index..].to_vec();
        chain.push(name.to_string());
        return Err(BootError::Internal(format!(
            "cyclic lazy module import detected: {}",
            chain.join(" -> ")
        )));
    }

    visiting.push(name.to_string());
    Ok(())
}
