use super::application::ModuleInstance;
use crate::pipeline::PipelineComponents;
use crate::{
    BootError, BoxFuture, ControllerDefinition, MessagePatternDefinition, Module, ModuleRef,
    ProviderDefinition, ProviderToken, Result, RouteDefinition, WebSocketGatewayDefinition,
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
        let name = module.name();
        if name.trim().is_empty() {
            return Err(BootError::EmptyModuleName);
        }

        if let Some(existing) = self.registered.get(name) {
            return Ok(existing.clone());
        }

        self.enter_module(name)?;
        let result = self.register_module_inner(module, name, global_pipeline, sink);
        self.exit_module();
        result
    }

    pub fn register_module_async<'a>(
        &'a mut self,
        module: Arc<dyn Module>,
        global_pipeline: &'a PipelineComponents,
        sink: &'a mut ModuleRegistrationSink<'_>,
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
                .register_module_async_inner(module, name, global_pipeline, sink)
                .await;
            self.exit_module();
            result
        })
    }

    fn register_module_inner(
        &mut self,
        module: Arc<dyn Module>,
        name: &'static str,
        global_pipeline: &PipelineComponents,
        sink: &mut ModuleRegistrationSink<'_>,
    ) -> Result<RegisteredModule> {
        let mut imported_modules = Vec::new();
        for imported in module.imports() {
            imported_modules.push(self.register_module(imported, global_pipeline, sink)?);
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

        for controller in module.controllers(&module_ref)? {
            register_controller(
                name,
                controller,
                &module_ref,
                global_pipeline,
                &module_pipeline,
                sink.routes,
            );
        }

        sink.routes
            .extend(module.routes()?.into_iter().map(|route| {
                route
                    .with_pipeline_prefix(&module_pipeline)
                    .with_pipeline_prefix(global_pipeline)
                    .with_module_name(name)
                    .with_module_ref(module_ref.clone())
            }));
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
    ) -> BoxFuture<'a, Result<RegisteredModule>> {
        Box::pin(async move {
            let mut imported_modules = Vec::new();
            for imported in module.imports() {
                imported_modules.push(
                    self.register_module_async(imported, global_pipeline, sink)
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

            for controller in module.controllers(&module_ref)? {
                register_controller(
                    name,
                    controller,
                    &module_ref,
                    global_pipeline,
                    &module_pipeline,
                    sink.routes,
                );
            }

            sink.routes
                .extend(module.routes()?.into_iter().map(|route| {
                    route
                        .with_pipeline_prefix(&module_pipeline)
                        .with_pipeline_prefix(global_pipeline)
                        .with_module_name(name)
                        .with_module_ref(module_ref.clone())
                }));
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
    exports: ModuleRef,
}

fn register_controller(
    module_name: &str,
    controller: ControllerDefinition,
    module_ref: &ModuleRef,
    global_pipeline: &PipelineComponents,
    module_pipeline: &PipelineComponents,
    routes: &mut Vec<RouteDefinition>,
) {
    routes.extend(controller.into_routes().into_iter().map(|route| {
        route
            .with_pipeline_prefix(module_pipeline)
            .with_pipeline_prefix(global_pipeline)
            .with_module_name(module_name)
            .with_module_ref(module_ref.clone())
    }));
}
