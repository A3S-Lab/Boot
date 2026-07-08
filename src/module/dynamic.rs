use super::Module;
use crate::{
    BoxFuture, ControllerDefinition, MessagePatternDefinition, Middleware, MiddlewareConsumer,
    ModuleRef, ProviderDefinition, ProviderToken, Result, RouteDefinition,
    WebSocketGatewayDefinition,
};
use std::sync::Arc;

/// Runtime-built module for configuration-driven imports and providers.
#[derive(Clone)]
pub struct DynamicModule {
    name: &'static str,
    imports: Vec<Arc<dyn Module>>,
    forward_imports: Vec<Arc<dyn Module>>,
    providers: Vec<ProviderDefinition>,
    exports: Vec<ProviderToken>,
    middleware: Vec<Arc<dyn Middleware>>,
    controllers: Vec<ControllerDefinition>,
    routes: Vec<RouteDefinition>,
    gateways: Vec<WebSocketGatewayDefinition>,
    message_patterns: Vec<MessagePatternDefinition>,
    global: bool,
    route_prefix: Option<String>,
    middleware_consumer: MiddlewareConsumer,
}

impl DynamicModule {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            imports: Vec::new(),
            forward_imports: Vec::new(),
            providers: Vec::new(),
            exports: Vec::new(),
            middleware: Vec::new(),
            controllers: Vec::new(),
            routes: Vec::new(),
            gateways: Vec::new(),
            message_patterns: Vec::new(),
            global: false,
            route_prefix: None,
            middleware_consumer: MiddlewareConsumer::new(),
        }
    }

    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.imports.push(Arc::new(module));
        self
    }

    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.imports.push(module);
        self
    }

    pub fn forward_import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.forward_imports.push(Arc::new(module));
        self
    }

    pub fn forward_import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.forward_imports.push(module);
        self
    }

    pub fn provider(mut self, provider: ProviderDefinition) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn export<T>(self) -> Self
    where
        T: Send + Sync + 'static,
    {
        self.export_token(ProviderToken::of::<T>())
    }

    pub fn export_named(self, token: impl Into<String>) -> Self {
        self.export_token(ProviderToken::named(token))
    }

    pub fn export_token(mut self, token: ProviderToken) -> Self {
        if !self.exports.contains(&token) {
            self.exports.push(token);
        }
        self
    }

    pub fn middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.middleware.push(Arc::new(middleware));
        self
    }

    pub fn middleware_arc(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    pub fn configure_middleware<F>(mut self, configure: F) -> Result<Self>
    where
        F: FnOnce(&mut MiddlewareConsumer) -> Result<()>,
    {
        configure(&mut self.middleware_consumer)?;
        Ok(self)
    }

    pub fn controller(mut self, controller: ControllerDefinition) -> Self {
        self.controllers.push(controller);
        self
    }

    pub fn route(mut self, route: RouteDefinition) -> Self {
        self.routes.push(route);
        self
    }

    pub fn route_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.route_prefix = Some(prefix.into());
        self
    }

    pub fn gateway(mut self, gateway: WebSocketGatewayDefinition) -> Self {
        self.gateways.push(gateway);
        self
    }

    pub fn message_pattern(mut self, pattern: MessagePatternDefinition) -> Self {
        self.message_patterns.push(pattern);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for DynamicModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        self.imports.clone()
    }

    fn forward_imports(&self) -> Vec<Arc<dyn Module>> {
        self.forward_imports.clone()
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(self.providers.clone())
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(self.exports.clone())
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn route_prefix(&self) -> Option<&str> {
        self.route_prefix.as_deref()
    }

    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        self.middleware.clone()
    }

    fn configure(&self, consumer: &mut MiddlewareConsumer, _module_ref: &ModuleRef) -> Result<()> {
        consumer.extend(self.middleware_consumer.clone());
        Ok(())
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(self.controllers.clone())
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(self.routes.clone())
    }

    fn gateways(&self, _module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        Ok(self.gateways.clone())
    }

    fn message_patterns(&self, _module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        Ok(self.message_patterns.clone())
    }

    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
