use crate::routing::path::{
    join_paths, match_path_params, match_path_shape, route_shape_key, validate_route_path,
};
use crate::{
    BootError, BootRequest, BoxFuture, ExecutionContext, ExecutionInterceptor, Guard, HttpMethod,
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::ops::Deref;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// Adapter-neutral WebSocket message used by gateways and adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

impl WebSocketMessage {
    pub fn new(event: impl Into<String>, data: impl Into<Value>) -> Self {
        Self {
            event: event.into(),
            data: data.into(),
        }
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn text(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self::new(event, Value::String(data.into()))
    }

    pub fn json<T>(event: impl Into<String>, data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(Self::new(
            event,
            serde_json::to_value(data).map_err(|err| BootError::Internal(err.to_string()))?,
        ))
    }
}

/// Return value accepted by WebSocket gateway handlers.
pub trait IntoWebSocketReply {
    fn into_websocket_reply(self) -> Option<WebSocketMessage>;
}

impl IntoWebSocketReply for WebSocketMessage {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        Some(self)
    }
}

impl IntoWebSocketReply for Option<WebSocketMessage> {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        self
    }
}

impl IntoWebSocketReply for () {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        None
    }
}

/// Outbound writer for adapter-backed WebSocket connections.
pub trait WebSocketOutbound: Send + Sync + 'static {
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketOutbound for F
where
    F: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(message))
    }
}

impl WebSocketOutbound for Arc<dyn WebSocketOutbound> {
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>> {
        self.as_ref().send(message)
    }
}

#[derive(Default)]
struct WebSocketGatewayState {
    next_connection_id: AtomicU64,
    connections: Mutex<BTreeMap<u64, WebSocketConnectionState>>,
}

struct WebSocketConnectionState {
    rooms: BTreeSet<String>,
    outbound: Option<Arc<dyn WebSocketOutbound>>,
}

impl WebSocketGatewayState {
    fn next_connection_id(&self) -> u64 {
        self.next_connection_id.fetch_add(1, Ordering::Relaxed) + 1
    }

    fn register(&self, id: u64, outbound: Option<Arc<dyn WebSocketOutbound>>) -> Result<()> {
        self.connections()?.insert(
            id,
            WebSocketConnectionState {
                rooms: BTreeSet::new(),
                outbound,
            },
        );
        Ok(())
    }

    fn unregister(&self, id: u64) -> Result<()> {
        self.connections()?.remove(&id);
        Ok(())
    }

    fn join(&self, id: u64, room: impl Into<String>) -> Result<()> {
        let room = normalize_room(room)?;
        let mut connections = self.connections()?;
        let connection = connections.get_mut(&id).ok_or_else(|| {
            BootError::BadRequest(format!("websocket connection {id} is not open"))
        })?;
        connection.rooms.insert(room);
        Ok(())
    }

    fn leave(&self, id: u64, room: impl Into<String>) -> Result<()> {
        let room = normalize_room(room)?;
        let mut connections = self.connections()?;
        let Some(connection) = connections.get_mut(&id) else {
            return Ok(());
        };
        connection.rooms.remove(&room);
        Ok(())
    }

    fn connection_count(&self) -> Result<usize> {
        Ok(self.connections()?.len())
    }

    fn connection_ids(&self) -> Result<Vec<u64>> {
        Ok(self.connections()?.keys().copied().collect())
    }

    fn rooms(&self) -> Result<Vec<String>> {
        let mut rooms = BTreeSet::new();
        for connection in self.connections()?.values() {
            rooms.extend(connection.rooms.iter().cloned());
        }
        Ok(rooms.into_iter().collect())
    }

    fn rooms_for_connection(&self, id: u64) -> Result<Vec<String>> {
        Ok(self
            .connections()?
            .get(&id)
            .map(|connection| connection.rooms.iter().cloned().collect())
            .unwrap_or_default())
    }

    fn room_members(&self, room: impl Into<String>) -> Result<Vec<u64>> {
        let room = normalize_room(room)?;
        Ok(self
            .connections()?
            .iter()
            .filter_map(|(id, connection)| connection.rooms.contains(&room).then_some(*id))
            .collect())
    }

    fn outbound_for_connection(&self, id: u64) -> Result<Option<Arc<dyn WebSocketOutbound>>> {
        Ok(self
            .connections()?
            .get(&id)
            .and_then(|connection| connection.outbound.clone()))
    }

