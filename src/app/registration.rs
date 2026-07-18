use super::application::ModuleInstance;
use crate::pipeline::{PipelineComponents, ProviderEnhancerComponents};
use crate::routing::path::join_paths;
use crate::{
    BootError, BoxFuture, ControllerDefinition, MessagePatternDefinition, MiddlewareConsumer,
    Module, ModuleRef, ProviderDefinition, ProviderToken, Result, RouteDefinition,
    WebSocketGatewayDefinition,
};
use std::collections::BTreeMap;
use std::sync::Arc;

pub(super) struct ModuleRegistry {
    registered: BTreeMap<String, RegisteredModule>,
    active: BTreeMap<String, RegisteredModule>,
    pending: Vec<PendingModuleRegistration>,
    visiting: Vec<String>,
    global_ref: ModuleRef,
    provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
    module_overrides: BTreeMap<String, Arc<dyn Module>>,
    provider_enhancers: ProviderEnhancerComponents,
}

pub(super) struct ModuleRegistrationSink<'a> {
    pub modules: &'a mut Vec<String>,
    pub module_instances: &'a mut Vec<ModuleInstance>,
    pub routes: &'a mut Vec<RouteDefinition>,
    pub gateways: &'a mut Vec<WebSocketGatewayDefinition>,
    pub message_patterns: &'a mut Vec<MessagePatternDefinition>,
}

struct ActiveModuleRegistration {
    route_prefix: String,
    module_ref: ModuleRef,
    exports: ModuleRef,
}

struct PendingModuleRegistration {
    module: Arc<dyn Module>,
    name: String,
    route_prefix: String,
    module_ref: ModuleRef,
    import_names: Vec<String>,
    exported_tokens: Vec<ProviderToken>,
    is_global: bool,
}

impl ModuleRegistry {
    pub fn new(
        global_ref: ModuleRef,
        provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
        module_overrides: BTreeMap<String, Arc<dyn Module>>,
    ) -> Self {
        Self {
            registered: BTreeMap::new(),
            active: BTreeMap::new(),
            pending: Vec::new(),
            visiting: Vec::new(),
            global_ref,
            provider_overrides,
            module_overrides,
            provider_enhancers: ProviderEnhancerComponents::default(),
        }
    }

    pub fn register_module(&mut self, module: Arc<dyn Module>) -> Result<RegisteredModule> {
        self.register_module_with_prefix(module, "")
    }

    fn register_module_with_prefix(
        &mut self,
        module: Arc<dyn Module>,
        parent_route_prefix: &str,
    ) -> Result<RegisteredModule> {
        let module = self.module_override_or(module);
        let name = module.name();
        if name.trim().is_empty() {
            return Err(BootError::EmptyModuleName);
        }

        if let Some(existing) = self.registered.get(name) {
            return Ok(existing.clone());
        }

        let route_prefix = module_route_prefix(parent_route_prefix, module.route_prefix())?;
        self.enter_module(name)?;
        let module_ref = ModuleRef::new();
        let exports = ModuleRef::new();
        let state = ActiveModuleRegistration {
            route_prefix,
            module_ref: module_ref.clone(),
            exports: exports.clone(),
        };
        self.active.insert(
            name.to_string(),
            RegisteredModule {
                name: name.to_string(),
                module_ref: module_ref.clone(),
                exports: exports.clone(),
            },
        );
        let result = self.register_module_inner(module, name, state);
        self.active.remove(name);
        self.exit_module();
        result
    }

    fn register_forward_module_with_prefix(
        &mut self,
        module: Arc<dyn Module>,
        parent_route_prefix: &str,
    ) -> Result<RegisteredModule> {
        let module = self.module_override_or(module);
        let name = module.name();
        if name.trim().is_empty() {
            return Err(BootError::EmptyModuleName);
        }

        if let Some(existing) = self.registered.get(name) {
            return Ok(existing.clone());
        }
        if let Some(active) = self.active.get(name) {
            return Ok(active.clone());
        }

        self.register_module_with_prefix(module, parent_route_prefix)
    }

