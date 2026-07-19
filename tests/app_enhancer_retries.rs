use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, CallHandler,
    ExecutionContext, FromModuleRef, HttpMethod, Interceptor, MessagePatternDefinition, Module,
    ModuleRef, ProviderDefinition, ProviderDependency, Result, RouteDefinition, TransportContext,
    TransportInterceptor, TransportMessage, TransportPipe, TransportReply, WebSocketContext,
    WebSocketGatewayDefinition, WebSocketInterceptor, WebSocketMessage, WebSocketPipe,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct RetryTrace {
    context_id: u64,
    events: Arc<Mutex<Vec<(&'static str, u64)>>>,
}

impl RetryTrace {
    fn record(&self, stage: &'static str) {
        self.events.lock().unwrap().push((stage, self.context_id));
    }

    fn ensure_request_context(&self, request: &BootRequest) -> Result<()> {
        let execution_context_id = request
            .context_id()
            .ok_or_else(|| BootError::Internal("retry request has no ContextId".to_string()))?
            .id();
        if execution_context_id != self.context_id {
            return Err(BootError::Internal(format!(
                "retry execution ContextId {execution_context_id} differs from provider ContextId {}",
                self.context_id
            )));
        }
        Ok(())
    }
}

macro_rules! retry_enhancer_providers {
    ($($provider:ident),+ $(,)?) => {
        $(
            #[derive(Debug)]
            struct $provider(Arc<RetryTrace>);

            impl FromModuleRef for $provider {
                fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
                    Ok(Self(module_ref.get::<RetryTrace>()?))
                }

                fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
                    Some(vec![ProviderDependency::typed::<RetryTrace>()])
                }
            }
        )+
    };
}

retry_enhancer_providers!(
    HttpRetryInterceptor,
    HttpRetryPipe,
    WebSocketRetryInterceptor,
    WebSocketRetryPipe,
    TransportRetryInterceptor,
    TransportRetryPipe,
);

impl Interceptor for HttpRetryInterceptor {
    fn intercept<'a>(
        &'a self,
        context: ExecutionContext,
        next: CallHandler<'a>,
    ) -> BoxFuture<'a, Result<BootResponse>> {
        Box::pin(async move {
            self.0.ensure_request_context(&context.request)?;
            self.0.record("http-interceptor");
            match next.handle().await {
                Err(BootError::ServiceUnavailable(_)) => next.handle().await,
                result => result,
            }
        })
    }
}

impl a3s_boot::Pipe for HttpRetryPipe {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        let result = self.0.ensure_request_context(&request).map(|()| {
            self.0.record("http-pipe");
            request
        });
        Box::pin(async move { result })
    }
}

impl WebSocketInterceptor for WebSocketRetryInterceptor {
    fn intercept<'a>(
        &'a self,
        context: WebSocketContext,
        next: CallHandler<'a, Option<WebSocketMessage>>,
    ) -> BoxFuture<'a, Result<Option<WebSocketMessage>>> {
        Box::pin(async move {
            self.0.ensure_request_context(&context.request)?;
            self.0.record("websocket-interceptor");
            match next.handle().await {
                Err(BootError::ServiceUnavailable(_)) => next.handle().await,
                result => result,
            }
        })
    }
}

impl WebSocketPipe for WebSocketRetryPipe {
    fn transform(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<WebSocketMessage>> {
        self.0.record("websocket-pipe");
        Box::pin(async move { Ok(message) })
    }
}

impl TransportInterceptor for TransportRetryInterceptor {
    fn intercept<'a>(
        &'a self,
        context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            self.0
                .ensure_request_context(&context.execution_context().request)?;
            self.0.record("transport-interceptor");
            match next.handle().await {
                Err(BootError::ServiceUnavailable(_)) => next.handle().await,
                result => result,
            }
        })
    }
}

impl TransportPipe for TransportRetryPipe {
    fn transform(&self, message: TransportMessage) -> BoxFuture<'static, Result<TransportMessage>> {
        self.0.record("transport-pipe");
        Box::pin(async move { Ok(message) })
    }
}