    fn broadcast_targets(
        &self,
        room: Option<&str>,
        exclude_connection_id: Option<u64>,
    ) -> Result<Vec<Arc<dyn WebSocketOutbound>>> {
        let connections = self.connections()?;
        Ok(connections
            .iter()
            .filter(|(id, _)| Some(**id) != exclude_connection_id)
            .filter(|(_, connection)| match room {
                Some(room) => connection.rooms.contains(room),
                None => true,
            })
            .filter_map(|(_, connection)| connection.outbound.clone())
            .collect())
    }

    fn connections(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<u64, WebSocketConnectionState>>> {
        self.connections.lock().map_err(|_| {
            BootError::Internal("websocket gateway state lock is poisoned".to_string())
        })
    }
}

fn normalize_room(room: impl Into<String>) -> Result<String> {
    let room = room.into();
    let room = room.trim();
    if room.is_empty() {
        return Err(BootError::BadRequest(
            "websocket room cannot be empty".to_string(),
        ));
    }
    Ok(room.to_string())
}

fn normalize_namespace(namespace: impl Into<String>) -> Result<String> {
    let namespace = namespace.into();
    let namespace = namespace.trim();
    if namespace.is_empty() {
        return Err(BootError::BadRequest(
            "websocket namespace cannot be empty".to_string(),
        ));
    }
    if namespace.contains('?') || namespace.contains('#') {
        return Err(BootError::BadRequest(format!(
            "websocket namespace cannot contain query or fragment markers: {namespace}"
        )));
    }
    if namespace.starts_with('/') {
        Ok(namespace.to_string())
    } else {
        Ok(format!("/{namespace}"))
    }
}

async fn send_to_outbounds(
    outbounds: Vec<Arc<dyn WebSocketOutbound>>,
    message: WebSocketMessage,
) -> Result<usize> {
    let mut sent = 0;
    for outbound in outbounds {
        outbound.send(message.clone()).await?;
        sent += 1;
    }
    Ok(sent)
}

/// Context available to WebSocket guards and interceptors.
#[derive(Debug, Clone)]
pub struct WebSocketContext {
    pub request: BootRequest,
    pub gateway_path: String,
    pub event: String,
    pub namespace: Option<String>,
    pub module_name: Option<String>,
    execution_context: ExecutionContext,
}

impl WebSocketContext {
    fn new(gateway: &WebSocketGatewayDefinition, request: BootRequest, event: &str) -> Self {
        let gateway_path = gateway.path.clone();
        let event = event.to_string();
        let namespace = gateway.namespace.clone();
        let module_name = gateway.module_name.clone();
        let execution_context = ExecutionContext::websocket(
            request.clone(),
            gateway_path.clone(),
            event.clone(),
            namespace.clone(),
            module_name.clone(),
        );
        Self {
            request,
            gateway_path,
            event,
            namespace,
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

impl Deref for WebSocketContext {
    type Target = ExecutionContext;

    fn deref(&self) -> &Self::Target {
        self.execution_context()
    }
}

/// Context passed to WebSocket gateway initialization hooks.
#[derive(Debug, Clone)]
pub struct WebSocketGatewayInitContext {
    pub gateway_path: String,
    pub namespace: Option<String>,
    pub module_name: Option<String>,
    pub events: Vec<String>,
}

impl WebSocketGatewayInitContext {
    fn new(gateway: &WebSocketGatewayDefinition) -> Self {
        Self {
            gateway_path: gateway.path.clone(),
            namespace: gateway.namespace.clone(),
            module_name: gateway.module_name.clone(),
            events: gateway.handlers.keys().cloned().collect(),
        }
    }
}

/// Hook invoked when a WebSocket gateway is initialized during application bootstrap.
pub trait WebSocketGatewayInitHook: Send + Sync + 'static {
    fn after_init(&self, context: WebSocketGatewayInitContext) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayInitHook for F
where
    F: Fn(WebSocketGatewayInitContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn after_init(&self, context: WebSocketGatewayInitContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(context))
    }
}

/// Hook invoked when a WebSocket client connects to a gateway.
pub trait WebSocketGatewayConnectionHook: Send + Sync + 'static {
    fn handle_connection(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayConnectionHook for F
where
    F: Fn(WebSocketGatewayConnection) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn handle_connection(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(connection))
    }
}

/// Hook invoked when a WebSocket client disconnects from a gateway.
pub trait WebSocketGatewayDisconnectHook: Send + Sync + 'static {
    fn handle_disconnect(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketGatewayDisconnectHook for F
where
    F: Fn(WebSocketGatewayConnection) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn handle_disconnect(
        &self,
        connection: WebSocketGatewayConnection,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(connection))
    }
}

/// Message transformation hook for WebSocket gateways.
pub trait WebSocketPipe: Send + Sync + 'static {
    fn transform(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<WebSocketMessage>>;
}

impl<F, Fut> WebSocketPipe for F
where
    F: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<WebSocketMessage>> + Send + 'static,
{
    fn transform(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<WebSocketMessage>> {
        Box::pin(self(message))
    }
}

/// Authorization hook for WebSocket gateway messages.
pub trait WebSocketGuard: Send + Sync + 'static {
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>>;
}

impl<F, Fut> WebSocketGuard for F
where
    F: Fn(WebSocketContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<bool>> + Send + 'static,
{
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(self(context))
    }
}

struct ExecutionWebSocketGuard<G> {
    inner: G,
}

impl<G> WebSocketGuard for ExecutionWebSocketGuard<G>
where
    G: Guard,
{
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        self.inner.can_activate(context.into_execution_context())
    }
}

