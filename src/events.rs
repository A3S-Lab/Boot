use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
pub use a3s_event::{
    Event as A3sEvent, EventBus as A3sEventBus, EventProvider as A3sEventProvider,
    MemoryConfig as A3sMemoryEventConfig, MemoryProvider as A3sMemoryEventProvider,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Application event payload dispatched by [`EventEmitter`].
#[derive(Debug, Clone)]
pub struct EventEnvelope {
    name: String,
    data: Value,
    event: A3sEvent,
}

impl PartialEq for EventEnvelope {
    fn eq(&self, other: &Self) -> bool {
        self.name == other.name && self.data == other.data
    }
}

impl EventEnvelope {
    pub fn new(name: impl Into<String>, data: Value) -> Result<Self> {
        let name = validate_event_name(name.into())?;
        let event = a3s_event_from_name(&name, data.clone());
        Ok(Self { name, data, event })
    }

    pub fn json<T>(name: impl Into<String>, data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let data = serde_json::to_value(data)
            .map_err(|error| BootError::Internal(format!("failed to serialize event: {error}")))?;
        Self::new(name, data)
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn as_a3s_event(&self) -> &A3sEvent {
        &self.event
    }

    pub fn data_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.data.clone()).map_err(|error| {
            BootError::Internal(format!("invalid event payload for {}: {error}", self.name))
        })
    }

    pub fn into_data(self) -> Value {
        self.data
    }

    pub fn into_a3s_event(self) -> A3sEvent {
        self.event
    }
}

/// Context passed to event listeners.
#[derive(Debug, Clone)]
pub struct EventContext {
    module_ref: ModuleRef,
}

impl EventContext {
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

/// Async event listener invoked by [`EventEmitter`].
pub trait EventListener: Send + Sync + 'static {
    fn handle(&self, event: EventEnvelope, context: EventContext)
        -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> EventListener for F
where
    F: Fn(EventEnvelope, EventContext) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<()>> + Send + 'static,
{
    fn handle(
        &self,
        event: EventEnvelope,
        context: EventContext,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(event, context))
    }
}

/// A listener definition that can be registered with an [`EventModule`].
#[derive(Clone)]
pub struct EventListenerDefinition {
    pattern: String,
    listener: Arc<dyn EventListener>,
}

impl fmt::Debug for EventListenerDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventListenerDefinition")
            .field("pattern", &self.pattern)
            .finish_non_exhaustive()
    }
}

impl EventListenerDefinition {
    pub fn new<L>(pattern: impl Into<String>, listener: L) -> Self
    where
        L: EventListener,
    {
        Self::from_arc(pattern, Arc::new(listener))
    }

    pub fn from_arc(pattern: impl Into<String>, listener: Arc<dyn EventListener>) -> Self {
        Self {
            pattern: pattern.into(),
            listener,
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }
}

/// In-process async event emitter exposed as a provider by [`EventModule`].
#[derive(Clone)]
pub struct EventEmitter {
    bus: Arc<A3sEventBus>,
    listeners: Arc<RwLock<Vec<EventListenerRegistration>>>,
    module_ref: Arc<RwLock<Option<ModuleRef>>>,
}

#[derive(Clone)]
struct EventListenerRegistration {
    subject_filter: String,
    listener: Arc<dyn EventListener>,
}

impl fmt::Debug for EventEmitter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let listener_count = self.listeners.read().map(|items| items.len()).unwrap_or(0);
        f.debug_struct("EventEmitter")
            .field("provider", &self.bus.provider_name())
            .field("listeners", &listener_count)
            .finish_non_exhaustive()
    }
}

impl Default for EventEmitter {
    fn default() -> Self {
        Self::new()
    }
}

impl EventEmitter {
    pub fn new() -> Self {
        Self::from_provider(A3sMemoryEventProvider::default())
    }

    pub fn from_provider<P>(provider: P) -> Self
    where
        P: A3sEventProvider + 'static,
    {
        Self::from_event_bus(A3sEventBus::new(provider))
    }

    pub fn from_event_bus(bus: A3sEventBus) -> Self {
        Self {
            bus: Arc::new(bus),
            listeners: Arc::new(RwLock::new(Vec::new())),
            module_ref: Arc::new(RwLock::new(None)),
        }
    }

    pub fn event_bus(&self) -> Arc<A3sEventBus> {
        Arc::clone(&self.bus)
    }

    pub fn on<L>(&self, pattern: impl Into<String>, listener: L) -> Result<()>
    where
        L: EventListener,
    {
        self.on_arc(pattern, Arc::new(listener))
    }

    pub fn on_arc(
        &self,
        pattern: impl Into<String>,
        listener: Arc<dyn EventListener>,
    ) -> Result<()> {
        let pattern = validate_event_pattern(pattern.into())?;
        let subject_filter = event_pattern_subject_filter(&pattern);
        self.write_listeners()?.push(EventListenerRegistration {
            subject_filter,
            listener,
        });
        Ok(())
    }

    pub async fn emit<T>(&self, name: impl Into<String>, data: &T) -> Result<usize>
    where
        T: Serialize,
    {
        self.emit_event(EventEnvelope::json(name, data)?).await
    }

    pub async fn emit_value(&self, name: impl Into<String>, data: Value) -> Result<usize> {
        self.emit_event(EventEnvelope::new(name, data)?).await
    }

    pub async fn emit_event(&self, event: EventEnvelope) -> Result<usize> {
        self.bus
            .publish_event(event.as_a3s_event())
            .await
            .map_err(event_error)?;

        let listeners = self.matching_listeners(event.as_a3s_event())?;
        let context = EventContext::new(self.module_ref()?);
        let listener_count = listeners.len();

        for listener in listeners {
            listener.handle(event.clone(), context.clone()).await?;
        }

        Ok(listener_count)
    }

