use super::ExecutionContext;
use crate::{BootResponse, BoxFuture, Result};
use std::sync::Arc;

/// Runs around the handler for cross-cutting behavior.
pub trait Interceptor: Send + Sync + 'static {
    fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn short_circuit(
        &self,
        _context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async { Ok(None) })
    }

    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(async move { Ok(response) })
    }
}

impl<F, Fut> Interceptor for F
where
    F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(context))
    }
}

/// Protocol-neutral observer that can run around HTTP, WebSocket, or transport handlers.
pub trait ExecutionInterceptor: Send + Sync + 'static {
    fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn after(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

impl<F, Fut> ExecutionInterceptor for F
where
    F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(context))
    }
}

impl ExecutionInterceptor for Arc<dyn ExecutionInterceptor> {
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        self.as_ref().before(context)
    }

    fn after(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        self.as_ref().after(context)
    }
}

pub(crate) struct ExecutionInterceptorAdapter<I> {
    inner: I,
}

impl<I> ExecutionInterceptorAdapter<I> {
    pub(crate) fn new(inner: I) -> Self {
        Self { inner }
    }
}

impl<I> Interceptor for ExecutionInterceptorAdapter<I>
where
    I: ExecutionInterceptor,
{
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        self.inner.before(context)
    }

    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let future = self.inner.after(context);
        Box::pin(async move {
            future.await?;
            Ok(response)
        })
    }
}
