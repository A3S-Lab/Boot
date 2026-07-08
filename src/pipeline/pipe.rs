use crate::{BootRequest, BoxFuture, Result};

/// Transforms a request after guards and interceptor `before` hooks, before handlers run.
pub trait Pipe: Send + Sync + 'static {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>>;
}

impl<F, Fut> Pipe for F
where
    F: Fn(BootRequest) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<BootRequest>> + Send + 'static,
{
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        Box::pin(self(request))
    }
}