    pub fn listener_count(&self) -> Result<usize> {
        Ok(self.read_listeners()?.len())
    }

    pub fn clear_listeners(&self) -> Result<()> {
        self.write_listeners()?.clear();
        Ok(())
    }

    pub(crate) fn attach_module_ref(&self, module_ref: ModuleRef) -> Result<()> {
        *self.write_module_ref()? = Some(module_ref);
        Ok(())
    }

    fn matching_listeners(&self, event: &A3sEvent) -> Result<Vec<Arc<dyn EventListener>>> {
        Ok(self
            .read_listeners()?
            .iter()
            .filter(|registration| {
                event_subject_matches(&registration.subject_filter, event.subject.as_str())
            })
            .map(|registration| Arc::clone(&registration.listener))
            .collect())
    }

    fn module_ref(&self) -> Result<ModuleRef> {
        Ok(self
            .read_module_ref()?
            .clone()
            .unwrap_or_else(ModuleRef::new))
    }

    fn read_listeners(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, Vec<EventListenerRegistration>>> {
        self.listeners
            .read()
            .map_err(|_| BootError::Internal("event listener lock is poisoned".to_string()))
    }

    fn write_listeners(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, Vec<EventListenerRegistration>>> {
        self.listeners
            .write()
            .map_err(|_| BootError::Internal("event listener lock is poisoned".to_string()))
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

/// Module that registers and exports an [`EventEmitter`] provider.
#[derive(Clone)]
pub struct EventModule {
    name: &'static str,
    token: ProviderToken,
    event_bus_token: ProviderToken,
    emitter: Arc<EventEmitter>,
    listeners: Vec<EventListenerDefinition>,
    global: bool,
}

impl fmt::Debug for EventModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("EventModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("event_bus_token", &self.event_bus_token)
            .field("listeners", &self.listeners.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl EventModule {
    pub fn in_process(name: &'static str) -> Self {
        Self::from_provider(name, A3sMemoryEventProvider::default())
    }

    pub fn from_emitter(name: &'static str, emitter: EventEmitter) -> Self {
        Self {
            name,
            token: ProviderToken::of::<EventEmitter>(),
            event_bus_token: ProviderToken::of::<A3sEventBus>(),
            emitter: Arc::new(emitter),
            listeners: Vec::new(),
            global: false,
        }
    }

    pub fn from_provider<P>(name: &'static str, provider: P) -> Self
    where
        P: A3sEventProvider + 'static,
    {
        Self::from_event_bus(name, A3sEventBus::new(provider))
    }

    pub fn from_event_bus(name: &'static str, bus: A3sEventBus) -> Self {
        Self::from_emitter(name, EventEmitter::from_event_bus(bus))
    }

    pub fn listener<L>(mut self, pattern: impl Into<String>, listener: L) -> Self
    where
        L: EventListener,
    {
        self.listeners
            .push(EventListenerDefinition::new(pattern, listener));
        self
    }

    pub fn listener_arc(
        mut self,
        pattern: impl Into<String>,
        listener: Arc<dyn EventListener>,
    ) -> Self {
        self.listeners
            .push(EventListenerDefinition::from_arc(pattern, listener));
        self
    }

    pub fn listeners<I>(mut self, listeners: I) -> Self
    where
        I: IntoIterator<Item = EventListenerDefinition>,
    {
        self.listeners.extend(listeners);
        self
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn named_event_bus(mut self, token: impl Into<String>) -> Self {
        self.event_bus_token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for EventModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::named_from_arc(self.token.as_str(), Arc::clone(&self.emitter)),
            ProviderDefinition::named_from_arc(
                self.event_bus_token.as_str(),
                self.emitter.event_bus(),
            ),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone(), self.event_bus_token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, module_ref: &ModuleRef) -> Result<()> {
        self.emitter.attach_module_ref(module_ref.clone())?;
        for listener in &self.listeners {
            self.emitter
                .on_arc(listener.pattern.clone(), Arc::clone(&listener.listener))?;
        }
        Ok(())
    }
}

fn validate_event_name(name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(BootError::Internal(format!(
            "event name must be non-empty and contain no whitespace: {name:?}"
        )));
    }
    Ok(name)
}

fn validate_event_pattern(pattern: String) -> Result<String> {
    let pattern = pattern.trim().to_string();
    if pattern == "*" {
        return Ok(pattern);
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        validate_event_name(prefix.to_string())?;
        return Ok(pattern);
    }
    validate_event_name(pattern)
}

fn a3s_event_from_name(name: &str, data: Value) -> A3sEvent {
    let category = name
        .split_once('.')
        .map(|(category, _)| category)
        .unwrap_or(name)
        .to_string();
    A3sEvent::typed(
        event_subject(name),
        category,
        name.to_string(),
        1,
        name.to_string(),
        "a3s-boot",
        data,
    )
}

fn event_subject(name: &str) -> String {
    format!("events.{name}")
}

fn event_pattern_subject_filter(pattern: &str) -> String {
    if pattern == "*" {
        return "events.>".to_string();
    }
    if let Some(prefix) = pattern.strip_suffix(".*") {
        return format!("events.{prefix}.>");
    }
    event_subject(pattern)
}

fn event_subject_matches(filter: &str, subject: &str) -> bool {
    a3s_event::subject::subject_matches(subject, filter)
}

fn event_error(error: a3s_event::EventError) -> BootError {
    BootError::Internal(format!("event bus error: {error}"))
}
