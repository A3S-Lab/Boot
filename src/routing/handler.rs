use crate::{BootError, BootRequest, BootResponse, BoxFuture, ModuleRef, Result};
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

pub(crate) struct RequestScopedRouteHandler<F> {
    factory: F,
}

impl<F> RequestScopedRouteHandler<F> {
    pub(crate) fn new(factory: F) -> Self {
        Self { factory }
    }
}

impl<F, H> RouteHandler for RequestScopedRouteHandler<F>
where
    F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
    H: RouteHandler,
{
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>> {
        let Some(module_ref) = request.module_ref().cloned() else {
            return Box::pin(async {
                Err(BootError::Internal(
                    "request-scoped route requires a module context".to_string(),
                ))
            });
        };

        match (self.factory)(&module_ref) {
            Ok(handler) => handler.call(request),
            Err(error) => Box::pin(async move { Err(error) }),
        }
    }
}
