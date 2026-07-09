use super::connection::WebSocketGatewayConnection;
use super::context::WebSocketGatewayInitContext;
use super::handler::WebSocketSubscriptionDefinition;
use super::hooks::{
    WebSocketGatewayConnectionHook, WebSocketGatewayDisconnectHook, WebSocketGatewayInitHook,
};
use super::message::{send_to_outbounds, IntoWebSocketReply, WebSocketMessage, WebSocketOutbound};
use super::pipeline::{
    prepend_execution_guards, prepend_execution_interceptors, ExecutionWebSocketGuard,
    ExecutionWebSocketInterceptor, WebSocketGuard, WebSocketInterceptor, WebSocketPipe,
};
use super::server::WebSocketGatewayServer;
use super::state::{normalize_namespace, normalize_room, WebSocketGatewayState};
use crate::pipeline::{PipelineComponent, PipelineOverrides};
use crate::routing::path::{
    join_paths, match_path_params, match_path_shape, route_shape_key, validate_route_path,
};
use crate::{
    catch_errors, BootError, BootErrorKind, BootRequest, ExecutionInterceptor, Guard, HttpMethod,
    Result, ValidationOptions, WebSocketExceptionFilter,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Framework-neutral WebSocket gateway definition.
#[derive(Clone)]
pub struct WebSocketGatewayDefinition {
    pub(crate) path: String,
    pub(crate) namespace: Option<String>,
    pub(crate) handlers: BTreeMap<String, WebSocketSubscriptionDefinition>,
    pub(crate) init_hooks: Vec<Arc<dyn WebSocketGatewayInitHook>>,
    pub(crate) connection_hooks: Vec<Arc<dyn WebSocketGatewayConnectionHook>>,
    pub(crate) disconnect_hooks: Vec<Arc<dyn WebSocketGatewayDisconnectHook>>,
    pub(crate) pipes: Vec<PipelineComponent<dyn WebSocketPipe>>,
    pub(crate) guards: Vec<PipelineComponent<dyn WebSocketGuard>>,
    pub(crate) interceptors: Vec<PipelineComponent<dyn WebSocketInterceptor>>,
    pub(crate) filters: Vec<PipelineComponent<dyn WebSocketExceptionFilter>>,
    pub(crate) metadata: BTreeMap<String, Value>,
    pub(crate) module_name: Option<String>,
    pub(crate) state: Arc<WebSocketGatewayState>,
}

impl WebSocketGatewayDefinition {
    pub fn new(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        validate_route_path(&path)?;
        Ok(Self {
            path,
            namespace: None,
            handlers: BTreeMap::new(),
            init_hooks: Vec::new(),
            connection_hooks: Vec::new(),
            disconnect_hooks: Vec::new(),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            metadata: BTreeMap::new(),
            module_name: None,
            state: Arc::new(WebSocketGatewayState::default()),
        })
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn path_shape(&self) -> String {
        route_shape_key(&self.path)
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn namespace(&self) -> Option<&str> {
        self.namespace.as_deref()
    }

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn event_metadata(&self, event: &str) -> Option<&BTreeMap<String, Value>> {
        self.handlers
            .get(event)
            .map(WebSocketSubscriptionDefinition::metadata)
    }

    pub fn with_metadata<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize websocket gateway metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        let key = key.into();
        self.metadata.insert(key.clone(), value.clone());
        self.handlers = self
            .handlers
            .into_iter()
            .map(|(event, handler)| {
                (
                    event,
                    handler.with_metadata_default_value(key.clone(), value.clone()),
                )
            })
            .collect();
        self
    }

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Result<Self> {
        self.namespace = Some(normalize_namespace(namespace)?);
        Ok(self)
    }

    pub fn events(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
    }

    pub fn server(&self) -> WebSocketGatewayServer {
        WebSocketGatewayServer::new(self.clone())
    }

    pub fn active_connection_count(&self) -> Result<usize> {
        self.state.connection_count()
    }

    pub fn active_connection_ids(&self) -> Result<Vec<u64>> {
        self.state.connection_ids()
    }

    pub fn rooms(&self) -> Result<Vec<String>> {
        self.state.rooms()
    }

    pub fn room_members(&self, room: impl Into<String>) -> Result<Vec<u64>> {
        self.state.room_members(room)
    }

    pub async fn broadcast(&self, message: WebSocketMessage) -> Result<usize> {
        let outbounds = self.state.broadcast_targets(None, None)?;
        send_to_outbounds(outbounds, message).await
    }

    pub async fn broadcast_to_room(
        &self,
        room: impl Into<String>,
        message: WebSocketMessage,
    ) -> Result<usize> {
        let room = normalize_room(room)?;
        let outbounds = self.state.broadcast_targets(Some(&room), None)?;
        send_to_outbounds(outbounds, message).await
    }

    /// Run gateway initialization hooks.
    pub async fn after_init(&self) -> Result<()> {
        let context = WebSocketGatewayInitContext::new(self);
        for hook in &self.init_hooks {
            hook.after_init(context.clone()).await?;
        }
        Ok(())
    }

    pub fn matches_path(&self, path: &str) -> bool {
        match_path_shape(&self.path, path)
    }

    pub fn path_params(&self, path: &str) -> Result<Option<BTreeMap<String, String>>> {
        match_path_params(&self.path, path)
    }

    pub fn subscribe<H, Fut, R>(mut self, event: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        let event = event.into();
        if event.trim().is_empty() {
            return Err(BootError::BadRequest(
                "websocket event cannot be empty".to_string(),
            ));
        }
        if self.handlers.contains_key(&event) {
            return Err(BootError::DuplicateRoute(format!(
                "{} {}",
                self.path, event
            )));
        }
        self.handlers.insert(
            event,
            WebSocketSubscriptionDefinition::new(handler).with_metadata_defaults(&self.metadata),
        );
        Ok(self)
    }

    pub fn subscribe_with_connection<H, Fut, R>(
        mut self,
        event: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(WebSocketGatewayConnection, WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        let event = event.into();
        if event.trim().is_empty() {
            return Err(BootError::BadRequest(
                "websocket event cannot be empty".to_string(),
            ));
        }
        if self.handlers.contains_key(&event) {
            return Err(BootError::DuplicateRoute(format!(
                "{} {}",
                self.path, event
            )));
        }
        self.handlers.insert(
            event,
            WebSocketSubscriptionDefinition::new_with_connection(handler)
                .with_metadata_defaults(&self.metadata),
        );
        Ok(self)
    }

    pub fn subscribe_with_server<H, Fut, R>(
        mut self,
        event: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(WebSocketGatewayServer, WebSocketMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoWebSocketReply + Send + 'static,
    {
        let event = event.into();
        if event.trim().is_empty() {
            return Err(BootError::BadRequest(
                "websocket event cannot be empty".to_string(),
            ));
        }
        if self.handlers.contains_key(&event) {
            return Err(BootError::DuplicateRoute(format!(
                "{} {}",
                self.path, event
            )));
        }
        self.handlers.insert(
            event,
            WebSocketSubscriptionDefinition::new_with_server(handler)
                .with_metadata_defaults(&self.metadata),
        );
        Ok(self)
    }

    pub fn subscribe_definition(
        mut self,
        event: impl Into<String>,
        subscription: WebSocketSubscriptionDefinition,
    ) -> Result<Self> {
        let event = event.into();
        if event.trim().is_empty() {
            return Err(BootError::BadRequest(
                "websocket event cannot be empty".to_string(),
            ));
        }
        if self.handlers.contains_key(&event) {
            return Err(BootError::DuplicateRoute(format!(
                "{} {}",
                self.path, event
            )));
        }
        self.handlers
            .insert(event, subscription.with_metadata_defaults(&self.metadata));
        Ok(self)
    }

    pub fn with_after_init<H>(mut self, hook: H) -> Self
    where
        H: WebSocketGatewayInitHook,
    {
        self.init_hooks.push(Arc::new(hook));
        self
    }

    pub fn with_connection_hook<H>(mut self, hook: H) -> Self
    where
        H: WebSocketGatewayConnectionHook,
    {
        self.connection_hooks.push(Arc::new(hook));
        self
    }

    pub fn with_disconnect_hook<H>(mut self, hook: H) -> Self
    where
        H: WebSocketGatewayDisconnectHook,
    {
        self.disconnect_hooks.push(Arc::new(hook));
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

    pub fn with_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards
            .push(PipelineComponent::<dyn WebSocketGuard>::new(
                ExecutionWebSocketGuard { inner: guard },
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

    pub(crate) fn with_guard_prefix(mut self, guards: &[Arc<dyn WebSocketGuard>]) -> Self {
        let mut merged = guards
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn WebSocketGuard>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.guards);
        self.guards = merged;
        self
    }

    pub(crate) fn with_interceptor_prefix(
        mut self,
        interceptors: &[Arc<dyn WebSocketInterceptor>],
    ) -> Self {
        let mut merged = interceptors
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn WebSocketInterceptor>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.interceptors);
        self.interceptors = merged;
        self
    }

    pub(crate) fn with_pipe_prefix(mut self, pipes: &[Arc<dyn WebSocketPipe>]) -> Self {
        let mut merged = pipes
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn WebSocketPipe>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.pipes);
        self.pipes = merged;
        self
    }

    pub(crate) fn with_filter_prefix(
        mut self,
        filters: &[Arc<dyn WebSocketExceptionFilter>],
    ) -> Self {
        let mut merged = filters
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn WebSocketExceptionFilter>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.filters);
        self.filters = merged;
        self
    }

    pub(crate) fn with_pipeline_overrides(mut self, overrides: &PipelineOverrides) -> Self {
        overrides.apply_to_websocket_pipes(&mut self.pipes);
        overrides.apply_to_websocket_guards(&mut self.guards);
        overrides.apply_to_websocket_interceptors(&mut self.interceptors);
        overrides.apply_to_websocket_filters(&mut self.filters);
        self.handlers = self
            .handlers
            .into_iter()
            .map(|(event, subscription)| (event, subscription.with_pipeline_overrides(overrides)))
            .collect();
        self
    }

    pub(crate) fn with_validation_prefix(
        mut self,
        validation_enabled: bool,
        validation_options: ValidationOptions,
    ) -> Self {
        self.handlers = self
            .handlers
            .into_iter()
            .map(|(event, subscription)| {
                (
                    event,
                    subscription.with_validation_prefix(validation_enabled, validation_options),
                )
            })
            .collect();
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

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn WebSocketInterceptor>::new(
                ExecutionWebSocketInterceptor { inner: interceptor },
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

    pub fn connect(&self, request: BootRequest) -> Result<WebSocketGatewayConnection> {
        if request.method() != HttpMethod::Get {
            return Err(BootError::MethodNotAllowed(format!(
                "{} {}",
                request.method().as_str(),
                request.path()
            )));
        }
        let Some(params) = self.path_params(request.path())? else {
            return Err(BootError::NotFound(format!(
                "{} {}",
                request.method().as_str(),
                request.path()
            )));
        };
        Ok(WebSocketGatewayConnection {
            gateway: self.clone(),
            id: self.state.next_connection_id(),
            request: request.with_path_params(params),
            outbound: None,
            opened: Arc::new(AtomicBool::new(false)),
        })
    }

    pub fn connect_with_outbound<O>(
        &self,
        request: BootRequest,
        outbound: O,
    ) -> Result<WebSocketGatewayConnection>
    where
        O: WebSocketOutbound,
    {
        let mut connection = self.connect(request)?;
        connection.outbound = Some(Arc::new(outbound));
        Ok(connection)
    }

    pub async fn connect_async(&self, request: BootRequest) -> Result<WebSocketGatewayConnection> {
        let connection = self.connect(request)?;
        connection.open().await?;
        Ok(connection)
    }

    pub async fn connect_async_with_outbound<O>(
        &self,
        request: BootRequest,
        outbound: O,
    ) -> Result<WebSocketGatewayConnection>
    where
        O: WebSocketOutbound,
    {
        let connection = self.connect_with_outbound(request, outbound)?;
        connection.open().await?;
        Ok(connection)
    }

    pub async fn emit_to_connection(
        &self,
        connection_id: u64,
        message: WebSocketMessage,
    ) -> Result<bool> {
        let outbounds = self
            .state
            .outbound_for_connection(connection_id)?
            .into_iter()
            .collect();
        Ok(send_to_outbounds(outbounds, message).await? > 0)
    }

    pub async fn dispatch(
        &self,
        request: BootRequest,
        message: WebSocketMessage,
    ) -> Result<Option<WebSocketMessage>> {
        self.connect(request)?.dispatch(message).await
    }

    pub(crate) fn with_path_prefix(mut self, prefix: &str) -> Result<Self> {
        self.path = join_paths(prefix, &self.path)?;
        Ok(self)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }
}
