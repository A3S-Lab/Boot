use super::application::ModuleInstance;
use crate::pipeline::PipelineComponents;
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
    visiting: Vec<String>,
    global_ref: ModuleRef,
    provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
}

pub(super) struct ModuleRegistrationSink<'a> {
    pub modules: &'a mut Vec<String>,
    pub module_instances: &'a mut Vec<ModuleInstance>,
    pub routes: &'a mut Vec<RouteDefinition>,
    pub gateways: &'a mut Vec<WebSocketGatewayDefinition>,
    pub message_patterns: &'a mut Vec<MessagePatternDefinition>,
}

impl ModuleRegistry {
    pub fn new(
        global_ref: ModuleRef,
        provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
    ) -> Self {
        Self {
            registered: BTreeMap::new(),
            visiting: Vec::new(),
            global_ref,
            provider_overrides,
        }
    }

    pub fn register_module(
        &mut self,
        module: Arc<dyn Module>,
        global_pipeline: &PipelineComponents,
        sink: &mut ModuleRegistrationSink<'_>,
    ) -> Result<RegisteredModule> {
        self.register_module_with_prefix(module, global_pipeline, sink, "")
    }

    fn register_module_with_prefix(
        &mut self,
        module: Arc<dyn Module>,
        global_pipeline: &PipelineComponents,
        sink: &mut ModuleRegistrationSink<'_>,
        parent_route_prefix: &str,
    ) -> Result<RegisteredModule> {
        let name = module.name();
        if name.trim().is_empty() {
            return Err(BootError::EmptyModuleName);
        }

        if let Some(existing) = self.registered.get(name) {
            return Ok(existing.clone());
        }

        self.enter_module(name)?;
        let result =
            self.register_module_inner(module, name, global_pipeline, sink, parent_route_prefix);
        self.exit_module();
        result
    }

