use super::connection::WebSocketGatewayConnection;
use super::context::WebSocketGatewayInitContext;
use crate::{BoxFuture, Result};
use std::future::Future;

/// Hook invoked when a WebSocket gateway is initialized during application bootstrap.
pub trait WebSocketGatewayInitHook: Send + Sync + 'static {
    fn after_init(&self, context: WebSocketGatewayInitContext) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayInitHook for F
where
    F: Fn(WebSocketGatewayInitContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn after_init(&self, context: WebSocketGatewayInitContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(context))
    }
}

/// Hook invoked when a WebSocket client connects to a gateway.
pub trait WebSocketGatewayConnectionHook: Send + Sync + 'static {
    fn handle_connection(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayConnectionHook for F
where
    F: Fn(WebSocketGatewayConnection) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn handle_connection(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(connection))
    }
}

/// Hook invoked when a WebSocket client disconnects from a gateway.
pub trait WebSocketGatewayDisconnectHook: Send + Sync + 'static {
    fn handle_disconnect(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayDisconnectHook for F
where
    F: Fn(WebSocketGatewayConnection) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn handle_disconnect(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(connection))
    }
}
