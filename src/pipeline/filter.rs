use super::ExecutionContext;
use crate::{BootError, BootResponse, BoxFuture, Result};

/// Maps route/pipeline errors to HTTP responses.
pub trait ExceptionFilter: Send + Sync + 'static {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>>;
}

impl<F, Fut> ExceptionFilter for F
where
    F: Fn(ExecutionContext, BootError) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<BootResponse>>> + Send + 'static,
{
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(self(context, error))
    }
}
