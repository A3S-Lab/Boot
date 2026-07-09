use super::ExecutionContext;
use crate::{
    BootError, BootErrorKind, BootResponse, BoxFuture, Result, TransportContext, TransportReply,
    WebSocketContext, WebSocketMessage,
};

/// Maps route/pipeline errors to HTTP responses.
pub trait ExceptionFilter: Send + Sync + 'static {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>>;
}

/// Handled transport exception response.
#[derive(Debug, Clone, PartialEq)]
pub struct TransportExceptionResponse {
    reply: Option<TransportReply>,
}

impl TransportExceptionResponse {
    pub fn reply(reply: TransportReply) -> Self {
        Self { reply: Some(reply) }
    }

    pub fn empty() -> Self {
        Self { reply: None }
    }

    pub fn into_reply(self) -> Option<TransportReply> {
        self.reply
    }
}

impl From<TransportReply> for TransportExceptionResponse {
    fn from(reply: TransportReply) -> Self {
        Self::reply(reply)
    }
}

/// Maps transport pipeline errors to protocol replies.
pub trait TransportExceptionFilter: Send + Sync + 'static {
    fn catch(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>>;
}

impl<F, Fut> TransportExceptionFilter for F
where
    F: Fn(TransportContext, BootError) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<TransportExceptionResponse>>> + Send + 'static,
{
    fn catch(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        Box::pin(self(context, error))
    }
}

/// Handled WebSocket exception response.
#[derive(Debug, Clone, PartialEq)]
pub struct WebSocketExceptionResponse {
    message: Option<WebSocketMessage>,
}

impl WebSocketExceptionResponse {
    pub fn message(message: WebSocketMessage) -> Self {
        Self {
            message: Some(message),
        }
    }

    pub fn empty() -> Self {
        Self { message: None }
    }

    pub fn into_message(self) -> Option<WebSocketMessage> {
        self.message
    }
}

impl From<WebSocketMessage> for WebSocketExceptionResponse {
    fn from(message: WebSocketMessage) -> Self {
        Self::message(message)
    }
}

/// Maps WebSocket gateway errors to outbound messages.
pub trait WebSocketExceptionFilter: Send + Sync + 'static {
    fn catch(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>>;
}

impl<F, Fut> WebSocketExceptionFilter for F
where
    F: Fn(WebSocketContext, BootError) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Option<WebSocketExceptionResponse>>> + Send + 'static,
{
    fn catch(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
        Box::pin(self(context, error))
    }
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

impl<F> TransportExceptionFilter for CatchFilter<F>
where
    F: TransportExceptionFilter,
{
    fn catch(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        if self.kinds.is_empty() || self.kinds.contains(&error.kind()) {
            return self.filter.catch(context, error);
        }

        Box::pin(async { Ok(None) })
    }
}

impl<F> WebSocketExceptionFilter for CatchFilter<F>
where
    F: WebSocketExceptionFilter,
{
    fn catch(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
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
{
    CatchFilter::new(kinds, filter)
}
