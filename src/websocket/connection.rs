use super::context::WebSocketContext;
use super::gateway::WebSocketGatewayDefinition;
use super::message::{send_to_outbounds, WebSocketMessage, WebSocketOutbound};
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

    pub async fn dispatch(
        &self,
        mut message: WebSocketMessage,
    ) -> Result<Option<WebSocketMessage>> {
        let event = message.event.clone();
        let handler = self.gateway.handlers.get(&event).cloned().ok_or_else(|| {
            BootError::NotFound(format!("websocket event {} {}", self.gateway.path, event))
        })?;

        let context = WebSocketContext::new(&self.gateway, self.request.clone(), &message.event);
        for guard in &self.gateway.guards {
            let can_activate = guard.can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }

        for interceptor in &self.gateway.interceptors {
            interceptor.before(context.clone()).await?;
        }

        for pipe in &self.gateway.pipes {
            message = pipe.transform(message).await?;
        }

        let mut reply = handler.call(message).await?;
        for interceptor in self.gateway.interceptors.iter().rev() {
            reply = interceptor.after(context.clone(), reply).await?;
        }
        Ok(reply)
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