    pub fn register_module_async<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        self.register_module_async_with_prefix(module, "")
    }

    fn register_module_async_with_prefix<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        parent_route_prefix: &'a str,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let module = self.module_override_or(module);
            let name = module.name();
            if name.trim().is_empty() {
                return Err(BootError::EmptyModuleName);
            }

            if let Some(existing) = self.registered.get(name) {
                return Ok(existing.clone());
            }

            let route_prefix = module_route_prefix(parent_route_prefix, module.route_prefix())?;
            self.enter_module(name)?;
            let module_ref = ModuleRef::new();
            let exports = ModuleRef::new();
            let state = ActiveModuleRegistration {
                route_prefix,
                module_ref: module_ref.clone(),
                exports: exports.clone(),
            };
            self.active.insert(
                name.to_string(),
                RegisteredModule {
                    name: name.to_string(),
                    module_ref: module_ref.clone(),
                    exports: exports.clone(),
                },
            );
            let result = self.register_module_async_inner(module, name, state).await;
            self.active.remove(name);
            self.exit_module();
            result
        })
    }

    fn register_forward_module_async_with_prefix<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        parent_route_prefix: &'a str,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let module = self.module_override_or(module);
            let name = module.name();
            if name.trim().is_empty() {
                return Err(BootError::EmptyModuleName);
            }

            if let Some(existing) = self.registered.get(name) {
                return Ok(existing.clone());
            }
            if let Some(active) = self.active.get(name) {
                return Ok(active.clone());
            }

            self.register_module_async_with_prefix(module, parent_route_prefix)
                .await
        })
    }

    pub fn registered_modules(&self) -> Vec<(String, RegisteredModule)> {
        self.registered
            .iter()
            .map(|(name, module)| (name.clone(), module.clone()))
            .collect()
    }

    pub fn provider_enhancers(&self) -> ProviderEnhancerComponents {
        self.provider_enhancers.clone()
    }

    fn register_module_inner(
        &mut self,
        module: Arc<dyn Module>,
        name: &'static str,
        state: ActiveModuleRegistration,
    ) -> Result<RegisteredModule> {
        let ActiveModuleRegistration {
            route_prefix,
            module_ref,
            exports,
        } = state;
        let mut imported_modules = Vec::new();
        for imported in module.imports() {
            imported_modules.push(self.register_module_with_prefix(imported, &route_prefix)?);
        }
        for imported in module.forward_imports() {
            imported_modules
                .push(self.register_forward_module_with_prefix(imported, &route_prefix)?);
        }

        module_ref.add_visible_scope(self.global_ref.clone())?;
        for imported in &imported_modules {
            module_ref.add_visible_scope(imported.exports.clone())?;
        }

        for provider in module.providers()? {
            let provider = self.provider_override_or(provider);
            let enhancers = provider.enhancer_markers().to_vec();
            module_ref.register(provider)?;
            for enhancer in enhancers {
                self.provider_enhancers
                    .push(enhancer.bind(module_ref.clone()));
            }
        }

        let export_tokens = module.exports()?;
        for token in &export_tokens {
            exports.export_from(&module_ref, token)?;
        }
        let exported_tokens = exports.local_tokens()?;
        let is_global = module.is_global();

        if is_global {
            for token in &exported_tokens {
                self.global_ref.export_from(&exports, token)?;
            }
        }

        let import_names = imported_modules
            .iter()
            .map(|module| module.name.clone())
            .collect::<Vec<_>>();
        let registered = RegisteredModule {
            name: name.to_string(),
            module_ref: module_ref.clone(),
            exports,
        };
        self.pending.push(PendingModuleRegistration {
            module,
            module_ref,
            name: name.to_string(),
            route_prefix,
            import_names,
            exported_tokens,
            is_global,
        });
        self.registered.insert(name.to_string(), registered.clone());
        Ok(registered)
    }

    fn register_module_async_inner<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        name: &'static str,
        state: ActiveModuleRegistration,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let ActiveModuleRegistration {
                route_prefix,
                module_ref,
                exports,
            } = state;
            let mut imported_modules = Vec::new();
            for imported in module.imports() {
                imported_modules.push(
                    self.register_module_async_with_prefix(imported, &route_prefix)
                        .await?,
                );
            }
            for imported in module.forward_imports() {
                imported_modules.push(
                    self.register_forward_module_async_with_prefix(imported, &route_prefix)
                        .await?,
                );
            }

            module_ref.add_visible_scope(self.global_ref.clone())?;
            for imported in &imported_modules {
                module_ref.add_visible_scope(imported.exports.clone())?;
            }

            for provider in module.providers()? {
                let provider = self.provider_override_or(provider);
                let enhancers = provider.enhancer_markers().to_vec();
                module_ref.register_async(provider).await?;
                for enhancer in enhancers {
                    self.provider_enhancers
                        .push(enhancer.bind(module_ref.clone()));
                }
            }

            let export_tokens = module.exports()?;
            for token in &export_tokens {
                exports.export_from(&module_ref, token)?;
            }
            let exported_tokens = exports.local_tokens()?;
            let is_global = module.is_global();

            if is_global {
                for token in &exported_tokens {
                    self.global_ref.export_from(&exports, token)?;
                }
            }

            let import_names = imported_modules
                .iter()
                .map(|module| module.name.clone())
                .collect::<Vec<_>>();
            let registered = RegisteredModule {
                name: name.to_string(),
                module_ref: module_ref.clone(),
                exports,
            };
            self.pending.push(PendingModuleRegistration {
                module,
                module_ref,
                name: name.to_string(),
                route_prefix,
                import_names,
                exported_tokens,
                is_global,
            });
            self.registered.insert(name.to_string(), registered.clone());
            Ok(registered)
        })
    }

    pub fn finalize(
        &mut self,
        global_pipeline: &PipelineComponents,
        sink: &mut ModuleRegistrationSink<'_>,
    ) -> Result<()> {
        for pending in &self.pending {
            pending.module_ref.validate_local_resolution_plans()?;
        }
        for pending in &self.pending {
            pending.module_ref.initialize_local_singletons()?;
        }

        let pending = std::mem::take(&mut self.pending);
        for pending in pending {
            finalize_module(pending, global_pipeline, sink)?;
        }
        Ok(())
    }

    pub fn finalize_async<'a>(
        &'a mut self,
        global_pipeline: &'a PipelineComponents,
        sink: &'a mut ModuleRegistrationSink<'_>,
    ) -> BoxFuture<'a, Result<()>> {
        Box::pin(async move {
            for pending in &self.pending {
                pending.module_ref.validate_local_resolution_plans()?;
            }
            for pending in &self.pending {
                pending.module_ref.seed_local_async_singletons().await?;
            }
            for pending in &self.pending {
                pending.module_ref.initialize_local_singletons()?;
            }

            let pending = std::mem::take(&mut self.pending);
            for pending in pending {
                finalize_module(pending, global_pipeline, sink)?;
            }
            Ok(())
        })
    }

    fn enter_module(&mut self, name: &str) -> Result<()> {
        if let Some(index) = self.visiting.iter().position(|active| active == name) {
            let mut chain = self.visiting[index..].to_vec();
            chain.push(name.to_string());
            return Err(BootError::Internal(format!(
                "cyclic module import detected: {}",
                chain.join(" -> ")
            )));
        }

        self.visiting.push(name.to_string());
        Ok(())
    }

    fn exit_module(&mut self) {
        self.visiting.pop();
    }

    fn provider_override_or(&self, provider: ProviderDefinition) -> ProviderDefinition {
        match self.provider_overrides.get(provider.token()) {
            Some(replacement) => replacement.clone().with_enhancers_from(&provider),
            None => provider,
        }
    }

    fn module_override_or(&self, module: Arc<dyn Module>) -> Arc<dyn Module> {
        self.module_overrides
            .get(module.name())
            .cloned()
            .unwrap_or(module)
    }
}

