use crate::{
    validate_value, BootError, BoxFuture, ExecutionContext, ExecutionInterceptor,
    ExecutionTransportKind, Guard, Result, Validate,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;

#[cfg(feature = "mqtt-transport")]
mod mqtt;
#[cfg(feature = "nats-transport")]
mod nats;
#[cfg(feature = "rabbitmq-transport")]
mod rabbitmq;
#[cfg(feature = "redis-transport")]
mod redis;
#[cfg(feature = "tcp-transport")]
mod tcp;

#[cfg(feature = "mqtt-transport")]
pub use self::mqtt::{MqttTransport, MqttTransportClient, MqttTransportOptions, MqttTransportQoS};
#[cfg(feature = "nats-transport")]
pub use self::nats::{NatsTransport, NatsTransportClient, NatsTransportOptions};
#[cfg(feature = "rabbitmq-transport")]
pub use self::rabbitmq::{RabbitMqTransport, RabbitMqTransportClient, RabbitMqTransportOptions};
#[cfg(feature = "redis-transport")]
pub use self::redis::{RedisTransport, RedisTransportClient, RedisTransportOptions};
#[cfg(feature = "tcp-transport")]
pub use tcp::{TcpTransport, TcpTransportClient, TcpTransportOptions};

/// Adapter-neutral microservice transport message.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportMessage {
    pub pattern: String,
    #[serde(default)]
    pub data: Value,
}

impl TransportMessage {
    pub fn new(pattern: impl Into<String>, data: impl Into<Value>) -> Self {
        Self {
            pattern: pattern.into(),
            data: data.into(),
        }
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn text(pattern: impl Into<String>, data: impl Into<String>) -> Self {
        Self::new(pattern, Value::String(data.into()))
    }

    pub fn json<T>(pattern: impl Into<String>, data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(Self::new(
            pattern,
            serde_json::to_value(data).map_err(|err| BootError::Internal(err.to_string()))?,
        ))
    }

    pub fn data_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.data.clone())
            .map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn validated_data<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        validate_value(self.data_as::<T>()?)
    }
}

/// Reply returned by request-response message pattern handlers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TransportReply {
    #[serde(default)]
    pub data: Value,
}

impl TransportReply {
    pub fn new(data: impl Into<Value>) -> Self {
        Self { data: data.into() }
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn text(data: impl Into<String>) -> Self {
        Self::new(Value::String(data.into()))
    }

    pub fn json<T>(data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(Self::new(
            serde_json::to_value(data).map_err(|err| BootError::Internal(err.to_string()))?,
        ))
    }

    pub fn data_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.data.clone())
            .map_err(|err| BootError::BadRequest(err.to_string()))
    }
}

/// Return value accepted by request-response message pattern handlers.
pub trait IntoTransportReply {
    fn into_transport_reply(self) -> Option<TransportReply>;
}

impl IntoTransportReply for TransportReply {
    fn into_transport_reply(self) -> Option<TransportReply> {
        Some(self)
    }
}

impl IntoTransportReply for Option<TransportReply> {
    fn into_transport_reply(self) -> Option<TransportReply> {
        self
    }
}

impl IntoTransportReply for Value {
    fn into_transport_reply(self) -> Option<TransportReply> {
        Some(TransportReply::new(self))
    }
}

impl IntoTransportReply for Option<Value> {
    fn into_transport_reply(self) -> Option<TransportReply> {
        self.map(TransportReply::new)
    }
}

impl IntoTransportReply for () {
    fn into_transport_reply(self) -> Option<TransportReply> {
        None
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessagePatternKind {
    RequestResponse,
    Event,
}

impl MessagePatternKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RequestResponse => "request-response",
            Self::Event => "event",
        }
    }
}

/// Context available to transport guards and interceptors.
#[derive(Debug, Clone)]
pub struct TransportContext {
    pub pattern: String,
    pub kind: MessagePatternKind,
    pub module_name: Option<String>,
    execution_context: ExecutionContext,
}

impl TransportContext {
    fn new(definition: &MessagePatternDefinition, pattern: &str) -> Self {
        let pattern = pattern.to_string();
        let kind = definition.kind;
        let module_name = definition.module_name.clone();
        let execution_context = ExecutionContext::transport(
            pattern.clone(),
            ExecutionTransportKind::from(kind),
            module_name.clone(),
        );
        Self {
            pattern,
            kind,
            module_name,
            execution_context,
        }
    }

    pub fn execution_context(&self) -> &ExecutionContext {
        &self.execution_context
    }

    pub fn into_execution_context(self) -> ExecutionContext {
        self.execution_context
    }
}

impl Deref for TransportContext {
    type Target = ExecutionContext;

