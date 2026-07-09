use super::context::CqrsContext;
use super::handlers::{CommandHandler, EventHandler, QueryHandler};
use super::messages::{Command, CqrsEvent, Query};
use crate::{BootError, BoxFuture, Result};
use std::any::{type_name, Any};
use std::marker::PhantomData;
use std::sync::Arc;

pub(crate) trait ErasedCommandHandler: Send + Sync + 'static {
    fn execute(
        &self,
        command: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>>;
}

pub(crate) struct TypedCommandHandler<C>
where
    C: Command,
{
    pub(crate) inner: Arc<dyn CommandHandler<C>>,
    pub(crate) marker: PhantomData<fn(C)>,
}

impl<C> ErasedCommandHandler for TypedCommandHandler<C>
where
    C: Command,
{
    fn execute(
        &self,
        command: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>> {
        let handler = Arc::clone(&self.inner);
        Box::pin(async move {
            let command = *command.downcast::<C>().map_err(|_| {
                BootError::Internal(format!(
                    "command handler received the wrong command type: {}",
                    type_name::<C>()
                ))
            })?;
            let output = handler.execute(command, context).await?;
            Ok(Box::new(output) as Box<dyn Any + Send>)
        })
    }
}

pub(crate) trait ErasedQueryHandler: Send + Sync + 'static {
    fn execute(
        &self,
        query: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>>;
}

pub(crate) struct TypedQueryHandler<Q>
where
    Q: Query,
{
    pub(crate) inner: Arc<dyn QueryHandler<Q>>,
    pub(crate) marker: PhantomData<fn(Q)>,
}

impl<Q> ErasedQueryHandler for TypedQueryHandler<Q>
where
    Q: Query,
{
    fn execute(
        &self,
        query: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>> {
        let handler = Arc::clone(&self.inner);
        Box::pin(async move {
            let query = *query.downcast::<Q>().map_err(|_| {
                BootError::Internal(format!(
                    "query handler received the wrong query type: {}",
                    type_name::<Q>()
                ))
            })?;
            let output = handler.execute(query, context).await?;
            Ok(Box::new(output) as Box<dyn Any + Send>)
        })
    }
}

pub(crate) trait ErasedEventHandler: Send + Sync + 'static {
    fn handle(
        &self,
        event: &(dyn Any + Send + Sync),
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<()>>;
}

pub(crate) struct TypedEventHandler<E>
where
    E: CqrsEvent,
{
    pub(crate) inner: Arc<dyn EventHandler<E>>,
    pub(crate) marker: PhantomData<fn(E)>,
}

impl<E> ErasedEventHandler for TypedEventHandler<E>
where
    E: CqrsEvent,
{
    fn handle(
        &self,
        event: &(dyn Any + Send + Sync),
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<()>> {
        let handler = Arc::clone(&self.inner);
        let event = match event.downcast_ref::<E>() {
            Some(event) => event.clone(),
            None => {
                return Box::pin(async {
                    Err(BootError::Internal(format!(
                        "event handler received the wrong event type: {}",
                        type_name::<E>()
                    )))
                });
            }
        };
        Box::pin(async move { handler.handle(event, context).await })
    }
}

pub(crate) fn downcast_output<T>(
    output: Box<dyn Any + Send>,
    handler_name: &'static str,
) -> Result<T>
where
    T: Send + 'static,
{
    output.downcast::<T>().map(|output| *output).map_err(|_| {
        BootError::Internal(format!(
            "CQRS handler returned the wrong output type: {handler_name}"
        ))
    })
}