fn finalize_module(
    pending: PendingModuleRegistration,
    global_pipeline: &PipelineComponents,
    sink: &mut ModuleRegistrationSink<'_>,
) -> Result<()> {
    let PendingModuleRegistration {
        module,
        name,
        route_prefix,
        module_ref,
        import_names,
        exported_tokens,
        is_global,
    } = pending;

    module_ref.initialize_local_providers()?;
    module.on_module_init(&module_ref)?;

    let mut module_pipeline = PipelineComponents::default();
    for middleware in module.middleware() {
        module_pipeline.push_middleware_arc(middleware);
    }
    let mut middleware_consumer = MiddlewareConsumer::new();
    module.configure(&mut middleware_consumer, &module_ref)?;

    let context = RouteRegistrationContext {
        module_name: &name,
        module_ref: &module_ref,
        global_pipeline,
        module_pipeline: &module_pipeline,
        middleware_consumer: &middleware_consumer,
        route_prefix: &route_prefix,
    };
    for controller in module.controllers(&module_ref)? {
        register_controller(&context, controller, sink.routes)?;
    }
    for route in module.routes()? {
        sink.routes.push(context.prepare_route(route)?);
    }
    sink.gateways
        .extend(module.gateways(&module_ref)?.into_iter().map(|gateway| {
            gateway
                .with_module_name(&name)
                .with_module_ref(module_ref.clone())
        }));
    sink.message_patterns
        .extend(
            module
                .message_patterns(&module_ref)?
                .into_iter()
                .map(|pattern| {
                    pattern
                        .with_module_name(&name)
                        .with_module_ref(module_ref.clone())
                }),
        );

    sink.modules.push(name);
    sink.module_instances.push(ModuleInstance {
        module,
        module_ref,
        imports: import_names,
        exports: exported_tokens,
        is_global,
        route_prefix: (!route_prefix.is_empty()).then_some(route_prefix),
    });
    Ok(())
}

