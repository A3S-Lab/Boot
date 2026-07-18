use crate::{
    validate_value, BootError, BootRequest, BoxFuture, CallHandler, ExecutionContext,
    ExecutionInterceptor, ExecutionTransportKind, Guard, Result, Validate,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::ops::Deref;

mod pattern;

pub use self::pattern::MessagePatternDefinition;

#[cfg(feature = "grpc-transport")]
mod grpc;
#[cfg(feature = "kafka-transport")]
mod kafka;
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

#[cfg(feature = "grpc-transport")]
pub use self::grpc::{GrpcTransport, GrpcTransportClient, GrpcTransportOptions};
#[cfg(feature = "kafka-transport")]
pub use self::kafka::{KafkaTransport, KafkaTransportClient, KafkaTransportOptions};
#[cfg(feature = "mqtt-transport")]
pub use self::mqtt::{MqttTransport, MqttTransportClient, MqttTransportOptions, MqttTransportQoS};
#[cfg(feature = "nats-transport")]
pub use self::nats::{NatsTransport, NatsTransportClient, NatsTransportOptions};
#[cfg(feature = "rabbitmq-transport")]
pub use self::rabbitmq::{RabbitMqTransport, RabbitMqTransportClient, RabbitMqTransportOptions};
#[cfg(feature = "redis-transport")]
pub use self::redis::{RedisTransport, RedisTransportClient, RedisTransportOptions};
#[cfg(any(
    feature = "grpc-transport",
    feature = "kafka-transport",
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "rabbitmq-transport",
    feature = "redis-transport",
    feature = "tcp-transport"
))]
pub(super) fn transport_error_from_status(status: u16, message: String) -> BootError {
    BootError::from_http_status(status, message)
}

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

    pub fn data_field(&self, name: &str) -> Result<Option<Value>> {
        let Value::Object(fields) = &self.data else {
            return Err(BootError::BadRequest(
                "expected JSON object transport data".to_string(),
            ));
        };

        Ok(fields.get(name).filter(|value| !value.is_null()).cloned())
    }

    pub fn data_field_as<T>(&self, name: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.data_field(name)? else {
            return Err(BootError::BadRequest(format!(
                "missing transport data field: {name}"
            )));
        };
        deserialize_data_field("transport data field", name, value)
    }

    pub fn optional_data_field_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.data_field(name)?
            .map(|value| deserialize_data_field("transport data field", name, value))
            .transpose()
    }

    pub fn data_field_string(&self, name: &str) -> Result<String> {
        let Some(value) = self.data_field(name)? else {
            return Err(BootError::BadRequest(format!(
                "missing transport data field: {name}"
            )));
        };
        data_field_value_to_string(value)
    }

    pub fn optional_data_field_string(&self, name: &str) -> Result<Option<String>> {
        self.data_field(name)?
            .map(data_field_value_to_string)
            .transpose()
    }

    pub fn validated_data<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        validate_value(self.data_as::<T>()?)
    }
}

fn deserialize_data_field<T>(label: &str, name: &str, value: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value)
        .map_err(|error| BootError::BadRequest(format!("invalid {label} {name}: {error}")))
}

fn data_field_value_to_string(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(&value).map_err(|error| BootError::BadRequest(error.to_string()))
        }
        Value::Null => Ok("null".to_string()),
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
    fn new(definition: &MessagePatternDefinition, pattern: &str, request: BootRequest) -> Self {
        let pattern = pattern.to_string();
        let kind = definition.kind();
        let module_name = definition.module_name().map(str::to_string);
        let metadata = definition.metadata().clone();
        let execution_context = ExecutionContext::transport(
            request,
            pattern.clone(),
            ExecutionTransportKind::from(kind),
            module_name.clone(),
            metadata,
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
    /// Run around the remaining transport pipeline.
    ///
    /// Override this method to recover downstream errors, retry the remaining
    /// pipeline, or return a reply without calling `next`. The default
    /// implementation preserves the legacy `before` and `after` hook behavior.
    fn intercept<'a>(
        &'a self,
        context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            self.before(context.clone()).await?;
            let reply = next.handle().await?;
            self.after(context, reply).await
        })
    }

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
