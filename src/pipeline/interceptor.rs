use super::ExecutionContext;
use crate::{BootResponse, BoxFuture, Result};

/// Runs around the handler for cross-cutting behavior.
pub trait Interceptor: Send + Sync + 'static {
    fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
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
