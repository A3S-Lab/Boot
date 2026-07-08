use crate::routing::path::{
    join_paths, match_path_params, match_path_shape, route_shape_key, validate_route_path,
};
use crate::{
    BootError, BootRequest, BoxFuture, ExecutionContext, ExecutionInterceptor, Guard, HttpMethod,
    Result,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::ops::Deref;
use std::sync::Arc;

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

/// Context available to WebSocket guards and interceptors.
#[derive(Debug, Clone)]
pub struct WebSocketContext {
    pub request: BootRequest,
    pub gateway_path: String,
    pub event: String,
    pub module_name: Option<String>,
    execution_context: ExecutionContext,
}

impl WebSocketContext {
    fn new(gateway: &WebSocketGatewayDefinition, request: BootRequest, event: &str) -> Self {
        let gateway_path = gateway.path.clone();
        let event = event.to_string();
        let module_name = gateway.module_name.clone();
        let execution_context = ExecutionContext::websocket(
            request.clone(),
            gateway_path.clone(),
            event.clone(),
            module_name.clone(),
        );
        Self {
            request,
            gateway_path,
            event,
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
    handlers: BTreeMap<String, Arc<dyn WebSocketMessageHandler>>,
    pipes: Vec<Arc<dyn WebSocketPipe>>,
    guards: Vec<Arc<dyn WebSocketGuard>>,
    interceptors: Vec<Arc<dyn WebSocketInterceptor>>,
    module_name: Option<String>,
}

impl WebSocketGatewayDefinition {
    pub fn new(path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        validate_route_path(&path)?;
        Ok(Self {
            path,
            handlers: BTreeMap::new(),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            module_name: None,
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

    pub fn events(&self) -> Vec<&str> {
        self.handlers.keys().map(String::as_str).collect()
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
            request: request.with_path_params(params),
        })
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
    request: BootRequest,
}

impl WebSocketGatewayConnection {
    pub fn request(&self) -> &BootRequest {
        &self.request
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
