use super::context::WebSocketContext;
use super::message::WebSocketMessage;
use crate::{BoxFuture, ExecutionInterceptor, Guard, Result};
use std::future::Future;
use std::sync::Arc;

/// Message transformation hook for WebSocket gateways.
pub trait WebSocketPipe: Send + Sync + 'static {
    fn transform(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<WebSocketMessage>>;
}

impl<F, Fut> WebSocketPipe for F
where
    F: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<WebSocketMessage>> + Send + 'static,
{
    fn transform(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<WebSocketMessage>> {
        Box::pin(self(message))
    }
}

/// Authorization hook for WebSocket gateway messages.
pub trait WebSocketGuard: Send + Sync + 'static {
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>>;
}

impl<F, Fut> WebSocketGuard for F
where
    F: Fn(WebSocketContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<bool>> + Send + 'static,
{
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(self(context))
    }
}

pub(crate) struct ExecutionWebSocketGuard<G> {
    pub(crate) inner: G,
}

impl<G> WebSocketGuard for ExecutionWebSocketGuard<G>
where
    G: Guard,
{
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        self.inner.can_activate(context.into_execution_context())
    }
}

/// Around-handler hook for WebSocket gateway messages.
pub trait WebSocketInterceptor: Send + Sync + 'static {
    fn before(&self, _context: WebSocketContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn after(
        &self,
        _context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        Box::pin(async move { Ok(reply) })
    }
}

pub(crate) struct ExecutionWebSocketInterceptor<I> {
    pub(crate) inner: I,
}

impl<I> WebSocketInterceptor for ExecutionWebSocketInterceptor<I>
where
    I: ExecutionInterceptor,
{
    fn before(&self, context: WebSocketContext) -> BoxFuture<'static, Result<()>> {
        self.inner.before(context.into_execution_context())
    }

    fn after(
        &self,
        context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        let future = self.inner.after(context.into_execution_context());
        Box::pin(async move {
            future.await?;
            Ok(reply)
        })
    }
}

pub(crate) fn prepend_execution_guards(
    prefix: &[Arc<dyn Guard>],
    values: Vec<Arc<dyn WebSocketGuard>>,
) -> Vec<Arc<dyn WebSocketGuard>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|guard| Arc::new(ExecutionWebSocketGuard { inner: guard }) as Arc<dyn WebSocketGuard>)
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

pub(crate) fn prepend_execution_interceptors(
    prefix: &[Arc<dyn ExecutionInterceptor>],
    values: Vec<Arc<dyn WebSocketInterceptor>>,
) -> Vec<Arc<dyn WebSocketInterceptor>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|interceptor| {
            Arc::new(ExecutionWebSocketInterceptor { inner: interceptor })
                as Arc<dyn WebSocketInterceptor>
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}
