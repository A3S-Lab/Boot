use super::ExecutionContext;
use crate::{BootError, BootErrorKind, BootResponse, BoxFuture, Result};

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

/// Exception filter wrapper that only handles selected [`BootErrorKind`] values.
pub struct CatchFilter<F> {
    kinds: Vec<BootErrorKind>,
    filter: F,
}

impl<F> CatchFilter<F> {
    pub fn new<I>(kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
    {
        Self {
            kinds: kinds.into_iter().collect(),
            filter,
        }
    }

    pub fn kinds(&self) -> &[BootErrorKind] {
        &self.kinds
    }

    pub fn into_inner(self) -> F {
        self.filter
    }
}

impl<F> ExceptionFilter for CatchFilter<F>
where
    F: ExceptionFilter,
{
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        if self.kinds.is_empty() || self.kinds.contains(&error.kind()) {
            return self.filter.catch(context, error);
        }

        Box::pin(async { Ok(None) })
    }
}

/// Build a Nest-style catch filter for selected [`BootErrorKind`] values.
pub fn catch_errors<I, F>(kinds: I, filter: F) -> CatchFilter<F>
where
    I: IntoIterator<Item = BootErrorKind>,
    F: ExceptionFilter,
{
    CatchFilter::new(kinds, filter)
}
