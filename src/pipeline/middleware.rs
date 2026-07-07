use crate::{BootRequest, BootResponse, BoxFuture, Result};
use std::future::Future;

/// Result of running a middleware.
pub enum MiddlewareOutcome {
    Continue(BootRequest),
    Respond(BootResponse),
}

impl MiddlewareOutcome {
    pub fn next(request: BootRequest) -> Self {
        Self::Continue(request)
    }

    pub fn response(response: BootResponse) -> Self {
        Self::Respond(response)
    }
}

/// Request middleware that runs before pipes, guards, interceptors, and handlers.
pub trait Middleware: Send + Sync + 'static {
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>>;
}

impl<F, Fut> Middleware for F
where
    F: Fn(BootRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<MiddlewareOutcome>> + Send + 'static,
{
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        Box::pin(self(request))
    }
}
