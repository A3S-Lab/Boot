use a3s_boot::{
    BootApplication, BootError, BootRequest, BoxFuture, CallHandler, FromModuleRef, HttpMethod,
    MessagePatternDefinition, Module, ModuleRef, ProviderDefinition, ProviderDependency, Result,
    TransportContext, TransportExceptionFilter, TransportExceptionResponse, TransportGuard,
    TransportInterceptor, TransportMessage, TransportPipe, TransportReply, WebSocketContext,
    WebSocketExceptionFilter, WebSocketExceptionResponse, WebSocketGatewayConnection,
    WebSocketGatewayDefinition, WebSocketGuard, WebSocketInterceptor, WebSocketMessage,
    WebSocketPipe,
};
use serde_json::{json, Value};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

const PROVIDER_ID_FIELD: &str = "providerId";
const PROVIDER_CONTEXT_ID_FIELD: &str = "providerContextId";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ProtocolEvent {
    stage: &'static str,
    provider_id: usize,
    provider_context_id: u64,
    execution_context_id: u64,
}

#[derive(Debug)]
struct ProtocolRequestTrace {
    provider_id: usize,
    context_id: u64,
    events: Arc<Mutex<Vec<ProtocolEvent>>>,
}

impl ProtocolRequestTrace {
    fn record(&self, stage: &'static str, request: Option<&BootRequest>) {
        let execution_context_id = request
            .and_then(BootRequest::context_id)
            .map(|context_id| context_id.id())
            .unwrap_or(self.context_id);
        self.events.lock().unwrap().push(ProtocolEvent {
            stage,
            provider_id: self.provider_id,
            provider_context_id: self.context_id,
            execution_context_id,
        });
    }
}

fn protocol_trace_provider(
    calls: &Arc<AtomicUsize>,
    events: &Arc<Mutex<Vec<ProtocolEvent>>>,
) -> ProviderDefinition {
    let calls = Arc::clone(calls);
    let events = Arc::clone(events);
    ProviderDefinition::request_scoped::<ProtocolRequestTrace, _>(move |module_ref| {
        let context_id = module_ref
            .context_id()
            .ok_or_else(|| {
                BootError::Internal("protocol trace was resolved without a ContextId".to_string())
            })?
            .id();
        Ok(ProtocolRequestTrace {
            provider_id: calls.fetch_add(1, Ordering::SeqCst) + 1,
            context_id,
            events: Arc::clone(&events),
        })
    })
}

fn attach_trace(data: &mut Value, trace: &ProtocolRequestTrace) -> Result<()> {
    let fields = data.as_object_mut().ok_or_else(|| {
        BootError::BadRequest("provider enhancer test payload must be an object".to_string())
    })?;
    fields.insert(PROVIDER_ID_FIELD.to_string(), json!(trace.provider_id));
    fields.insert(
        PROVIDER_CONTEXT_ID_FIELD.to_string(),
        json!(trace.context_id),
    );
    Ok(())
}

macro_rules! trace_enhancer_providers {
    ($($provider:ident),+ $(,)?) => {
        $(
            #[derive(Debug)]
            struct $provider(Arc<ProtocolRequestTrace>);

            impl FromModuleRef for $provider {
                fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
                    Ok(Self(module_ref.get::<ProtocolRequestTrace>()?))
                }

                fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
                    Some(vec![ProviderDependency::typed::<ProtocolRequestTrace>()])
                }
            }
        )+
    };
}

trace_enhancer_providers!(
    ProviderWebSocketGuard,
    ProviderWebSocketPipe,
    ProviderWebSocketInterceptor,
    ProviderWebSocketFilter,
    ProviderTransportGuard,
    ProviderTransportPipe,
    ProviderTransportInterceptor,
    ProviderTransportFilter,
);

impl WebSocketGuard for ProviderWebSocketGuard {
    fn can_activate(&self, context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        self.0.record("guard", Some(&context.request));
        Box::pin(async { Ok(true) })
    }
}

impl WebSocketPipe for ProviderWebSocketPipe {
    fn transform(
        &self,
        mut message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<WebSocketMessage>> {
        self.0.record("pipe", None);
        let result = attach_trace(&mut message.data, &self.0).map(|()| message);
        Box::pin(async move { result })
    }
}

impl WebSocketInterceptor for ProviderWebSocketInterceptor {
    fn intercept<'a>(
        &'a self,
        context: WebSocketContext,
        next: CallHandler<'a, Option<WebSocketMessage>>,
    ) -> BoxFuture<'a, Result<Option<WebSocketMessage>>> {
        Box::pin(async move {
            self.0.record("interceptor-before", Some(&context.request));
            let reply = next.handle().await?;
            self.0.record("interceptor-after", Some(&context.request));
            Ok(reply)
        })
    }
}

