use super::context::WebSocketContext;
use super::gateway::WebSocketGatewayDefinition;
use super::message::{send_to_outbounds, WebSocketMessage, WebSocketOutbound};
use super::server::WebSocketGatewayServer;
use super::state::normalize_room;
use crate::{
    BootError, BootRequest, BoxFuture, CallHandler, ContextId, ContextIdFactory, Result,
    WebSocketInterceptor,
};
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
        let context_id = ContextIdFactory::create();
        let mut connection = self.clone();
        if let Some(module_ref) = &connection.gateway.module_ref {
            connection.request = connection
                .request
                .with_module_ref(module_ref.context_scope(&context_id));
        }
        let context = WebSocketContext::new(
            &connection.gateway,
            connection.request.clone(),
            &message.event,
        );
        match connection
            .dispatch_pipeline(message, context.clone(), context_id.clone())
            .await
        {
            Ok(reply) => Ok(reply),
            Err(error) => connection.handle_error(context, error, &context_id).await,
        }
    }

    async fn dispatch_pipeline(
        &self,
        message: WebSocketMessage,
        context: WebSocketContext,
        context_id: ContextId,
    ) -> Result<Option<WebSocketMessage>> {
        let event = message.event.clone();
        let handler = self.gateway.handlers.get(&event).cloned().ok_or_else(|| {
            BootError::NotFound(format!("websocket event {} {}", self.gateway.path, event))
        })?;

        for guard in &self.gateway.guards {
            let can_activate = guard
                .resolve(&context_id)?
                .can_activate(context.clone())
                .await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }
        for guard in &handler.guards {
            let can_activate = guard
                .resolve(&context_id)?
                .can_activate(context.clone())
                .await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }

        let interceptors = self
            .gateway
            .interceptors
            .iter()
            .chain(handler.interceptors.iter())
            .map(|interceptor| interceptor.resolve(&context_id))
            .collect::<Result<Vec<_>>>()?;

        let connection = self.clone();
        let replay_handler = handler.clone();
        let replay_message = message.clone();
        let replay_context_id = context_id.clone();
        let terminal = CallHandler::from_fn(move || {
            let connection = connection.clone();
            let handler = replay_handler.clone();
            let mut message = replay_message.clone();
            let context_id = replay_context_id.clone();
            async move {
                for pipe in &connection.gateway.pipes {
                    message = pipe.resolve(&context_id)?.transform(message).await?;
                }
                for pipe in &handler.pipes {
                    message = pipe.resolve(&context_id)?.transform(message).await?;
                }

                if handler.validation_enabled {
                    for validator in &handler.validators {
                        message = validator(message, handler.validation_options)?;
                    }
                }

                handler.handler.call(connection, message).await
            }
        });

        run_interceptor_chain(&interceptors, context, terminal).await
    }

    async fn handle_error(
        &self,
        context: WebSocketContext,
        error: BootError,
        context_id: &ContextId,
    ) -> Result<Option<WebSocketMessage>> {
        if let Some(handler) = self.gateway.handlers.get(&context.event) {
            for filter in handler.filters.iter().rev() {
                let filter = filter.resolve(context_id)?;
                if let Some(response) = filter
                    .catch(context.clone(), error.clone_for_filter())
                    .await?
                {
                    return Ok(response.into_message());
                }
            }
        }
        for filter in self.gateway.filters.iter().rev() {
            let filter = filter.resolve(context_id)?;
            if let Some(response) = filter
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(response.into_message());
            }
        }
        Err(error)
    }
}

fn run_interceptor_chain<'a>(
    interceptors: &'a [Arc<dyn WebSocketInterceptor>],
    context: WebSocketContext,
    terminal: CallHandler<'a, Option<WebSocketMessage>>,
) -> BoxFuture<'a, Result<Option<WebSocketMessage>>> {
    let Some((interceptor, remaining)) = interceptors.split_first() else {
        return terminal.handle();
    };

    let next_context = context.clone();
    let next_terminal = terminal.clone();
    let next = CallHandler::from_fn(move || {
        run_interceptor_chain(remaining, next_context.clone(), next_terminal.clone())
    });
    interceptor.intercept(context, next)
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
