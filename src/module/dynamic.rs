use super::Module;
use crate::{
    BoxFuture, ControllerDefinition, MessagePatternDefinition, Middleware, ModuleRef,
    ProviderDefinition, ProviderToken, Result, RouteDefinition, WebSocketGatewayDefinition,
};
use std::sync::Arc;

/// Runtime-built module for configuration-driven imports and providers.
#[derive(Clone)]
pub struct DynamicModule {
    name: &'static str,
    imports: Vec<Arc<dyn Module>>,
    providers: Vec<ProviderDefinition>,
    exports: Vec<ProviderToken>,
    middleware: Vec<Arc<dyn Middleware>>,
    controllers: Vec<ControllerDefinition>,
    routes: Vec<RouteDefinition>,
    gateways: Vec<WebSocketGatewayDefinition>,
    message_patterns: Vec<MessagePatternDefinition>,
    global: bool,
}

impl DynamicModule {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            imports: Vec::new(),
            providers: Vec::new(),
            exports: Vec::new(),
            middleware: Vec::new(),
            controllers: Vec::new(),
            routes: Vec::new(),
            gateways: Vec::new(),
            message_patterns: Vec::new(),
            global: false,
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

    pub fn controller(mut self, controller: ControllerDefinition) -> Self {
        self.controllers.push(controller);
        self
    }

    pub fn route(mut self, route: RouteDefinition) -> Self {
        self.routes.push(route);
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

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(self.providers.clone())
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(self.exports.clone())
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        self.middleware.clone()
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