/// Around-handler hook for WebSocket gateway messages.
pub trait WebSocketInterceptor: Send + Sync + 'static {
    fn before(&self, _context: WebSocketContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn after(
        &self,
        _context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        Box::pin(async move { Ok(reply) })
    }
}

struct ExecutionWebSocketInterceptor<I> {
    inner: I,
}

impl<I> WebSocketInterceptor for ExecutionWebSocketInterceptor<I>
where
    I: ExecutionInterceptor,
{
    fn before(&self, context: WebSocketContext) -> BoxFuture<'static, Result<()>> {
        self.inner.before(context.into_execution_context())
    }

    fn after(
        &self,
        context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        let future = self.inner.after(context.into_execution_context());
        Box::pin(async move {
            future.await?;
            Ok(reply)
        })
    }
}

type WebSocketHandlerFuture = BoxFuture<'static, Result<Option<WebSocketMessage>>>;

trait WebSocketMessageHandler: Send + Sync + 'static {
    fn call(&self, message: WebSocketMessage) -> WebSocketHandlerFuture;
}

struct WebSocketHandlerAdapter<H> {
    handler: H,
}

impl<H, Fut, R> WebSocketMessageHandler for WebSocketHandlerAdapter<H>
where
    H: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoWebSocketReply + Send + 'static,
{
    fn call(&self, message: WebSocketMessage) -> WebSocketHandlerFuture {
        let future = (self.handler)(message);
        Box::pin(async move { Ok(future.await?.into_websocket_reply()) })
    }
}

/// Framework-neutral WebSocket gateway definition.
#[derive(Clone)]
pub struct WebSocketGatewayDefinition {
    path: String,
    namespace: Option<String>,
    handlers: BTreeMap<String, Arc<dyn WebSocketMessageHandler>>,
    init_hooks: Vec<Arc<dyn WebSocketGatewayInitHook>>,
    connection_hooks: Vec<Arc<dyn WebSocketGatewayConnectionHook>>,
    disconnect_hooks: Vec<Arc<dyn WebSocketGatewayDisconnectHook>>,
    pipes: Vec<Arc<dyn WebSocketPipe>>,
    guards: Vec<Arc<dyn WebSocketGuard>>,
    interceptors: Vec<Arc<dyn WebSocketInterceptor>>,
    module_name: Option<String>,
    state: Arc<WebSocketGatewayState>,
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

fn prepend_execution_guards(
    prefix: &[Arc<dyn Guard>],
    values: Vec<Arc<dyn WebSocketGuard>>,
) -> Vec<Arc<dyn WebSocketGuard>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|guard| Arc::new(ExecutionWebSocketGuard { inner: guard }) as Arc<dyn WebSocketGuard>)
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