#[derive(Debug)]
struct RetryAppEnhancerModule {
    trace_factory_calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<(&'static str, u64)>>>,
    http_attempts: Arc<AtomicUsize>,
    websocket_attempts: Arc<AtomicUsize>,
    transport_attempts: Arc<AtomicUsize>,
}

impl Module for RetryAppEnhancerModule {
    fn name(&self) -> &'static str {
        "retry-app-enhancers"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let trace_factory_calls = Arc::clone(&self.trace_factory_calls);
        let events = Arc::clone(&self.events);
        Ok(vec![
            ProviderDefinition::request_scoped::<RetryTrace, _>(move |module_ref| {
                let context_id = module_ref
                    .context_id()
                    .ok_or_else(|| BootError::Internal("retry trace has no ContextId".to_string()))?
                    .id();
                trace_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(RetryTrace {
                    context_id,
                    events: Arc::clone(&events),
                })
            }),
            ProviderDefinition::app_interceptor::<HttpRetryInterceptor>(),
            ProviderDefinition::app_pipe::<HttpRetryPipe>(),
            ProviderDefinition::app_websocket_interceptor::<WebSocketRetryInterceptor>(),
            ProviderDefinition::app_websocket_pipe::<WebSocketRetryPipe>(),
            ProviderDefinition::app_transport_interceptor::<TransportRetryInterceptor>(),
            ProviderDefinition::app_transport_pipe::<TransportRetryPipe>(),
        ])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        let attempts = Arc::clone(&self.http_attempts);
        Ok(vec![RouteDefinition::get("/retry-context", move |_| {
            let attempts = Arc::clone(&attempts);
            async move {
                if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(BootError::ServiceUnavailable("retry HTTP".to_string()));
                }
                Ok(BootResponse::text("http-retried"))
            }
        })?])
    }

    fn gateways(&self, _module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        let attempts = Arc::clone(&self.websocket_attempts);
        Ok(vec![WebSocketGatewayDefinition::new("/retry-context/ws")?
            .subscribe("retry", move |_| {
                let attempts = Arc::clone(&attempts);
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Err(BootError::ServiceUnavailable("retry WebSocket".to_string()));
                    }
                    Ok(WebSocketMessage::text("retried", "websocket-retried"))
                }
            })?])
    }

    fn message_patterns(&self, _module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        let attempts = Arc::clone(&self.transport_attempts);
        Ok(vec![MessagePatternDefinition::request(
            "retry.context",
            move |_| {
                let attempts = Arc::clone(&attempts);
                async move {
                    if attempts.fetch_add(1, Ordering::SeqCst) == 0 {
                        return Err(BootError::ServiceUnavailable("retry transport".to_string()));
                    }
                    Ok(TransportReply::text("transport-retried"))
                }
            },
        )?])
    }
}

struct RetryTestHarness {
    app: BootApplication,
    trace_factory_calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<(&'static str, u64)>>>,
    http_attempts: Arc<AtomicUsize>,
    websocket_attempts: Arc<AtomicUsize>,
    transport_attempts: Arc<AtomicUsize>,
}

fn retry_app() -> RetryTestHarness {
    let trace_factory_calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let http_attempts = Arc::new(AtomicUsize::new(0));
    let websocket_attempts = Arc::new(AtomicUsize::new(0));
    let transport_attempts = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(RetryAppEnhancerModule {
            trace_factory_calls: Arc::clone(&trace_factory_calls),
            events: Arc::clone(&events),
            http_attempts: Arc::clone(&http_attempts),
            websocket_attempts: Arc::clone(&websocket_attempts),
            transport_attempts: Arc::clone(&transport_attempts),
        })
        .build()
        .unwrap();
    RetryTestHarness {
        app,
        trace_factory_calls,
        events,
        http_attempts,
        websocket_attempts,
        transport_attempts,
    }
}

fn assert_retry_context(harness: &RetryTestHarness, stages: &[&'static str]) {
    let events = harness.events.lock().unwrap();
    assert_eq!(
        events.iter().map(|(stage, _)| *stage).collect::<Vec<_>>(),
        stages
    );
    let context_id = events.first().unwrap().1;
    assert!(events.iter().all(|(_, id)| *id == context_id));
    assert_eq!(harness.trace_factory_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn http_app_enhancer_retry_reuses_the_invocation_context() {
    let harness = retry_app();
    let response = harness
        .app
        .call(BootRequest::new(HttpMethod::Get, "/retry-context"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "http-retried");
    assert_eq!(harness.http_attempts.load(Ordering::SeqCst), 2);
    assert_retry_context(&harness, &["http-interceptor", "http-pipe", "http-pipe"]);
}

#[tokio::test]
async fn websocket_app_enhancer_retry_reuses_the_message_context() {
    let harness = retry_app();
    let connection = harness
        .app
        .gateway_for("/retry-context/ws")
        .unwrap()
        .connect(BootRequest::new(HttpMethod::Get, "/retry-context/ws"))
        .unwrap();
    let reply = connection
        .dispatch(WebSocketMessage::new("retry", json!({})))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::text("retried", "websocket-retried")
    );
    assert_eq!(harness.websocket_attempts.load(Ordering::SeqCst), 2);
    assert_retry_context(
        &harness,
        &["websocket-interceptor", "websocket-pipe", "websocket-pipe"],
    );
}

#[tokio::test]
async fn transport_app_enhancer_retry_reuses_the_message_context() {
    let harness = retry_app();
    let reply = harness
        .app
        .dispatch_message(TransportMessage::new("retry.context", json!({})))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, TransportReply::text("transport-retried"));
    assert_eq!(harness.transport_attempts.load(Ordering::SeqCst), 2);
    assert_retry_context(
        &harness,
        &["transport-interceptor", "transport-pipe", "transport-pipe"],
    );
}