impl WebSocketExceptionFilter for ProviderWebSocketFilter {
    fn catch(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
        self.0.record("filter", Some(&context.request));
        let response = WebSocketExceptionResponse::message(WebSocketMessage::text(
            "provider.filtered",
            format!(
                "ws-filtered:{}:{}:{error}",
                self.0.provider_id, self.0.context_id
            ),
        ));
        Box::pin(async move { Ok(Some(response)) })
    }
}

impl TransportGuard for ProviderTransportGuard {
    fn can_activate(&self, context: TransportContext) -> BoxFuture<'static, Result<bool>> {
        self.0
            .record("guard", Some(&context.execution_context().request));
        Box::pin(async { Ok(true) })
    }
}

impl TransportPipe for ProviderTransportPipe {
    fn transform(
        &self,
        mut message: TransportMessage,
    ) -> BoxFuture<'static, Result<TransportMessage>> {
        self.0.record("pipe", None);
        let result = attach_trace(&mut message.data, &self.0).map(|()| message);
        Box::pin(async move { result })
    }
}

impl TransportInterceptor for ProviderTransportInterceptor {
    fn intercept<'a>(
        &'a self,
        context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            self.0.record(
                "interceptor-before",
                Some(&context.execution_context().request),
            );
            let reply = next.handle().await?;
            self.0.record(
                "interceptor-after",
                Some(&context.execution_context().request),
            );
            Ok(reply)
        })
    }
}

impl TransportExceptionFilter for ProviderTransportFilter {
    fn catch(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        self.0
            .record("filter", Some(&context.execution_context().request));
        let response = TransportExceptionResponse::reply(TransportReply::text(format!(
            "transport-filtered:{}:{}:{error}",
            self.0.provider_id, self.0.context_id
        )));
        Box::pin(async move { Ok(Some(response)) })
    }
}

#[derive(Debug)]
struct ProtocolAppEnhancerModule {
    calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<ProtocolEvent>>>,
}

impl Module for ProtocolAppEnhancerModule {
    fn name(&self) -> &'static str {
        "protocol-app-enhancers"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            protocol_trace_provider(&self.calls, &self.events),
            ProviderDefinition::app_websocket_guard::<ProviderWebSocketGuard>(),
            ProviderDefinition::app_websocket_pipe::<ProviderWebSocketPipe>(),
            ProviderDefinition::app_websocket_interceptor::<ProviderWebSocketInterceptor>(),
            ProviderDefinition::app_websocket_filter::<ProviderWebSocketFilter>(),
            ProviderDefinition::app_transport_guard::<ProviderTransportGuard>(),
            ProviderDefinition::app_transport_pipe::<ProviderTransportPipe>(),
            ProviderDefinition::app_transport_interceptor::<ProviderTransportInterceptor>(),
            ProviderDefinition::app_transport_filter::<ProviderTransportFilter>(),
        ])
    }
}

#[derive(Debug)]
struct ProtocolTargetModule;

impl Module for ProtocolTargetModule {
    fn name(&self) -> &'static str {
        "protocol-app-enhancer-target"
    }

    fn gateways(&self, module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        ensure_trace_is_private(module_ref)?;
        Ok(vec![WebSocketGatewayDefinition::new("/provider-app/ws")?
            .subscribe_with_connection(
                "trace",
                |connection: WebSocketGatewayConnection, message: WebSocketMessage| async move {
                    let provider_id = message.data_field_as::<usize>(PROVIDER_ID_FIELD)?;
                    let provider_context_id =
                        message.data_field_as::<u64>(PROVIDER_CONTEXT_ID_FIELD)?;
                    let handler_context_id = connection
                        .request()
                        .context_id()
                        .ok_or_else(|| {
                            BootError::Internal(
                                "websocket handler request is missing its ContextId".to_string(),
                            )
                        })?
                        .id();
                    if handler_context_id != provider_context_id {
                        return Err(BootError::Internal(format!(
                            "websocket handler ContextId {handler_context_id} differs from provider ContextId {provider_context_id}"
                        )));
                    }
                    if message.data_field_as::<bool>("fail")? {
                        return Err(BootError::BadRequest(format!(
                            "websocket boom:{provider_id}:{provider_context_id}"
                        )));
                    }
                    Ok(WebSocketMessage::text(
                        "provider.reply",
                        format!("{provider_id}:{provider_context_id}"),
                    ))
                },
            )?])
    }

    fn message_patterns(&self, module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        ensure_trace_is_private(module_ref)?;
        Ok(vec![MessagePatternDefinition::request(
            "provider.transport",
            |message: TransportMessage| async move {
                let provider_id = message.data_field_as::<usize>(PROVIDER_ID_FIELD)?;
                let provider_context_id =
                    message.data_field_as::<u64>(PROVIDER_CONTEXT_ID_FIELD)?;
                if message.data_field_as::<bool>("fail")? {
                    return Err(BootError::BadRequest(format!(
                        "transport boom:{provider_id}:{provider_context_id}"
                    )));
                }
                Ok(TransportReply::text(format!(
                    "{provider_id}:{provider_context_id}"
                )))
            },
        )?])
    }
}

