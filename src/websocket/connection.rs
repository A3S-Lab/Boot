use super::context::WebSocketContext;
use super::gateway::WebSocketGatewayDefinition;
use super::message::{send_to_outbounds, WebSocketMessage, WebSocketOutbound};
use super::server::WebSocketGatewayServer;
use super::state::normalize_room;
use crate::{BootError, BootRequest, BoxFuture, Result};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Adapter-neutral WebSocket connection.
pub trait WebSocketConnection: Send + Sync {
    fn request(&self) -> &BootRequest;

    fn dispatch(
        &self,
        message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>>;
}

/// In-process WebSocket gateway connection used by adapters and tests.
#[derive(Clone)]
pub struct WebSocketGatewayConnection {
    pub(crate) gateway: WebSocketGatewayDefinition,
    pub(crate) id: u64,
    pub(crate) request: BootRequest,
    pub(crate) outbound: Option<Arc<dyn WebSocketOutbound>>,
    pub(crate) opened: Arc<AtomicBool>,
}

impl WebSocketGatewayConnection {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn request(&self) -> &BootRequest {
        &self.request
    }

    pub fn namespace(&self) -> Option<&str> {
        self.gateway.namespace()
    }

    pub fn server(&self) -> WebSocketGatewayServer {
        self.gateway.server()
    }

    pub fn rooms(&self) -> Result<Vec<String>> {
        self.gateway.state.rooms_for_connection(self.id)
    }

    pub fn join(&self, room: impl Into<String>) -> Result<()> {
        self.gateway.state.join(self.id, room)
    }

    pub fn leave(&self, room: impl Into<String>) -> Result<()> {
        self.gateway.state.leave(self.id, room)
    }

    pub async fn emit(&self, message: WebSocketMessage) -> Result<bool> {
        self.gateway.emit_to_connection(self.id, message).await
    }

    pub async fn broadcast(&self, message: WebSocketMessage) -> Result<usize> {
        let outbounds = self.gateway.state.broadcast_targets(None, Some(self.id))?;
        send_to_outbounds(outbounds, message).await
    }

    pub async fn broadcast_to_room(
        &self,
        room: impl Into<String>,
        message: WebSocketMessage,
    ) -> Result<usize> {
        let room = normalize_room(room)?;
        let outbounds = self
            .gateway
            .state
            .broadcast_targets(Some(&room), Some(self.id))?;
        send_to_outbounds(outbounds, message).await
    }

    pub async fn open(&self) -> Result<()> {
        if self.opened.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.gateway
            .state
            .register(self.id, self.outbound.clone())?;
        let mut hook_result = Ok(());
        for hook in &self.gateway.connection_hooks {
            if let Err(error) = hook.handle_connection(self.clone()).await {
                hook_result = Err(error);
                break;
            }
        }
        if hook_result.is_err() {
            self.opened.store(false, Ordering::Release);
            self.gateway.state.unregister(self.id)?;
        }
        hook_result
    }

    pub async fn close(&self) -> Result<()> {
        if !self.opened.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let mut hook_result = Ok(());
        for hook in self.gateway.disconnect_hooks.iter().rev() {
            if let Err(error) = hook.handle_disconnect(self.clone()).await {
                hook_result = Err(error);
                break;
            }
        }
        let unregister_result = self.gateway.state.unregister(self.id);
        hook_result?;
        unregister_result
    }

    pub async fn dispatch(&self, message: WebSocketMessage) -> Result<Option<WebSocketMessage>> {
        let context = WebSocketContext::new(&self.gateway, self.request.clone(), &message.event);
        match self.dispatch_pipeline(message, context.clone()).await {
            Ok(reply) => Ok(reply),
            Err(error) => self.handle_error(context, error).await,
        }
    }

    async fn dispatch_pipeline(
        &self,
        mut message: WebSocketMessage,
        context: WebSocketContext,
    ) -> Result<Option<WebSocketMessage>> {
        let event = message.event.clone();
        let handler = self.gateway.handlers.get(&event).cloned().ok_or_else(|| {
            BootError::NotFound(format!("websocket event {} {}", self.gateway.path, event))
        })?;

        for guard in &self.gateway.guards {
            let can_activate = guard.inner().can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }
        for guard in &handler.guards {
            let can_activate = guard.inner().can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }

        for interceptor in &self.gateway.interceptors {
            interceptor.inner().before(context.clone()).await?;
        }
        for interceptor in &handler.interceptors {
            interceptor.inner().before(context.clone()).await?;
        }

        for pipe in &self.gateway.pipes {
            message = pipe.inner().transform(message).await?;
        }
        for pipe in &handler.pipes {
            message = pipe.inner().transform(message).await?;
        }

        if handler.validation_enabled {
            for validator in &handler.validators {
                message = validator(message, handler.validation_options)?;
            }
        }

        let mut reply = handler.handler.call(self.clone(), message).await?;
        for interceptor in handler.interceptors.iter().rev() {
            reply = interceptor.inner().after(context.clone(), reply).await?;
        }
        for interceptor in self.gateway.interceptors.iter().rev() {
            reply = interceptor.inner().after(context.clone(), reply).await?;
        }
        Ok(reply)
    }

    async fn handle_error(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> Result<Option<WebSocketMessage>> {
        if let Some(handler) = self.gateway.handlers.get(&context.event) {
            for filter in handler.filters.iter().rev() {
                if let Some(response) = filter
                    .inner()
                    .catch(context.clone(), error.clone_for_filter())
                    .await?
                {
                    return Ok(response.into_message());
                }
            }
        }
        for filter in self.gateway.filters.iter().rev() {
            if let Some(response) = filter
                .inner()
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(response.into_message());
            }
        }
        Err(error)
    }
}

impl WebSocketConnection for WebSocketGatewayConnection {
    fn request(&self) -> &BootRequest {
        self.request()
    }

    fn dispatch(
        &self,
        message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        let connection = self.clone();
        Box::pin(async move { connection.dispatch(message).await })
    }
}