    fn deref(&self) -> &Self::Target {
        self.execution_context()
    }
}

impl From<MessagePatternKind> for ExecutionTransportKind {
    fn from(kind: MessagePatternKind) -> Self {
        match kind {
            MessagePatternKind::RequestResponse => Self::RequestResponse,
            MessagePatternKind::Event => Self::Event,
        }
    }
}

/// Message transformation hook for transport message patterns.
pub trait TransportPipe: Send + Sync + 'static {
    fn transform(&self, message: TransportMessage) -> BoxFuture<'static, Result<TransportMessage>>;
}

impl<F, Fut> TransportPipe for F
where
    F: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<TransportMessage>> + Send + 'static,
{
    fn transform(&self, message: TransportMessage) -> BoxFuture<'static, Result<TransportMessage>> {
        Box::pin(self(message))
    }
}

/// Authorization hook for transport message patterns.
pub trait TransportGuard: Send + Sync + 'static {
    fn can_activate(&self, context: TransportContext) -> BoxFuture<'static, Result<bool>>;
}

impl<F, Fut> TransportGuard for F
where
    F: Fn(TransportContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<bool>> + Send + 'static,
{
    fn can_activate(&self, context: TransportContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(self(context))
    }
}

struct ExecutionTransportGuard<G> {
    inner: G,
}

impl<G> TransportGuard for ExecutionTransportGuard<G>
where
    G: Guard,
{
    fn can_activate(&self, context: TransportContext) -> BoxFuture<'static, Result<bool>> {
        self.inner.can_activate(context.into_execution_context())
    }
}

/// Around-handler hook for transport message patterns.
pub trait TransportInterceptor: Send + Sync + 'static {
    fn before(&self, _context: TransportContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn after(
        &self,
        _context: TransportContext,
        reply: Option<TransportReply>,
    ) -> BoxFuture<'static, Result<Option<TransportReply>>> {
        Box::pin(async move { Ok(reply) })
    }
}

struct ExecutionTransportInterceptor<I> {
    inner: I,
}

impl<I> TransportInterceptor for ExecutionTransportInterceptor<I>
where
    I: ExecutionInterceptor,
{
    fn before(&self, context: TransportContext) -> BoxFuture<'static, Result<()>> {
        self.inner.before(context.into_execution_context())
    }

    fn after(
        &self,
        context: TransportContext,
        reply: Option<TransportReply>,
    ) -> BoxFuture<'static, Result<Option<TransportReply>>> {
        let future = self.inner.after(context.into_execution_context());
        Box::pin(async move {
            future.await?;
            Ok(reply)
        })
    }
}

type TransportHandlerFuture = BoxFuture<'static, Result<Option<TransportReply>>>;
type MessageValidator = Arc<dyn Fn(&TransportMessage) -> Result<()> + Send + Sync>;

trait TransportMessageHandler: Send + Sync + 'static {
    fn call(&self, message: TransportMessage) -> TransportHandlerFuture;
}

struct TransportHandlerAdapter<H> {
    handler: H,
}

impl<H, Fut, R> TransportMessageHandler for TransportHandlerAdapter<H>
where
    H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoTransportReply + Send + 'static,
{
    fn call(&self, message: TransportMessage) -> TransportHandlerFuture {
        let future = (self.handler)(message);
        Box::pin(async move { Ok(future.await?.into_transport_reply()) })
    }
}

/// Framework-neutral message pattern handler definition.
#[derive(Clone)]
pub struct MessagePatternDefinition {
    pattern: String,
    kind: MessagePatternKind,
    handler: Arc<dyn TransportMessageHandler>,
    pipes: Vec<Arc<dyn TransportPipe>>,
    guards: Vec<Arc<dyn TransportGuard>>,
    interceptors: Vec<Arc<dyn TransportInterceptor>>,
    validators: Vec<MessageValidator>,
    module_name: Option<String>,
}

