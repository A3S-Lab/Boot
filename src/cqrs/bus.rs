use super::context::CqrsContext;
use super::definitions::{
    CommandHandlerDefinition, EventHandlerDefinition, QueryHandlerDefinition,
};
use super::erased::{
    downcast_output, ErasedCommandHandler, ErasedEventHandler, ErasedQueryHandler,
};
use super::handlers::{CommandHandler, EventHandler, QueryHandler};
use super::messages::{Command, CqrsEvent, Query};
use crate::{BootError, ModuleRef, Result};
use std::any::{type_name, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Dispatches typed command messages to one registered handler.
#[derive(Clone, Default)]
pub struct CommandBus {
    handlers: Arc<RwLock<BTreeMap<TypeId, CommandHandlerRegistration>>>,
    module_ref: Arc<RwLock<Option<ModuleRef>>>,
}

#[derive(Clone)]
struct CommandHandlerRegistration {
    type_name: &'static str,
    handler: Arc<dyn ErasedCommandHandler>,
}

impl fmt::Debug for CommandBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handler_count = self.handlers.read().map(|items| items.len()).unwrap_or(0);
        f.debug_struct("CommandBus")
            .field("handlers", &handler_count)
            .finish_non_exhaustive()
    }
}

impl CommandBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<C, H>(&self, handler: H) -> Result<()>
    where
        C: Command,
        H: CommandHandler<C>,
    {
        self.register_definition(CommandHandlerDefinition::new::<C, H>(handler))
    }

    pub fn register_definition(&self, definition: CommandHandlerDefinition) -> Result<()> {
        let mut handlers = self.write_handlers()?;
        if handlers.contains_key(&definition.type_id) {
            return Err(BootError::Internal(format!(
                "command handler is already registered: {}",
                definition.type_name
            )));
        }
        handlers.insert(
            definition.type_id,
            CommandHandlerRegistration {
                type_name: definition.type_name,
                handler: definition.handler,
            },
        );
        Ok(())
    }

    pub async fn execute<C>(&self, command: C) -> Result<C::Output>
    where
        C: Command,
    {
        let registration = self
            .read_handlers()?
            .get(&TypeId::of::<C>())
            .cloned()
            .ok_or_else(|| {
                BootError::Internal(format!(
                    "command handler is not registered: {}",
                    type_name::<C>()
                ))
            })?;
        let output = registration
            .handler
            .execute(Box::new(command), self.context()?)
            .await?;
        downcast_output::<C::Output>(output, registration.type_name)
    }

    pub fn handler_count(&self) -> Result<usize> {
        Ok(self.read_handlers()?.len())
    }

    pub fn clear_handlers(&self) -> Result<()> {
        self.write_handlers()?.clear();
        Ok(())
    }

    pub(crate) fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
        *self.write_module_ref()? = Some(module_ref);
        Ok(())
    }

    fn context(&self) -> Result<CqrsContext> {
        Ok(CqrsContext::new(
            self.read_module_ref()?
                .clone()
                .unwrap_or_else(ModuleRef::new),
        ))
    }

    fn read_handlers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<TypeId, CommandHandlerRegistration>>> {
        self.handlers
            .read()
            .map_err(|_| BootError::Internal("command handler lock is poisoned".to_string()))
    }

    fn write_handlers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<TypeId, CommandHandlerRegistration>>> {
        self.handlers
            .write()
            .map_err(|_| BootError::Internal("command handler lock is poisoned".to_string()))
    }

    fn read_module_ref(&self) -> Result<std::sync::RwLockReadGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .read()
            .map_err(|_| BootError::Internal("command module ref lock is poisoned".to_string()))
    }

    fn write_module_ref(&self) -> Result<std::sync::RwLockWriteGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .write()
            .map_err(|_| BootError::Internal("command module ref lock is poisoned".to_string()))
    }
}

/// Dispatches typed query messages to one registered handler.
#[derive(Clone, Default)]
pub struct QueryBus {
    handlers: Arc<RwLock<BTreeMap<TypeId, QueryHandlerRegistration>>>,
    module_ref: Arc<RwLock<Option<ModuleRef>>>,
}

#[derive(Clone)]
struct QueryHandlerRegistration {
    type_name: &'static str,
    handler: Arc<dyn ErasedQueryHandler>,
}

impl fmt::Debug for QueryBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handler_count = self.handlers.read().map(|items| items.len()).unwrap_or(0);
        f.debug_struct("QueryBus")
            .field("handlers", &handler_count)
            .finish_non_exhaustive()
    }
}

