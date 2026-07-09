use super::context::CqrsContext;
use super::messages::{Command, CqrsEvent, Query};
use crate::{BoxFuture, Result};

/// Handler for one command type.
pub trait CommandHandler<C>: Send + Sync + 'static
where
    C: Command,
{
    fn execute(&self, command: C, context: CqrsContext) -> BoxFuture<'static, Result<C::Output>>;
}

impl<C, F, Fut> CommandHandler<C> for F
where
    C: Command,
    F: Fn(C, CqrsContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<C::Output>> + Send + 'static,
{
    fn execute(&self, command: C, context: CqrsContext) -> BoxFuture<'static, Result<C::Output>> {
        Box::pin(self(command, context))
    }
}

/// Handler for one query type.
pub trait QueryHandler<Q>: Send + Sync + 'static
where
    Q: Query,
{
    fn execute(&self, query: Q, context: CqrsContext) -> BoxFuture<'static, Result<Q::Output>>;
}

impl<Q, F, Fut> QueryHandler<Q> for F
where
    Q: Query,
    F: Fn(Q, CqrsContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<Q::Output>> + Send + 'static,
{
    fn execute(&self, query: Q, context: CqrsContext) -> BoxFuture<'static, Result<Q::Output>> {
        Box::pin(self(query, context))
    }
}

/// Handler for one CQRS event type.
pub trait EventHandler<E>: Send + Sync + 'static
where
    E: CqrsEvent,
{
    fn handle(&self, event: E, context: CqrsContext) -> BoxFuture<'static, Result<()>>;
}

impl<E, F, Fut> EventHandler<E> for F
where
    E: CqrsEvent,
    F: Fn(E, CqrsContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn handle(&self, event: E, context: CqrsContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(event, context))
    }
}
