use super::erased::{
    ErasedCommandHandler, ErasedEventHandler, ErasedQueryHandler, TypedCommandHandler,
    TypedEventHandler, TypedQueryHandler,
};
use super::handlers::{CommandHandler, EventHandler, QueryHandler};
use super::messages::{Command, CqrsEvent, Query};
use std::any::{type_name, TypeId};
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// Type-erased command handler definition registered by [`crate::CqrsModule`].
#[derive(Clone)]
pub struct CommandHandlerDefinition {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) handler: Arc<dyn ErasedCommandHandler>,
}

impl fmt::Debug for CommandHandlerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CommandHandlerDefinition")
            .field("type_name", &self.type_name)
            .finish_non_exhaustive()
    }
}

impl CommandHandlerDefinition {
    pub fn new<C, H>(handler: H) -> Self
    where
        C: Command,
        H: CommandHandler<C>,
    {
        Self::from_arc::<C>(Arc::new(handler))
    }

    pub fn from_arc<C>(handler: Arc<dyn CommandHandler<C>>) -> Self
    where
        C: Command,
    {
        Self {
            type_id: TypeId::of::<C>(),
            type_name: type_name::<C>(),
            handler: Arc::new(TypedCommandHandler::<C> {
                inner: handler,
                marker: PhantomData,
            }),
        }
    }
}

/// Type-erased query handler definition registered by [`crate::CqrsModule`].
#[derive(Clone)]
pub struct QueryHandlerDefinition {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) handler: Arc<dyn ErasedQueryHandler>,
}

impl fmt::Debug for QueryHandlerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueryHandlerDefinition")
            .field("type_name", &self.type_name)
            .finish_non_exhaustive()
    }
}

impl QueryHandlerDefinition {
    pub fn new<Q, H>(handler: H) -> Self
    where
        Q: Query,
        H: QueryHandler<Q>,
    {
        Self::from_arc::<Q>(Arc::new(handler))
    }

    pub fn from_arc<Q>(handler: Arc<dyn QueryHandler<Q>>) -> Self
    where
        Q: Query,
    {
        Self {
            type_id: TypeId::of::<Q>(),
            type_name: type_name::<Q>(),
            handler: Arc::new(TypedQueryHandler::<Q> {
                inner: handler,
                marker: PhantomData,
            }),
        }
    }
}

/// Type-erased event handler definition registered by [`crate::CqrsModule`].
#[derive(Clone)]
pub struct EventHandlerDefinition {
    pub(crate) type_id: TypeId,
    pub(crate) type_name: &'static str,
    pub(crate) handler: Arc<dyn ErasedEventHandler>,
}

impl fmt::Debug for EventHandlerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventHandlerDefinition")
            .field("type_name", &self.type_name)
            .finish_non_exhaustive()
    }
}

impl EventHandlerDefinition {
    pub fn new<E, H>(handler: H) -> Self
    where
        E: CqrsEvent,
        H: EventHandler<E>,
    {
        Self::from_arc::<E>(Arc::new(handler))
    }

    pub fn from_arc<E>(handler: Arc<dyn EventHandler<E>>) -> Self
    where
        E: CqrsEvent,
    {
        Self {
            type_id: TypeId::of::<E>(),
            type_name: type_name::<E>(),
            handler: Arc::new(TypedEventHandler::<E> {
                inner: handler,
                marker: PhantomData,
            }),
        }
    }
}
