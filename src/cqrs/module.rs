use super::bus::{CommandBus, EventBus, QueryBus};
use super::definitions::{
    CommandHandlerDefinition, EventHandlerDefinition, QueryHandlerDefinition,
};
use super::handlers::{CommandHandler, EventHandler, QueryHandler};
use super::messages::{Command, CqrsEvent, Query};
use crate::{Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
use std::fmt;
use std::sync::Arc;

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
