use crate::pipeline::PipelineOverrides;
use crate::{
    BootApplication, BootApplicationBuilder, BootError, BootRequest, BootResponse,
    ControllerDefinition, DynamicModule, ExceptionFilter, Guard, Interceptor,
    MessagePatternDefinition, Module, ModuleRef, Pipe, ProviderDefinition, ProviderToken, Result,
    RouteDefinition, WebSocketGatewayDefinition,
};
use std::sync::Arc;

/// Compiled test module with an in-process [`BootApplication`].
pub struct TestingModule {
    app: BootApplication,
}

impl std::fmt::Debug for TestingModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestingModule")
            .field("modules", &self.app.module_names())
            .field("routes", &self.app.routes().len())
            .field("gateways", &self.app.gateways().len())
            .field("message_patterns", &self.app.message_patterns().len())
            .finish()
    }
}

impl TestingModule {
    pub fn builder() -> TestingModuleBuilder {
        TestingModuleBuilder::new()
    }

    pub fn app(&self) -> &BootApplication {
        &self.app
    }

    pub fn into_app(self) -> BootApplication {
        self.app
    }

    pub fn module_ref(&self) -> &ModuleRef {
        self.app.module_ref()
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional::<T>()?
            .ok_or_else(|| BootError::MissingProvider(ProviderToken::of::<T>().to_string()))
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional_named::<T>(token)?
            .ok_or_else(|| BootError::MissingProvider(ProviderToken::named(token).to_string()))
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        if let Some(value) = self.app.get_optional::<T>()? {
            return Ok(Some(value));
        }

        for instance in &self.app.module_instances {
            if let Some(value) = instance.module_ref.get_optional::<T>()? {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        if let Some(value) = self.app.get_optional_named::<T>(token)? {
            return Ok(Some(value));
        }

        for instance in &self.app.module_instances {
            if let Some(value) = instance.module_ref.get_optional_named::<T>(token)? {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    pub async fn call(&self, request: BootRequest) -> Result<BootResponse> {
        self.app.call(request).await
    }
}

/// Builder for Nest-style test modules.
pub struct TestingModuleBuilder {
    app: BootApplicationBuilder,
    module: DynamicModule,
    pipeline_overrides: PipelineOverrides,
}

impl Default for TestingModuleBuilder {
    fn default() -> Self {
        Self {
            app: BootApplication::builder(),
            module: DynamicModule::new("TestingModule"),
            pipeline_overrides: PipelineOverrides::default(),
        }
    }
}

impl TestingModuleBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.module = self.module.import(module);
        self
    }

    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.module = self.module.import_arc(module);
        self
    }

    pub fn provider(mut self, provider: ProviderDefinition) -> Self {
        self.module = self.module.provider(provider);
        self
    }

    pub fn controller(mut self, controller: ControllerDefinition) -> Self {
        self.module = self.module.controller(controller);
        self
    }

    pub fn route(mut self, route: RouteDefinition) -> Self {
        self.module = self.module.route(route);
        self
    }

    pub fn gateway(mut self, gateway: WebSocketGatewayDefinition) -> Self {
        self.module = self.module.gateway(gateway);
        self
    }

    pub fn message_pattern(mut self, pattern: MessagePatternDefinition) -> Self {
        self.module = self.module.message_pattern(pattern);
        self
    }

    pub fn override_provider(mut self, provider: ProviderDefinition) -> Self {
        self.app = self.app.override_provider(provider);
        self
    }

    pub fn override_guard<T, G>(mut self, guard: G) -> Self
    where
        T: Guard,
        G: Guard,
    {
        self.pipeline_overrides.override_guard::<T, G>(guard);
        self
    }

    pub fn override_interceptor<T, I>(mut self, interceptor: I) -> Self
    where
        T: Interceptor,
        I: Interceptor,
    {
        self.pipeline_overrides
            .override_interceptor::<T, I>(interceptor);
        self
    }

    pub fn override_filter<T, F>(mut self, filter: F) -> Self
    where
        T: ExceptionFilter,
        F: ExceptionFilter,
    {
        self.pipeline_overrides.override_filter::<T, F>(filter);
        self
    }

    pub fn override_pipe<T, P>(mut self, pipe: P) -> Self
    where
        T: Pipe,
        P: Pipe,
    {
        self.pipeline_overrides.override_pipe::<T, P>(pipe);
        self
    }

    pub fn compile(self) -> Result<TestingModule> {
        let mut app = self.app.import(self.module).build()?;
        if !self.pipeline_overrides.is_empty() {
            app.routes = app
                .routes
                .into_iter()
                .map(|route| route.with_pipeline_overrides(&self.pipeline_overrides))
                .collect();
        }
        Ok(TestingModule { app })
    }
}
