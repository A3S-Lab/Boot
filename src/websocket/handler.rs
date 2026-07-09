use super::connection::WebSocketGatewayConnection;
use super::message::{IntoWebSocketReply, WebSocketMessage};
use super::pipeline::{WebSocketGuard, WebSocketInterceptor, WebSocketPipe};
use super::server::WebSocketGatewayServer;
use crate::pipeline::{PipelineComponent, PipelineOverrides};
use crate::{
    catch_errors, validate_json_value_with_options, BootError, BootErrorKind, BoxFuture, Result,
    Validate, ValidationOptions, ValidationSchema, WebSocketExceptionFilter,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;

pub(crate) type WebSocketHandlerFuture = BoxFuture<'static, Result<Option<WebSocketMessage>>>;
type WebSocketMessageValidator =
    Arc<dyn Fn(WebSocketMessage, ValidationOptions) -> Result<WebSocketMessage> + Send + Sync>;

pub(crate) trait WebSocketMessageHandler: Send + Sync + 'static {
    fn call(
        &self,
        connection: WebSocketGatewayConnection,
        message: WebSocketMessage,
    ) -> WebSocketHandlerFuture;
}

pub(crate) struct WebSocketHandlerAdapter<H> {
    pub(crate) handler: H,
}

impl<H, Fut, R> WebSocketMessageHandler for WebSocketHandlerAdapter<H>
where
    H: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoWebSocketReply + Send + 'static,
{
    fn call(
        &self,
        _connection: WebSocketGatewayConnection,
        message: WebSocketMessage,
    ) -> WebSocketHandlerFuture {
        let future = (self.handler)(message);
        Box::pin(async move { Ok(future.await?.into_websocket_reply()) })
    }
}

pub(crate) struct WebSocketConnectionHandlerAdapter<H> {
    pub(crate) handler: H,
}

impl<H, Fut, R> WebSocketMessageHandler for WebSocketConnectionHandlerAdapter<H>
where
    H: Fn(WebSocketGatewayConnection, WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoWebSocketReply + Send + 'static,
{
    fn call(
        &self,
        connection: WebSocketGatewayConnection,
        message: WebSocketMessage,
    ) -> WebSocketHandlerFuture {
        let future = (self.handler)(connection, message);
        Box::pin(async move { Ok(future.await?.into_websocket_reply()) })
    }
}

pub(crate) struct WebSocketServerHandlerAdapter<H> {
    pub(crate) handler: H,
}

impl<H, Fut, R> WebSocketMessageHandler for WebSocketServerHandlerAdapter<H>
where
    H: Fn(WebSocketGatewayServer, WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoWebSocketReply + Send + 'static,
{
    fn call(
        &self,
        connection: WebSocketGatewayConnection,
        message: WebSocketMessage,
    ) -> WebSocketHandlerFuture {
        let future = (self.handler)(connection.server(), message);
        Box::pin(async move { Ok(future.await?.into_websocket_reply()) })
    }
}

/// Handler definition for one WebSocket subscription.
#[derive(Clone)]
pub struct WebSocketSubscriptionDefinition {
    pub(crate) handler: Arc<dyn WebSocketMessageHandler>,
    pub(crate) pipes: Vec<PipelineComponent<dyn WebSocketPipe>>,
    pub(crate) guards: Vec<PipelineComponent<dyn WebSocketGuard>>,
    pub(crate) interceptors: Vec<PipelineComponent<dyn WebSocketInterceptor>>,
    pub(crate) filters: Vec<PipelineComponent<dyn WebSocketExceptionFilter>>,
    pub(crate) validators: Vec<WebSocketMessageValidator>,
    pub(crate) validation_enabled: bool,
    pub(crate) validation_disabled: bool,
    pub(crate) validation_options: ValidationOptions,
    pub(crate) metadata: BTreeMap<String, Value>,
}

impl WebSocketSubscriptionDefinition {
    pub fn new<H, Fut, R>(handler: H) -> Self
    where
        H: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        Self {
            handler: Arc::new(WebSocketHandlerAdapter { handler }),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            validation_options: ValidationOptions::default(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn new_with_connection<H, Fut, R>(handler: H) -> Self
    where
        H: Fn(WebSocketGatewayConnection, WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        Self {
            handler: Arc::new(WebSocketConnectionHandlerAdapter { handler }),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            validation_options: ValidationOptions::default(),
            metadata: BTreeMap::new(),
        }
    }

    pub fn new_with_server<H, Fut, R>(handler: H) -> Self
    where
        H: Fn(WebSocketGatewayServer, WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        Self {
            handler: Arc::new(WebSocketServerHandlerAdapter { handler }),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            validation_options: ValidationOptions::default(),
            metadata: BTreeMap::new(),
        }
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
                "failed to serialize websocket subscription metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub(crate) fn with_metadata_defaults(mut self, metadata: &BTreeMap<String, Value>) -> Self {
        for (key, value) in metadata {
            self.metadata
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        self
    }

    pub(crate) fn with_metadata_default_value(
        mut self,
        key: impl Into<String>,
        value: Value,
    ) -> Self {
        self.metadata.entry(key.into()).or_insert(value);
        self
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: WebSocketPipe,
    {
        self.pipes
            .push(PipelineComponent::<dyn WebSocketPipe>::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: WebSocketGuard,
    {
        self.guards
            .push(PipelineComponent::<dyn WebSocketGuard>::new(guard));
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: WebSocketInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn WebSocketInterceptor>::new(
                interceptor,
            ));
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: WebSocketExceptionFilter,
    {
        self.filters
            .push(PipelineComponent::<dyn WebSocketExceptionFilter>::new(
                filter,
            ));
        self
    }

    pub fn with_catch_filter<I, F>(self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: WebSocketExceptionFilter,
    {
        self.with_filter(catch_errors(kinds, filter))
    }

    pub(crate) fn with_pipeline_overrides(mut self, overrides: &PipelineOverrides) -> Self {
        overrides.apply_to_websocket_pipes(&mut self.pipes);
        overrides.apply_to_websocket_guards(&mut self.guards);
        overrides.apply_to_websocket_interceptors(&mut self.interceptors);
        overrides.apply_to_websocket_filters(&mut self.filters);
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
                    "message data",
                )?;
                if options.transform || options.whitelist {
                    message.data = data;
                }
                Ok(message)
            }));
        self.with_validation()
    }
}