#[derive(Clone)]
pub(super) struct RegisteredModule {
    pub name: String,
    pub module_ref: ModuleRef,
    pub exports: ModuleRef,
}

struct RouteRegistrationContext<'a> {
    module_name: &'a str,
    module_ref: &'a ModuleRef,
    global_pipeline: &'a PipelineComponents,
    module_pipeline: &'a PipelineComponents,
    middleware_consumer: &'a MiddlewareConsumer,
    route_prefix: &'a str,
}

impl RouteRegistrationContext<'_> {
    fn prepare_route(&self, route: RouteDefinition) -> Result<RouteDefinition> {
        let local_path = route.path().to_string();
        let route = with_route_prefix(route, self.route_prefix)?;
        let path_candidates = route_path_candidates(local_path, route.path());
        let route = self
            .middleware_consumer
            .apply_to_route(route, &path_candidates);

        Ok(route
            .with_pipeline_prefix(self.module_pipeline)
            .with_pipeline_prefix(self.global_pipeline)
            .with_module_name(self.module_name)
            .with_module_ref(self.module_ref.clone()))
    }
}

fn register_controller(
    context: &RouteRegistrationContext<'_>,
    controller: ControllerDefinition,
    routes: &mut Vec<RouteDefinition>,
) -> Result<()> {
    for route in controller.into_routes() {
        routes.push(context.prepare_route(route)?);
    }
    Ok(())
}

fn module_route_prefix(parent_prefix: &str, module_prefix: Option<&str>) -> Result<String> {
    let module_prefix = module_prefix.unwrap_or("");
    if parent_prefix.is_empty() && (module_prefix.is_empty() || module_prefix == "/") {
        return Ok(String::new());
    }

    if module_prefix.is_empty() || module_prefix == "/" {
        return Ok(parent_prefix.to_string());
    }

    join_paths(parent_prefix, module_prefix)
}

fn with_route_prefix(route: RouteDefinition, route_prefix: &str) -> Result<RouteDefinition> {
    if route_prefix.is_empty() {
        Ok(route)
    } else {
        route.with_path_prefix(route_prefix)
    }
}

fn route_path_candidates(local_path: String, prefixed_path: &str) -> Vec<String> {
    if local_path == prefixed_path {
        vec![local_path]
    } else {
        vec![local_path, prefixed_path.to_string()]
    }
}