    pub fn register_module_async<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        global_pipeline: &'a PipelineComponents,
        sink: &'a mut ModuleRegistrationSink<'_>,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        self.register_module_async_with_prefix(module, global_pipeline, sink, "")
    }

    fn register_module_async_with_prefix<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        global_pipeline: &'a PipelineComponents,
        sink: &'a mut ModuleRegistrationSink<'_>,
        parent_route_prefix: &'a str,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let name = module.name();
            if name.trim().is_empty() {
                return Err(BootError::EmptyModuleName);
            }

            if let Some(existing) = self.registered.get(name) {
                return Ok(existing.clone());
            }

            self.enter_module(name)?;
            let result = self
                .register_module_async_inner(
                    module,
                    name,
                    global_pipeline,
                    sink,
                    parent_route_prefix,
                )
                .await;
            self.exit_module();
            result
        })
    }

    pub fn registered_modules(&self) -> Vec<(String, RegisteredModule)> {
        self.registered
            .iter()
            .map(|(name, module)| (name.clone(), module.clone()))
            .collect()
    }

    fn register_module_inner(
        &mut self,
        module: Arc<dyn Module>,
        name: &'static str,
        global_pipeline: &PipelineComponents,
        sink: &mut ModuleRegistrationSink<'_>,
        parent_route_prefix: &str,
    ) -> Result<RegisteredModule> {
        let route_prefix = module_route_prefix(parent_route_prefix, module.route_prefix())?;
        let mut imported_modules = Vec::new();
        for imported in module.imports() {
            imported_modules.push(self.register_module_with_prefix(
                imported,
                global_pipeline,
                sink,
                &route_prefix,
            )?);
        }

        let module_ref = ModuleRef::new();
        module_ref.add_visible_scope(self.global_ref.clone())?;
        for imported in &imported_modules {
            module_ref.add_visible_scope(imported.exports.clone())?;
        }

        for provider in module.providers()? {
            let provider = self.provider_override_or(provider);
            module_ref.register(provider)?;
        }
        module_ref.initialize_local_singletons()?;

        let exports = ModuleRef::new();
        for token in module.exports()? {
            exports.export_from(&module_ref, &token)?;
        }

        if module.is_global() {
            for token in exports.local_tokens()? {
                self.global_ref.export_from(&exports, &token)?;
            }
        }

        module_ref.initialize_local_providers()?;
        module.on_module_init(&module_ref)?;

        let mut module_pipeline = PipelineComponents::default();
        for middleware in module.middleware() {
            module_pipeline.push_middleware_arc(middleware);
        }
        let mut middleware_consumer = MiddlewareConsumer::new();
        module.configure(&mut middleware_consumer, &module_ref)?;

        for controller in module.controllers(&module_ref)? {
            let context = RouteRegistrationContext {
                module_name: name,
                module_ref: &module_ref,
                global_pipeline,
                module_pipeline: &module_pipeline,
                middleware_consumer: &middleware_consumer,
                route_prefix: &route_prefix,
            };
            register_controller(&context, controller, sink.routes)?;
        }

        for route in module.routes()? {
            let context = RouteRegistrationContext {
                module_name: name,
                module_ref: &module_ref,
                global_pipeline,
                module_pipeline: &module_pipeline,
                middleware_consumer: &middleware_consumer,
                route_prefix: &route_prefix,
            };
            sink.routes.push(context.prepare_route(route)?);
        }
        sink.gateways.extend(
            module
                .gateways(&module_ref)?
                .into_iter()
                .map(|gateway| gateway.with_module_name(name)),
        );
        sink.message_patterns.extend(
            module
                .message_patterns(&module_ref)?
                .into_iter()
                .map(|pattern| pattern.with_module_name(name)),
        );

        let registered = RegisteredModule {
            module_ref: module_ref.clone(),
            exports,
        };
        sink.modules.push(name.to_string());
        sink.module_instances
            .push(ModuleInstance { module, module_ref });
        self.registered.insert(name.to_string(), registered.clone());
        Ok(registered)
    }

    fn register_module_async_inner<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        name: &'static str,
        global_pipeline: &'a PipelineComponents,
        sink: &'a mut ModuleRegistrationSink<'_>,
        parent_route_prefix: &'a str,
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let route_prefix = module_route_prefix(parent_route_prefix, module.route_prefix())?;
            let mut imported_modules = Vec::new();
            for imported in module.imports() {
                imported_modules.push(
                    self.register_module_async_with_prefix(
                        imported,
                        global_pipeline,
                        sink,
                        &route_prefix,
                    )
                    .await?,
                );
            }

            let module_ref = ModuleRef::new();
            module_ref.add_visible_scope(self.global_ref.clone())?;
            for imported in &imported_modules {
                module_ref.add_visible_scope(imported.exports.clone())?;
            }

            for provider in module.providers()? {
                let provider = self.provider_override_or(provider);
                module_ref.register_async(provider).await?;
            }
            module_ref.initialize_local_singletons_async().await?;

            let exports = ModuleRef::new();
            for token in module.exports()? {
                exports.export_from(&module_ref, &token)?;
            }

            if module.is_global() {
                for token in exports.local_tokens()? {
                    self.global_ref.export_from(&exports, &token)?;
                }
            }

            module_ref.initialize_local_providers()?;
            module.on_module_init(&module_ref)?;

            let mut module_pipeline = PipelineComponents::default();
            for middleware in module.middleware() {
                module_pipeline.push_middleware_arc(middleware);
            }
            let mut middleware_consumer = MiddlewareConsumer::new();
            module.configure(&mut middleware_consumer, &module_ref)?;

            for controller in module.controllers(&module_ref)? {
                let context = RouteRegistrationContext {
                    module_name: name,
                    module_ref: &module_ref,
                    global_pipeline,
                    module_pipeline: &module_pipeline,
                    middleware_consumer: &middleware_consumer,
                    route_prefix: &route_prefix,
                };
                register_controller(&context, controller, sink.routes)?;
            }

            for route in module.routes()? {
                let context = RouteRegistrationContext {
                    module_name: name,
                    module_ref: &module_ref,
                    global_pipeline,
                    module_pipeline: &module_pipeline,
                    middleware_consumer: &middleware_consumer,
                    route_prefix: &route_prefix,
                };
                sink.routes.push(context.prepare_route(route)?);
            }
            sink.gateways.extend(
                module
                    .gateways(&module_ref)?
                    .into_iter()
                    .map(|gateway| gateway.with_module_name(name)),
            );
            sink.message_patterns.extend(
                module
                    .message_patterns(&module_ref)?
                    .into_iter()
                    .map(|pattern| pattern.with_module_name(name)),
            );

            let registered = RegisteredModule {
                module_ref: module_ref.clone(),
                exports,
            };
            sink.modules.push(name.to_string());
            sink.module_instances
                .push(ModuleInstance { module, module_ref });
            self.registered.insert(name.to_string(), registered.clone());
            Ok(registered)
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
        self.provider_overrides
            .get(provider.token())
            .cloned()
            .unwrap_or(provider)
    }
}

#[derive(Clone)]
pub(super) struct RegisteredModule {
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
