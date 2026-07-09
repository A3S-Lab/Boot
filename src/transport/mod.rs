use crate::pipeline::{PipelineComponent, PipelineOverrides};
use crate::{
    catch_errors, validate_json_value_with_options, validate_value, BootError, BootErrorKind,
    BoxFuture, ExecutionContext, ExecutionInterceptor, ExecutionTransportKind, Guard, Result,
    TransportExceptionFilter, Validate, ValidationOptions, ValidationSchema,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;

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
    fn new(definition: &MessagePatternDefinition, pattern: &str) -> Self {
        let pattern = pattern.to_string();
        let kind = definition.kind;
        let module_name = definition.module_name.clone();
        let metadata = definition.metadata.clone();
        let execution_context = ExecutionContext::transport(
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
type MessageValidator =
    Arc<dyn Fn(TransportMessage, ValidationOptions) -> Result<TransportMessage> + Send + Sync>;

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
    pipes: Vec<PipelineComponent<dyn TransportPipe>>,
    guards: Vec<PipelineComponent<dyn TransportGuard>>,
    interceptors: Vec<PipelineComponent<dyn TransportInterceptor>>,
    filters: Vec<PipelineComponent<dyn TransportExceptionFilter>>,
    validators: Vec<MessageValidator>,
    validation_enabled: bool,
    validation_disabled: bool,
    validation_options: ValidationOptions,
    metadata: BTreeMap<String, Value>,
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
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            validation_options: ValidationOptions::default(),
            metadata: BTreeMap::new(),
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

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn with_metadata<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize message pattern metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: TransportPipe,
    {
        self.pipes
            .push(PipelineComponent::<dyn TransportPipe>::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: TransportGuard,
    {
        self.guards
            .push(PipelineComponent::<dyn TransportGuard>::new(guard));
        self
    }

    pub fn with_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards
            .push(PipelineComponent::<dyn TransportGuard>::new(
                ExecutionTransportGuard { inner: guard },
            ));
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

    pub(crate) fn with_guard_prefix(mut self, guards: &[Arc<dyn TransportGuard>]) -> Self {
        let mut merged = guards
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportGuard>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.guards);
        self.guards = merged;
        self
    }

    pub(crate) fn with_interceptor_prefix(
        mut self,
        interceptors: &[Arc<dyn TransportInterceptor>],
    ) -> Self {
        let mut merged = interceptors
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportInterceptor>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.interceptors);
        self.interceptors = merged;
        self
    }

    pub(crate) fn with_pipe_prefix(mut self, pipes: &[Arc<dyn TransportPipe>]) -> Self {
        let mut merged = pipes
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportPipe>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.pipes);
        self.pipes = merged;
        self
    }

    pub(crate) fn with_filter_prefix(
        mut self,
        filters: &[Arc<dyn TransportExceptionFilter>],
    ) -> Self {
        let mut merged = filters
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportExceptionFilter>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.filters);
        self.filters = merged;
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn TransportInterceptor>::new(
                interceptor,
            ));
        self
    }

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn TransportInterceptor>::new(
                ExecutionTransportInterceptor { inner: interceptor },
            ));
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: TransportExceptionFilter,
    {
        self.filters
            .push(PipelineComponent::<dyn TransportExceptionFilter>::new(
                filter,
            ));
        self
    }

    pub fn with_catch_filter<I, F>(self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: TransportExceptionFilter,
    {
        self.with_filter(catch_errors(kinds, filter))
    }

    pub(crate) fn with_pipeline_overrides(mut self, overrides: &PipelineOverrides) -> Self {
        overrides.apply_to_transport_pipes(&mut self.pipes);
        overrides.apply_to_transport_guards(&mut self.guards);
        overrides.apply_to_transport_interceptors(&mut self.interceptors);
        overrides.apply_to_transport_filters(&mut self.filters);
        self
    }

    pub fn with_validation(mut self) -> Self {
        self.validation_enabled = true;
        self.validation_disabled = false;
        self
    }

    pub fn with_validation_options(mut self, options: ValidationOptions) -> Self {
        self.validation_enabled = true;
        self.validation_disabled = false;
        self.validation_options = self.validation_options.merge(options);
        self
    }

    pub fn without_validation(mut self) -> Self {
        self.validation_enabled = false;
        self.validation_disabled = true;
        self
    }

    pub(crate) fn with_validation_prefix(
        mut self,
        validation_enabled: bool,
        validation_options: ValidationOptions,
    ) -> Self {
        if !self.validation_disabled {
            self.validation_enabled = validation_enabled || self.validation_enabled;
            self.validation_options = validation_options.merge(self.validation_options);
        }
        self
    }

    pub fn with_payload_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(Arc::new(|message, _| {
            message.validated_data::<T>().map(|_| message)
        }));
        self.with_validation()
    }

    pub fn with_payload_validation_options<T>(mut self, options: ValidationOptions) -> Self
    where
        T: DeserializeOwned + Serialize + Validate + ValidationSchema + 'static,
    {
        self.validators
            .push(Arc::new(move |mut message, inherited_options| {
                let options = inherited_options.merge(options);
                let data = validate_json_value_with_options::<T>(
                    message.data.clone(),
                    options,
                    "message property",
                )?;
                if options.transform || options.whitelist {
                    message.data = data;
                }
                Ok(message)
            }));
        self.with_validation()
    }

    pub async fn dispatch(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let context = TransportContext::new(self, &message.pattern);
        match self.dispatch_pipeline(message, context.clone()).await {
            Ok(reply) => Ok(reply),
            Err(error) => self.handle_error(context, error).await,
        }
    }

    async fn dispatch_pipeline(
        &self,
        mut message: TransportMessage,
        context: TransportContext,
    ) -> Result<Option<TransportReply>> {
        if message.pattern != self.pattern {
            return Err(BootError::NotFound(format!(
                "message pattern {}",
                message.pattern
            )));
        }

        for guard in &self.guards {
            let can_activate = guard.inner().can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "message pattern {}",
                    message.pattern
                )));
            }
        }

        for interceptor in &self.interceptors {
            interceptor.inner().before(context.clone()).await?;
        }

        for pipe in &self.pipes {
            message = pipe.inner().transform(message).await?;
        }

        if self.validation_enabled {
            for validator in &self.validators {
                message = validator(message, self.validation_options)?;
            }
        }

        let mut reply = self.handler.call(message).await?;
        if self.kind == MessagePatternKind::Event {
            reply = None;
        }

        for interceptor in self.interceptors.iter().rev() {
            reply = interceptor.inner().after(context.clone(), reply).await?;
        }
        Ok(reply)
    }

    async fn handle_error(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> Result<Option<TransportReply>> {
        for filter in self.filters.iter().rev() {
            if let Some(response) = filter
                .inner()
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(if self.kind == MessagePatternKind::Event {
                    None
                } else {
                    response.into_reply()
                });
            }
        }
        Err(error)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }
}

fn prepend_execution_guards(
    prefix: &[Arc<dyn Guard>],
    values: Vec<PipelineComponent<dyn TransportGuard>>,
) -> Vec<PipelineComponent<dyn TransportGuard>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|guard| {
            PipelineComponent::<dyn TransportGuard>::new(ExecutionTransportGuard { inner: guard })
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

fn prepend_execution_interceptors(
    prefix: &[Arc<dyn ExecutionInterceptor>],
    values: Vec<PipelineComponent<dyn TransportInterceptor>>,
) -> Vec<PipelineComponent<dyn TransportInterceptor>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|interceptor| {
            PipelineComponent::<dyn TransportInterceptor>::new(ExecutionTransportInterceptor {
                inner: interceptor,
            })
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
