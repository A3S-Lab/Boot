use crate::{BootRequest, BootResponse, BoxFuture, Result};
use std::future::Future;

/// Type-erased route handler used by adapters.
pub trait RouteHandler: Send + Sync + 'static {
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>>;
}

impl<F, Fut> RouteHandler for F
where
    F: Fn(BootRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<BootResponse>> + Send + 'static,
{
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(self(request))
    }
}
