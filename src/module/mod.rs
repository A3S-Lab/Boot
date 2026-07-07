use crate::{
    BoxFuture, ControllerDefinition, MessagePatternDefinition, Middleware, ModuleRef,
    ProviderDefinition, ProviderToken, Result, RouteDefinition, WebSocketGatewayDefinition,
};
use std::sync::Arc;

mod dynamic;

pub use dynamic::DynamicModule;

/// A module contributes imports, providers, controllers, and routes.
///
/// This is the Rust equivalent of a Nest module boundary. Modules organize the
/// application graph; HTTP serving remains delegated to an adapter.
pub trait Module: Send + Sync + 'static {
    /// Stable module name used for deduplication and diagnostics.
    fn name(&self) -> &'static str;

    /// Imported modules that should be registered before this module.
    fn imports(&self) -> Vec<Arc<dyn Module>> {
        Vec::new()
    }

    /// Providers exported into the application container.
    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(Vec::new())
    }

    /// Providers this module exposes to importing modules.
    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(Vec::new())
    }

    /// Whether exported providers should be visible to every module scope.
    fn is_global(&self) -> bool {
        false
    }

    /// Middleware applied to controllers and direct routes declared by this module.
    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        Vec::new()
    }

    /// Controller route groups built with access to the provider container.
    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(Vec::new())
    }

    /// Framework-neutral routes contributed directly by this module.
    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(Vec::new())
    }

    /// WebSocket gateways contributed by this module.
    fn gateways(&self, _module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        Ok(Vec::new())
    }

    /// Microservice message patterns contributed by this module.
    fn message_patterns(&self, _module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        Ok(Vec::new())
    }

    /// Lifecycle hook called after imports and providers are registered.
    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        Ok(())
    }

    /// Async lifecycle hook called by hosts that want startup work before serve.
    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    /// Async lifecycle hook called by hosts that need graceful shutdown cleanup.
    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}
