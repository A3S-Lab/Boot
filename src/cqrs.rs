use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
use std::any::{type_name, Any, TypeId};
use std::collections::BTreeMap;
use std::fmt;
use std::marker::PhantomData;
use std::sync::{Arc, RwLock};

/// Command message handled by [`CommandBus`].
pub trait Command: Send + 'static {
    type Output: Send + 'static;
}

/// Query message handled by [`QueryBus`].
pub trait Query: Send + 'static {
    type Output: Send + 'static;
}

/// Event message published through [`EventBus`].
pub trait CqrsEvent: Clone + Send + Sync + 'static {}

impl<T> CqrsEvent for T where T: Clone + Send + Sync + 'static {}

/// Context passed to CQRS handlers.
#[derive(Debug, Clone)]
pub struct CqrsContext {
    module_ref: ModuleRef,
}

impl CqrsContext {
    pub fn new(module_ref: ModuleRef) -> Self {
        Self { module_ref }
    }

    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_named::<T>(token)
    }
}

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

/// Type-erased command handler definition registered by [`CqrsModule`].
#[derive(Clone)]
pub struct CommandHandlerDefinition {
    type_id: TypeId,
    type_name: &'static str,
    handler: Arc<dyn ErasedCommandHandler>,
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

/// Type-erased query handler definition registered by [`CqrsModule`].
#[derive(Clone)]
pub struct QueryHandlerDefinition {
    type_id: TypeId,
    type_name: &'static str,
    handler: Arc<dyn ErasedQueryHandler>,
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

/// Type-erased event handler definition registered by [`CqrsModule`].
#[derive(Clone)]
pub struct EventHandlerDefinition {
    type_id: TypeId,
    type_name: &'static str,
    handler: Arc<dyn ErasedEventHandler>,
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

    fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
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

    fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
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

    fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
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

/// Module that registers CQRS buses and handler definitions.
#[derive(Clone)]
pub struct CqrsModule {
    name: &'static str,
    command_bus: Arc<CommandBus>,
    query_bus: Arc<QueryBus>,
    event_bus: Arc<EventBus>,
    imports: Vec<Arc<dyn Module>>,
    providers: Vec<ProviderDefinition>,
    command_handlers: Vec<CommandHandlerDefinition>,
    query_handlers: Vec<QueryHandlerDefinition>,
    event_handlers: Vec<EventHandlerDefinition>,
    global: bool,
}

impl fmt::Debug for CqrsModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CqrsModule")
            .field("name", &self.name)
            .field("command_handlers", &self.command_handlers.len())
            .field("query_handlers", &self.query_handlers.len())
            .field("event_handlers", &self.event_handlers.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl CqrsModule {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            command_bus: Arc::new(CommandBus::new()),
            query_bus: Arc::new(QueryBus::new()),
            event_bus: Arc::new(EventBus::new()),
            imports: Vec::new(),
            providers: Vec::new(),
            command_handlers: Vec::new(),
            query_handlers: Vec::new(),
            event_handlers: Vec::new(),
            global: false,
        }
    }

    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.imports.push(Arc::new(module));
        self
    }

    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.imports.push(module);
        self
    }

    pub fn provider(mut self, provider: ProviderDefinition) -> Self {
        self.providers.push(provider);
        self
    }

    pub fn command_handler<C, H>(mut self, handler: H) -> Self
    where
        C: Command,
        H: CommandHandler<C>,
    {
        self.command_handlers
            .push(CommandHandlerDefinition::new::<C, H>(handler));
        self
    }

    pub fn command_handler_definition(mut self, definition: CommandHandlerDefinition) -> Self {
        self.command_handlers.push(definition);
        self
    }

    pub fn query_handler<Q, H>(mut self, handler: H) -> Self
    where
        Q: Query,
        H: QueryHandler<Q>,
    {
        self.query_handlers
            .push(QueryHandlerDefinition::new::<Q, H>(handler));
        self
    }

    pub fn query_handler_definition(mut self, definition: QueryHandlerDefinition) -> Self {
        self.query_handlers.push(definition);
        self
    }

    pub fn event_handler<E, H>(mut self, handler: H) -> Self
    where
        E: CqrsEvent,
        H: EventHandler<E>,
    {
        self.event_handlers
            .push(EventHandlerDefinition::new::<E, H>(handler));
        self
    }

    pub fn event_handler_definition(mut self, definition: EventHandlerDefinition) -> Self {
        self.event_handlers.push(definition);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for CqrsModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        self.imports.clone()
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let mut providers = vec![
            ProviderDefinition::from_arc(Arc::clone(&self.command_bus)),
            ProviderDefinition::from_arc(Arc::clone(&self.query_bus)),
            ProviderDefinition::from_arc(Arc::clone(&self.event_bus)),
        ];
        providers.extend(self.providers.clone());
        Ok(providers)
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![
            ProviderToken::of::<CommandBus>(),
            ProviderToken::of::<QueryBus>(),
            ProviderToken::of::<EventBus>(),
        ])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        self.command_bus.attach_module_ref(module_ref.clone())?;
        self.query_bus.attach_module_ref(module_ref.clone())?;
        self.event_bus.attach_module_ref(module_ref.clone())?;

        for handler in &self.command_handlers {
            self.command_bus.register_definition(handler.clone())?;
        }
        for handler in &self.query_handlers {
            self.query_bus.register_definition(handler.clone())?;
        }
        for handler in &self.event_handlers {
            self.event_bus.register_definition(handler.clone())?;
        }

        Ok(())
    }
}

trait ErasedCommandHandler: Send + Sync + 'static {
    fn execute(
        &self,
        command: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>>;
}

struct TypedCommandHandler<C>
where
    C: Command,
{
    inner: Arc<dyn CommandHandler<C>>,
    marker: PhantomData<fn(C)>,
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

trait ErasedQueryHandler: Send + Sync + 'static {
    fn execute(
        &self,
        query: Box<dyn Any + Send>,
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<Box<dyn Any + Send>>>;
}

struct TypedQueryHandler<Q>
where
    Q: Query,
{
    inner: Arc<dyn QueryHandler<Q>>,
    marker: PhantomData<fn(Q)>,
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

trait ErasedEventHandler: Send + Sync + 'static {
    fn handle(
        &self,
        event: &(dyn Any + Send + Sync),
        context: CqrsContext,
    ) -> BoxFuture<'static, Result<()>>;
}

struct TypedEventHandler<E>
where
    E: CqrsEvent,
{
    inner: Arc<dyn EventHandler<E>>,
    marker: PhantomData<fn(E)>,
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

fn downcast_output<T>(output: Box<dyn Any + Send>, handler_name: &'static str) -> Result<T>
where
    T: Send + 'static,
{
    output.downcast::<T>().map(|output| *output).map_err(|_| {
        BootError::Internal(format!(
            "CQRS handler returned the wrong output type: {handler_name}"
        ))
    })
}