impl MessagePatternDefinition {
    pub fn request<H, Fut, R>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        Self::new(pattern, MessagePatternKind::RequestResponse, handler)
    }

    pub fn event<H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::new(pattern, MessagePatternKind::Event, handler)
    }

    pub fn request_json<T, H, Fut, R>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::request(pattern, move |message: TransportMessage| {
            let payload = message.data_as::<T>();
            let future = payload.map(&handler);
            async move {
                let response = future?.await?;
                TransportReply::json(&response)
            }
        })
    }

    pub fn request_validated_json<T, H, Fut, R>(
        pattern: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::request(pattern, move |message: TransportMessage| {
            let payload = message.validated_data::<T>();
            let future = payload.map(&handler);
            async move {
                let response = future?.await?;
                TransportReply::json(&response)
            }
        })
    }

    pub fn event_json<T, H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::event(pattern, move |message: TransportMessage| {
            let payload = message.data_as::<T>();
            let future = payload.map(&handler);
            async move { future?.await }
        })
    }

    pub fn event_validated_json<T, H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::event(pattern, move |message: TransportMessage| {
            let payload = message.validated_data::<T>();
            let future = payload.map(&handler);
            async move { future?.await }
        })
    }

    fn new<H, Fut, R>(
        pattern: impl Into<String>,
        kind: MessagePatternKind,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        let pattern = pattern.into();
        validate_pattern(&pattern)?;
        Ok(Self {
            pattern,
            kind,
            handler: Arc::new(TransportHandlerAdapter { handler }),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            validators: Vec::new(),
            module_name: None,
        })
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn kind(&self) -> MessagePatternKind {
        self.kind
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: TransportPipe,
    {
        self.pipes.push(Arc::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: TransportGuard,
    {
        self.guards.push(Arc::new(guard));
        self
    }

    pub fn with_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards
            .push(Arc::new(ExecutionTransportGuard { inner: guard }));
        self
    }

    pub(crate) fn with_execution_pipeline_prefix(
        mut self,
        guards: &[Arc<dyn Guard>],
        interceptors: &[Arc<dyn ExecutionInterceptor>],
    ) -> Self {
        self.guards = prepend_execution_guards(guards, self.guards);
        self.interceptors = prepend_execution_interceptors(interceptors, self.interceptors);
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.interceptors
            .push(Arc::new(ExecutionTransportInterceptor {
                inner: interceptor,
            }));
        self
    }

    pub fn with_payload_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(Arc::new(|message| {
            message.validated_data::<T>().map(|_| ())
        }));
        self
    }

    pub async fn dispatch(&self, mut message: TransportMessage) -> Result<Option<TransportReply>> {
        if message.pattern != self.pattern {
            return Err(BootError::NotFound(format!(
                "message pattern {}",
                message.pattern
            )));
        }

        let context = TransportContext::new(self, &message.pattern);
        for guard in &self.guards {
            let can_activate = guard.can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "message pattern {}",
                    message.pattern
                )));
            }
        }

        for interceptor in &self.interceptors {
            interceptor.before(context.clone()).await?;
        }

        for pipe in &self.pipes {
            message = pipe.transform(message).await?;
        }

        for validator in &self.validators {
            validator(&message)?;
        }

        let mut reply = self.handler.call(message).await?;
        if self.kind == MessagePatternKind::Event {
            reply = None;
        }

        for interceptor in self.interceptors.iter().rev() {
            reply = interceptor.after(context.clone(), reply).await?;
        }
        Ok(reply)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }
}

fn prepend_execution_guards(
    prefix: &[Arc<dyn Guard>],
    values: Vec<Arc<dyn TransportGuard>>,
) -> Vec<Arc<dyn TransportGuard>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|guard| Arc::new(ExecutionTransportGuard { inner: guard }) as Arc<dyn TransportGuard>)
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

fn prepend_execution_interceptors(
    prefix: &[Arc<dyn ExecutionInterceptor>],
    values: Vec<Arc<dyn TransportInterceptor>>,
) -> Vec<Arc<dyn TransportInterceptor>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|interceptor| {
            Arc::new(ExecutionTransportInterceptor { inner: interceptor })
                as Arc<dyn TransportInterceptor>
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

/// Adapter trait for message transports such as in-process, Redis, NATS, or Kafka.
pub trait MessageTransport {
    type Output;

    fn build(&self, app: crate::BootApplication) -> Result<Self::Output>;

    fn serve(&self, app: crate::BootApplication) -> BoxFuture<'static, Result<()>>;
}

/// In-process transport useful for tests and single-process message dispatch.
#[derive(Debug, Clone, Copy, Default)]
pub struct InProcessTransport;

impl InProcessTransport {
    pub fn new() -> Self {
        Self
    }
}

impl MessageTransport for InProcessTransport {
    type Output = InProcessTransportClient;

    fn build(&self, app: crate::BootApplication) -> Result<Self::Output> {
        Ok(InProcessTransportClient { app })
    }

    fn serve(&self, _app: crate::BootApplication) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// In-process message client backed by a resolved Boot application.
#[derive(Clone)]
pub struct InProcessTransportClient {
    app: crate::BootApplication,
}

impl InProcessTransportClient {
    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        self.app.dispatch_message(message).await
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        self.app.emit_message(message).await
    }
}

fn validate_pattern(pattern: &str) -> Result<()> {
    if pattern.trim().is_empty() {
        return Err(BootError::BadRequest(
            "message pattern cannot be empty".to_string(),
        ));
    }
    Ok(())
}
