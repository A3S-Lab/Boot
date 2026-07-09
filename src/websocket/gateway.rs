use super::connection::WebSocketGatewayConnection;
use super::context::WebSocketGatewayInitContext;
use super::handler::{WebSocketHandlerAdapter, WebSocketMessageHandler};
use super::hooks::{
    WebSocketGatewayConnectionHook, WebSocketGatewayDisconnectHook, WebSocketGatewayInitHook,
};
use super::message::{send_to_outbounds, IntoWebSocketReply, WebSocketMessage, WebSocketOutbound};
use super::pipeline::{
    prepend_execution_guards, prepend_execution_interceptors, ExecutionWebSocketGuard,
    ExecutionWebSocketInterceptor, WebSocketGuard, WebSocketInterceptor, WebSocketPipe,
};
use super::state::{normalize_namespace, normalize_room, WebSocketGatewayState};
use crate::routing::path::{
    join_paths, match_path_params, match_path_shape, route_shape_key, validate_route_path,
};
use crate::{BootError, BootRequest, ExecutionInterceptor, Guard, HttpMethod, Result};
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

/// Framework-neutral WebSocket gateway definition.
#[derive(Clone)]
pub struct WebSocketGatewayDefinition {
    pub(crate) path: String,
    pub(crate) namespace: Option<String>,
    pub(crate) handlers: BTreeMap<String, Arc<dyn WebSocketMessageHandler>>,
    pub(crate) init_hooks: Vec<Arc<dyn WebSocketGatewayInitHook>>,
    pub(crate) connection_hooks: Vec<Arc<dyn WebSocketGatewayConnectionHook>>,
    pub(crate) disconnect_hooks: Vec<Arc<dyn WebSocketGatewayDisconnectHook>>,
    pub(crate) pipes: Vec<Arc<dyn WebSocketPipe>>,
    pub(crate) guards: Vec<Arc<dyn WebSocketGuard>>,
    pub(crate) interceptors: Vec<Arc<dyn WebSocketInterceptor>>,
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

    pub fn with_namespace(mut self, namespace: impl Into<String>) -> Result<Self> {
        self.namespace = Some(normalize_namespace(namespace)?);
        Ok(self)
    }

    pub fn events(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
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
        self.handlers
            .insert(event, Arc::new(WebSocketHandlerAdapter { handler }));
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
        self.pipes.push(Arc::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: WebSocketGuard,
    {
        self.guards.push(Arc::new(guard));
        self
    }

    pub fn with_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards
            .push(Arc::new(ExecutionWebSocketGuard { inner: guard }));
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
        I: WebSocketInterceptor,
    {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.interceptors
            .push(Arc::new(ExecutionWebSocketInterceptor {
                inner: interceptor,
            }));
        self
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