fn prepend_execution_interceptors(
    prefix: &[Arc<dyn ExecutionInterceptor>],
    values: Vec<Arc<dyn WebSocketInterceptor>>,
) -> Vec<Arc<dyn WebSocketInterceptor>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|interceptor| {
            Arc::new(ExecutionWebSocketInterceptor { inner: interceptor })
                as Arc<dyn WebSocketInterceptor>
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

/// Adapter-neutral WebSocket connection.
pub trait WebSocketConnection: Send + Sync {
    fn request(&self) -> &BootRequest;

    fn dispatch(
        &self,
        message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>>;
}

/// In-process WebSocket gateway connection used by adapters and tests.
#[derive(Clone)]
pub struct WebSocketGatewayConnection {
    gateway: WebSocketGatewayDefinition,
    id: u64,
    request: BootRequest,
    outbound: Option<Arc<dyn WebSocketOutbound>>,
    opened: Arc<AtomicBool>,
}

impl WebSocketGatewayConnection {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn request(&self) -> &BootRequest {
        &self.request
    }

    pub fn namespace(&self) -> Option<&str> {
        self.gateway.namespace()
    }

    pub fn rooms(&self) -> Result<Vec<String>> {
        self.gateway.state.rooms_for_connection(self.id)
    }

    pub fn join(&self, room: impl Into<String>) -> Result<()> {
        self.gateway.state.join(self.id, room)
    }

    pub fn leave(&self, room: impl Into<String>) -> Result<()> {
        self.gateway.state.leave(self.id, room)
    }

    pub async fn emit(&self, message: WebSocketMessage) -> Result<bool> {
        self.gateway.emit_to_connection(self.id, message).await
    }

    pub async fn broadcast(&self, message: WebSocketMessage) -> Result<usize> {
        let outbounds = self.gateway.state.broadcast_targets(None, Some(self.id))?;
        send_to_outbounds(outbounds, message).await
    }

    pub async fn broadcast_to_room(
        &self,
        room: impl Into<String>,
        message: WebSocketMessage,
    ) -> Result<usize> {
        let room = normalize_room(room)?;
        let outbounds = self
            .gateway
            .state
            .broadcast_targets(Some(&room), Some(self.id))?;
        send_to_outbounds(outbounds, message).await
    }

    pub async fn open(&self) -> Result<()> {
        if self.opened.swap(true, Ordering::AcqRel) {
            return Ok(());
        }
        self.gateway
            .state
            .register(self.id, self.outbound.clone())?;
        let mut hook_result = Ok(());
        for hook in &self.gateway.connection_hooks {
            if let Err(error) = hook.handle_connection(self.clone()).await {
                hook_result = Err(error);
                break;
            }
        }
        if hook_result.is_err() {
            self.opened.store(false, Ordering::Release);
            self.gateway.state.unregister(self.id)?;
        }
        hook_result
    }

    pub async fn close(&self) -> Result<()> {
        if !self.opened.swap(false, Ordering::AcqRel) {
            return Ok(());
        }
        let mut hook_result = Ok(());
        for hook in self.gateway.disconnect_hooks.iter().rev() {
            if let Err(error) = hook.handle_disconnect(self.clone()).await {
                hook_result = Err(error);
                break;
            }
        }
        let unregister_result = self.gateway.state.unregister(self.id);
        hook_result?;
        unregister_result
    }

    pub async fn dispatch(
        &self,
        mut message: WebSocketMessage,
    ) -> Result<Option<WebSocketMessage>> {
        let event = message.event.clone();
        let handler = self.gateway.handlers.get(&event).cloned().ok_or_else(|| {
            BootError::NotFound(format!("websocket event {} {}", self.gateway.path, event))
        })?;

        let context = WebSocketContext::new(&self.gateway, self.request.clone(), &message.event);
        for guard in &self.gateway.guards {
            let can_activate = guard.can_activate(context.clone()).await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "websocket event {} {}",
                    self.gateway.path, message.event
                )));
            }
        }

        for interceptor in &self.gateway.interceptors {
            interceptor.before(context.clone()).await?;
        }

        for pipe in &self.gateway.pipes {
            message = pipe.transform(message).await?;
        }

        let mut reply = handler.call(message).await?;
        for interceptor in self.gateway.interceptors.iter().rev() {
            reply = interceptor.after(context.clone(), reply).await?;
        }
        Ok(reply)
    }
}

impl WebSocketConnection for WebSocketGatewayConnection {
    fn request(&self) -> &BootRequest {
        self.request()
    }

    fn dispatch(
        &self,
        message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        let connection = self.clone();
        Box::pin(async move { connection.dispatch(message).await })
    }
}