fn ensure_trace_is_private(module_ref: &ModuleRef) -> Result<()> {
    if module_ref.contains_provider::<ProtocolRequestTrace>()? {
        return Err(BootError::Internal(
            "the target module unexpectedly sees the private protocol trace provider".to_string(),
        ));
    }
    Ok(())
}

fn take_events(events: &Arc<Mutex<Vec<ProtocolEvent>>>) -> Vec<ProtocolEvent> {
    std::mem::take(&mut *events.lock().unwrap())
}

fn assert_dispatch_events(
    events: &[ProtocolEvent],
    expected_stages: &[&'static str],
    expected_provider_id: usize,
) -> u64 {
    assert_eq!(
        events.iter().map(|event| event.stage).collect::<Vec<_>>(),
        expected_stages
    );
    assert!(events
        .iter()
        .all(|event| event.provider_id == expected_provider_id));
    let context_id = events.first().unwrap().provider_context_id;
    assert!(events.iter().all(|event| {
        event.provider_context_id == context_id && event.execution_context_id == context_id
    }));
    context_id
}

fn protocol_app(
    calls: &Arc<AtomicUsize>,
    events: &Arc<Mutex<Vec<ProtocolEvent>>>,
) -> BootApplication {
    BootApplication::builder()
        .import(ProtocolAppEnhancerModule {
            calls: Arc::clone(calls),
            events: Arc::clone(events),
        })
        .import(ProtocolTargetModule)
        .build()
        .unwrap()
}

#[tokio::test]
async fn provider_backed_websocket_app_enhancers_use_one_fresh_context_per_message() {
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let app = protocol_app(&calls, &events);
    let connection = app
        .gateway_for("/provider-app/ws")
        .unwrap()
        .connect(BootRequest::new(HttpMethod::Get, "/provider-app/ws"))
        .unwrap();

    let first_reply = connection
        .dispatch(WebSocketMessage::new("trace", json!({ "fail": false })))
        .await
        .unwrap()
        .unwrap();
    let first_context_id = assert_dispatch_events(
        &take_events(&events),
        &["guard", "interceptor-before", "pipe", "interceptor-after"],
        1,
    );
    assert_eq!(
        first_reply,
        WebSocketMessage::text("provider.reply", format!("1:{first_context_id}"))
    );

    let handled_error = connection
        .dispatch(WebSocketMessage::new("trace", json!({ "fail": true })))
        .await
        .unwrap()
        .unwrap();
    let second_context_id = assert_dispatch_events(
        &take_events(&events),
        &["guard", "interceptor-before", "pipe", "filter"],
        2,
    );
    assert_eq!(
        handled_error,
        WebSocketMessage::text(
            "provider.filtered",
            format!(
                "ws-filtered:2:{second_context_id}:bad request: websocket boom:2:{second_context_id}"
            ),
        )
    );
    assert_ne!(first_context_id, second_context_id);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn provider_backed_transport_app_enhancers_use_one_fresh_context_per_dispatch() {
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let app = protocol_app(&calls, &events);

    let first_reply = app
        .dispatch_message(TransportMessage::new(
            "provider.transport",
            json!({ "fail": false }),
        ))
        .await
        .unwrap()
        .unwrap();
    let first_context_id = assert_dispatch_events(
        &take_events(&events),
        &["guard", "interceptor-before", "pipe", "interceptor-after"],
        1,
    );
    assert_eq!(
        first_reply,
        TransportReply::text(format!("1:{first_context_id}"))
    );

    let handled_error = app
        .dispatch_message(TransportMessage::new(
            "provider.transport",
            json!({ "fail": true }),
        ))
        .await
        .unwrap()
        .unwrap();
    let second_context_id = assert_dispatch_events(
        &take_events(&events),
        &["guard", "interceptor-before", "pipe", "filter"],
        2,
    );
    assert_eq!(
        handled_error,
        TransportReply::text(format!(
            "transport-filtered:2:{second_context_id}:bad request: transport boom:2:{second_context_id}"
        ))
    );
    assert_ne!(first_context_id, second_context_id);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}
