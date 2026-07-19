use super::ExecutionContext;
use crate::{BootError, BootResponse, BoxFuture, Result};
use std::future::Future;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Reusable access to the next handler in an interceptor chain.
///
/// This is the Rust equivalent of Nest's `CallHandler`. Calling [`handle`](Self::handle)
/// runs the remaining interceptors, pipes, validation, and handler. The handle is
/// reusable so an interceptor can deliberately retry the downstream pipeline
/// sequentially. Concurrent calls are rejected because they would share one
/// request-scoped provider context.
pub struct CallHandler<'a, T = BootResponse> {
    call: Arc<dyn Fn() -> BoxFuture<'a, Result<T>> + Send + Sync + 'a>,
    running: Arc<AtomicBool>,
}

impl<'a, T> Clone for CallHandler<'a, T> {
    fn clone(&self) -> Self {
        Self {
            call: Arc::clone(&self.call),
            running: Arc::clone(&self.running),
        }
    }
}

impl<'a, T> CallHandler<'a, T>
where
    T: Send + 'a,
{
    /// Build a call handler from a reusable async function.
    ///
    /// Frameworks normally provide the handler to an interceptor. This
    /// constructor is public so interceptors and combinators can be tested in
    /// isolation.
    pub fn from_fn<F, Fut>(call: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'a,
        Fut: Future<Output = Result<T>> + Send + 'a,
    {
        Self {
            call: Arc::new(move || Box::pin(call())),
            running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Run the remaining interceptor chain and underlying handler once.
    ///
    /// A completed or cancelled call releases the handler for a later retry.
    /// Starting overlapping calls returns [`BootError::Internal`].
    pub fn handle(&self) -> BoxFuture<'a, Result<T>> {
        let call = Arc::clone(&self.call);
        let running = Arc::clone(&self.running);
        Box::pin(async move {
            if running
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
                .is_err()
            {
                return Err(BootError::Internal(
                    "call handler is already running".to_string(),
                ));
            }

            let _reset = CallHandlerReset {
                running: Arc::clone(&running),
            };
            call().await
        })
    }
}

struct CallHandlerReset {
    running: Arc<AtomicBool>,
}

impl Drop for CallHandlerReset {
    fn drop(&mut self) {
        self.running.store(false, Ordering::Release);
    }
}

/// Runs around the handler for cross-cutting behavior.
pub trait Interceptor: Send + Sync + 'static {
    /// Run around the remaining HTTP pipeline.
    ///
    /// Override this method to catch or replace downstream errors, retry the
    /// handler, apply a timeout, or return a response without calling `next`.
    /// The default implementation preserves the legacy `before`,
    /// `short_circuit`, and `after` hook behavior.
    fn intercept<'a>(
        &'a self,
        context: ExecutionContext,
        next: CallHandler<'a>,
    ) -> BoxFuture<'a, Result<BootResponse>> {
        Box::pin(async move {
            self.before(context.clone()).await?;

            if let Some(response) = self.short_circuit(context.clone()).await? {
                return Ok(response);
            }

            let response = next.handle().await?;
            self.after(context, response).await
        })
    }

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