impl QueryBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<Q, H>(&self, handler: H) -> Result<()>
    where
        Q: Query,
        H: QueryHandler<Q>,
    {
        self.register_definition(QueryHandlerDefinition::new::<Q, H>(handler))
    }

    pub fn register_definition(&self, definition: QueryHandlerDefinition) -> Result<()> {
        let mut handlers = self.write_handlers()?;
        if handlers.contains_key(&definition.type_id) {
            return Err(BootError::Internal(format!(
                "query handler is already registered: {}",
                definition.type_name
            )));
        }
        handlers.insert(
            definition.type_id,
            QueryHandlerRegistration {
                type_name: definition.type_name,
                handler: definition.handler,
            },
        );
        Ok(())
    }

    pub async fn execute<Q>(&self, query: Q) -> Result<Q::Output>
    where
        Q: Query,
    {
        let registration = self
            .read_handlers()?
            .get(&TypeId::of::<Q>())
            .cloned()
            .ok_or_else(|| {
                BootError::Internal(format!(
                    "query handler is not registered: {}",
                    type_name::<Q>()
                ))
            })?;
        let output = registration
            .handler
            .execute(Box::new(query), self.context()?)
            .await?;
        downcast_output::<Q::Output>(output, registration.type_name)
    }

    pub fn handler_count(&self) -> Result<usize> {
        Ok(self.read_handlers()?.len())
    }

    pub fn clear_handlers(&self) -> Result<()> {
        self.write_handlers()?.clear();
        Ok(())
    }

    pub(crate) fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
        *self.write_module_ref()? = Some(module_ref);
        Ok(())
    }

    fn context(&self) -> Result<CqrsContext> {
        Ok(CqrsContext::new(
            self.read_module_ref()?
                .clone()
                .unwrap_or_else(ModuleRef::new),
        ))
    }

    fn read_handlers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<TypeId, QueryHandlerRegistration>>> {
        self.handlers
            .read()
            .map_err(|_| BootError::Internal("query handler lock is poisoned".to_string()))
    }

    fn write_handlers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<TypeId, QueryHandlerRegistration>>> {
        self.handlers
            .write()
            .map_err(|_| BootError::Internal("query handler lock is poisoned".to_string()))
    }

    fn read_module_ref(&self) -> Result<std::sync::RwLockReadGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .read()
            .map_err(|_| BootError::Internal("query module ref lock is poisoned".to_string()))
    }

    fn write_module_ref(&self) -> Result<std::sync::RwLockWriteGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .write()
            .map_err(|_| BootError::Internal("query module ref lock is poisoned".to_string()))
    }
}

/// Publishes typed event messages to all registered handlers.
#[derive(Clone, Default)]
pub struct EventBus {
    handlers: Arc<RwLock<BTreeMap<TypeId, Vec<EventHandlerRegistration>>>>,
    module_ref: Arc<RwLock<Option<ModuleRef>>>,
}

#[derive(Clone)]
struct EventHandlerRegistration {
    handler: Arc<dyn ErasedEventHandler>,
}

impl fmt::Debug for EventBus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let handler_count = self
            .handlers
            .read()
            .map(|items| items.values().map(Vec::len).sum::<usize>())
            .unwrap_or(0);
        f.debug_struct("EventBus")
            .field("handlers", &handler_count)
            .finish_non_exhaustive()
    }
}

impl EventBus {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register<E, H>(&self, handler: H) -> Result<()>
    where
        E: CqrsEvent,
        H: EventHandler<E>,
    {
        self.register_definition(EventHandlerDefinition::new::<E, H>(handler))
    }

    pub fn register_definition(&self, definition: EventHandlerDefinition) -> Result<()> {
        self.write_handlers()?
            .entry(definition.type_id)
            .or_default()
            .push(EventHandlerRegistration {
                handler: definition.handler,
            });
        Ok(())
    }

    pub async fn publish<E>(&self, event: E) -> Result<usize>
    where
        E: CqrsEvent,
    {
        let handlers = self
            .read_handlers()?
            .get(&TypeId::of::<E>())
            .cloned()
            .unwrap_or_default();
        let count = handlers.len();
        for registration in handlers {
            registration.handler.handle(&event, self.context()?).await?;
        }
        Ok(count)
    }

    pub fn handler_count(&self) -> Result<usize> {
        Ok(self.read_handlers()?.values().map(Vec::len).sum::<usize>())
    }

    pub fn clear_handlers(&self) -> Result<()> {
        self.write_handlers()?.clear();
        Ok(())
    }

    pub(crate) fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
        *self.write_module_ref()? = Some(module_ref);
        Ok(())
    }

    fn context(&self) -> Result<CqrsContext> {
        Ok(CqrsContext::new(
            self.read_module_ref()?
                .clone()
                .unwrap_or_else(ModuleRef::new),
        ))
    }

    fn read_handlers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<TypeId, Vec<EventHandlerRegistration>>>>
    {
        self.handlers
            .read()
            .map_err(|_| BootError::Internal("event handler lock is poisoned".to_string()))
    }

    fn write_handlers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<TypeId, Vec<EventHandlerRegistration>>>>
    {
        self.handlers
            .write()
            .map_err(|_| BootError::Internal("event handler lock is poisoned".to_string()))
    }

    fn read_module_ref(&self) -> Result<std::sync::RwLockReadGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .read()
            .map_err(|_| BootError::Internal("event module ref lock is poisoned".to_string()))
    }

    fn write_module_ref(&self) -> Result<std::sync::RwLockWriteGuard<'_, Option<ModuleRef>>> {
        self.module_ref
            .write()
            .map_err(|_| BootError::Internal("event module ref lock is poisoned".to_string()))
    }
}
