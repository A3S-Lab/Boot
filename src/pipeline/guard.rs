use super::ExecutionContext;
use crate::{BoxFuture, Result};

/// Decides whether a route handler can run.
pub trait Guard: Send + Sync + 'static {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>>;
}

impl<F, Fut> Guard for F
where
    F: Fn(ExecutionContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<bool>> + Send + 'static,
{
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(self(context))
    }
}
