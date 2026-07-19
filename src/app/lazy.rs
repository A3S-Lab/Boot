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
    /// Newly loaded global modules are rejected because changing global
    /// visibility after singleton initialization would make the existing
    /// provider graph inconsistent.
    pub fn load<M>(&self, module: M) -> Result<LazyLoadedModule>
    where
        M: Module,
    {
        self.load_arc(Arc::new(module))
    }

    /// Load a shared module on demand and return its provider container.
    pub fn load_arc(&self, module: Arc<dyn Module>) -> Result<LazyLoadedModule> {
        let name = validate_lazy_module_name(module.name())?;
        if let Some(cached) = self.registry.cached(name)? {
            return Ok(cached);
        }

        let mut graph = LazyModuleGraph::default();
        let loaded = self.register_arc_inner(module, &mut graph, false)?;
        graph.validate()?;
        graph.initialize()?;
        self.registry.cache_modules(&graph.pending)?;
        self.registry
            .cached(loaded.name())?
            .ok_or_else(|| missing_cached_lazy_module(loaded.name()))
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
        let name = validate_lazy_module_name(module.name())?;
        if let Some(cached) = self.registry.cached(name)? {
            return Ok(cached);
        }

        let mut graph = LazyModuleGraph::default();
        let loaded = self
            .register_arc_async_inner(module, &mut graph, false)
            .await?;
        graph.validate()?;
        graph.initialize_async().await?;
        self.registry.cache_modules(&graph.pending)?;
        self.registry
            .cached(loaded.name())?
            .ok_or_else(|| missing_cached_lazy_module(loaded.name()))
    }

    fn register_arc_inner(
        &self,
        module: Arc<dyn Module>,
        graph: &mut LazyModuleGraph,
        allow_active: bool,
    ) -> Result<LazyLoadedModule> {
        let name = validate_lazy_module_name(module.name())?;
        if let Some(cached) = self.registry.cached(name)? {
            return Ok(cached);
        }
        if let Some(registered) = graph.registered.get(name) {
            return Ok(registered.clone());
        }
        if let Some(active) = graph.active.get(name) {
            if allow_active {
                return Ok(active.clone());
            }
            return Err(cyclic_lazy_module_error(&graph.visiting, name));
        }
        reject_lazy_global(module.as_ref(), name)?;

        enter_lazy_module(&mut graph.visiting, name)?;
        let loaded = LazyLoadedModule::new(name.to_string(), ModuleRef::new(), ModuleRef::new());
        graph.active.insert(name.to_string(), loaded.clone());
        let result = self.register_lazy_module(module, &loaded, graph);
        graph.active.remove(name);
        graph.visiting.pop();
        result?;

        graph.registered.insert(name.to_string(), loaded.clone());
        graph.pending.push(loaded.clone());
        Ok(loaded)
    }

    fn register_lazy_module(
        &self,
        module: Arc<dyn Module>,
        loaded: &LazyLoadedModule,
        graph: &mut LazyModuleGraph,
    ) -> Result<()> {
        let mut imported_modules = Vec::new();
        for imported in module.imports() {
            imported_modules.push(self.register_arc_inner(imported, graph, false)?);
        }
        for imported in module.forward_imports() {
            imported_modules.push(self.register_arc_inner(imported, graph, true)?);
        }

        self.prepare_module_ref(&loaded.module_ref, &imported_modules)?;
        for provider in module.providers()? {
            reject_lazy_provider_enhancers(&provider, loaded.name())?;
            loaded.module_ref.register(provider)?;
        }
        self.populate_exports(&loaded.exports, &loaded.module_ref, module.exports()?)
    }

    fn register_arc_async_inner<'a>(
        &'a self,
        module: Arc<dyn Module>,
        graph: &'a mut LazyModuleGraph,
        allow_active: bool,
    ) -> BoxFuture<'a, Result<LazyLoadedModule>> {
        Box::pin(async move {
            let name = validate_lazy_module_name(module.name())?;
            if let Some(cached) = self.registry.cached(name)? {
                return Ok(cached);
            }
            if let Some(registered) = graph.registered.get(name) {
                return Ok(registered.clone());
            }
            if let Some(active) = graph.active.get(name) {
                if allow_active {
                    return Ok(active.clone());
                }
                return Err(cyclic_lazy_module_error(&graph.visiting, name));
            }
            reject_lazy_global(module.as_ref(), name)?;

            enter_lazy_module(&mut graph.visiting, name)?;
            let loaded =
                LazyLoadedModule::new(name.to_string(), ModuleRef::new(), ModuleRef::new());
            graph.active.insert(name.to_string(), loaded.clone());
            let result = self
                .register_lazy_module_async(module, &loaded, graph)
                .await;
            graph.active.remove(name);
            graph.visiting.pop();
            result?;

            graph.registered.insert(name.to_string(), loaded.clone());
            graph.pending.push(loaded.clone());
            Ok(loaded)
        })
    }

    fn register_lazy_module_async<'a>(
        &'a self,
        module: Arc<dyn Module>,
        loaded: &'a LazyLoadedModule,
        graph: &'a mut LazyModuleGraph,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            let mut imported_modules = Vec::new();
            for imported in module.imports() {
                imported_modules.push(
                    self.register_arc_async_inner(imported, graph, false)
                        .await?,
                );
            }
            for imported in module.forward_imports() {
                imported_modules.push(self.register_arc_async_inner(imported, graph, true).await?);
            }

            self.prepare_module_ref(&loaded.module_ref, &imported_modules)?;
            for provider in module.providers()? {
                reject_lazy_provider_enhancers(&provider, loaded.name())?;
                loaded.module_ref.register_async(provider).await?;
            }
            self.populate_exports(&loaded.exports, &loaded.module_ref, module.exports()?)
        })
    }

    fn prepare_module_ref(
        &self,
        module_ref: &ModuleRef,
        imported_modules: &[LazyLoadedModule],
    ) -> Result<()> {
        module_ref.add_visible_scope(self.registry.global_ref.clone())?;
        for imported in imported_modules {
            module_ref.add_visible_scope(imported.exports.clone())?;
        }
        Ok(())
    }

    fn populate_exports(
        &self,
        exports: &ModuleRef,
        module_ref: &ModuleRef,
        tokens: Vec<ProviderToken>,
    ) -> Result<()> {
        for token in tokens {
            exports.export_from(module_ref, &token)?;
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

    fn cache_modules(&self, pending: &[LazyLoadedModule]) -> Result<()> {
        let mut modules = self.write_modules()?;
        for module in pending {
            modules
                .entry(module.name.clone())
                .or_insert_with(|| module.clone());
        }
        Ok(())
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

#[derive(Default)]
struct LazyModuleGraph {
    registered: BTreeMap<String, LazyLoadedModule>,
    active: BTreeMap<String, LazyLoadedModule>,
    pending: Vec<LazyLoadedModule>,
    visiting: Vec<String>,
}

impl LazyModuleGraph {
    fn validate(&self) -> Result<()> {
        for module in &self.pending {
            module.module_ref.validate_local_resolution_plans()?;
        }
        Ok(())
    }

    fn initialize(&self) -> Result<()> {
        for module in &self.pending {
            module.module_ref.initialize_local_singletons()?;
        }
        Ok(())
    }

    async fn initialize_async(&self) -> Result<()> {
        for module in &self.pending {
            module.module_ref.seed_local_async_singletons().await?;
        }
        for module in &self.pending {
            module.module_ref.initialize_local_singletons()?;
        }
        Ok(())
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
        return Err(cyclic_lazy_module_error(&visiting[index..], name));
    }

    visiting.push(name.to_string());
    Ok(())
}

fn cyclic_lazy_module_error(visiting: &[String], name: &str) -> BootError {
    let index = visiting
        .iter()
        .position(|active| active == name)
        .unwrap_or(0);
    let mut chain = visiting[index..].to_vec();
    chain.push(name.to_string());
    BootError::Internal(format!(
        "cyclic lazy module import detected: {}",
        chain.join(" -> ")
    ))
}

fn reject_lazy_global(module: &dyn Module, name: &str) -> Result<()> {
    if module.is_global() {
        return Err(BootError::Internal(format!(
            "lazy-loaded global module `{name}` would change the finalized application provider graph; register global modules eagerly"
        )));
    }
    Ok(())
}

fn reject_lazy_provider_enhancers(
    provider: &crate::ProviderDefinition,
    module_name: &str,
) -> Result<()> {
    if !provider.enhancer_markers().is_empty() {
        return Err(BootError::Internal(format!(
            "lazy-loaded module `{module_name}` declares an application-wide provider enhancer; register modules with APP_* providers eagerly"
        )));
    }
    Ok(())
}

fn missing_cached_lazy_module(name: &str) -> BootError {
    BootError::Internal(format!(
        "lazy module `{name}` was initialized but not cached"
    ))
}
