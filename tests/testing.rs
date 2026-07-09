use a3s_boot::{
    BootError, BootRequest, BootResponse, BoxFuture, ControllerDefinition, ExceptionFilter,
    ExecutionContext, Guard, HttpMethod, Interceptor, Module, ModuleRef, Pipe, ProviderDefinition,
    ProviderToken, Result, RouteDefinition, TestingModule, TransportContext,
    TransportExceptionResponse, TransportGuard, TransportInterceptor, TransportMessage,
    TransportPipe, TransportReply, WebSocketContext, WebSocketExceptionResponse,
    WebSocketGatewayDefinition, WebSocketGuard, WebSocketInterceptor, WebSocketMessage,
    WebSocketPipe, WebSocketSubscriptionDefinition,
};
use std::sync::Arc;

#[derive(Debug)]
struct GreetingService {
    message: &'static str,
}

#[derive(Debug)]
struct GreetingModule;

impl Module for GreetingModule {
    fn name(&self) -> &'static str {
        "GreetingModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GreetingService {
            message: "real",
        })])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let service = module_ref.get::<GreetingService>()?;
        Ok(vec![ControllerDefinition::new("/greetings")?.route(
            RouteDefinition::get("/", move |_| {
                let service = Arc::clone(&service);
                async move { Ok(BootResponse::text(service.message)) }
            })?,
        )?])
    }
}

#[derive(Debug)]
struct DirectRealModule;

impl Module for DirectRealModule {
    fn name(&self) -> &'static str {
        "DirectRealModule"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/direct-module")?
            .get("/", |_| async {
                Ok(BootResponse::text("real-module"))
            })?])
    }
}

#[derive(Debug)]
struct DirectFakeModule;

impl Module for DirectFakeModule {
    fn name(&self) -> &'static str {
        "DirectFakeModule"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/direct-module")?
            .get("/", |_| async {
                Ok(BootResponse::text("fake-module"))
            })?])
    }
}

#[derive(Debug)]
struct OverrideDependencyService {
    value: &'static str,
}

#[derive(Debug)]
struct RealDependencyModule;

impl Module for RealDependencyModule {
    fn name(&self) -> &'static str {
        "RealDependencyModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(
            OverrideDependencyService { value: "real" },
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<OverrideDependencyService>()])
    }
}

#[derive(Debug)]
struct FakeDependencyModule;

impl Module for FakeDependencyModule {
    fn name(&self) -> &'static str {
        "FakeDependencyModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(
            OverrideDependencyService { value: "fake" },
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<OverrideDependencyService>()])
    }
}

#[derive(Debug)]
struct DependencyConsumerModule;

impl Module for DependencyConsumerModule {
    fn name(&self) -> &'static str {
        "DependencyConsumerModule"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(RealDependencyModule)]
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let dependency = module_ref.get::<OverrideDependencyService>()?;
        Ok(vec![ControllerDefinition::new("/nested-module")?.get(
            "/",
            move |_| {
                let dependency = Arc::clone(&dependency);
                async move { Ok(BootResponse::text(dependency.value)) }
            },
        )?])
    }
}

#[derive(Debug)]
struct PipelineOverrideModule;

impl Module for PipelineOverrideModule {
    fn name(&self) -> &'static str {
        "PipelineOverrideModule"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/pipeline")?
            .route(
                RouteDefinition::post("/ok", |request: BootRequest| async move {
                    Ok(BootResponse::text(request.text()?))
                })?
                .with_pipe(OriginalPipe)
                .with_guard(DenyGuard)
                .with_interceptor(OriginalInterceptor),
            )?
            .route(
                RouteDefinition::get("/fail", |_| async {
                    Err(BootError::BadRequest("handler failed".to_string()))
                })?
                .with_filter(OriginalFilter),
            )?])
    }
}

#[derive(Debug)]
struct OriginalPipe;

impl Pipe for OriginalPipe {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        Box::pin(async move { Ok(request.with_body("original-pipe")) })
    }
}

#[derive(Debug)]
struct ReplacementPipe;

impl Pipe for ReplacementPipe {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        Box::pin(async move { Ok(request.with_body("replacement-pipe")) })
    }
}

#[derive(Debug)]
struct DenyGuard;

impl Guard for DenyGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[derive(Debug)]
struct AllowGuard;

impl Guard for AllowGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct OriginalInterceptor;

impl Interceptor for OriginalInterceptor {
    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(async move { Ok(response.with_header("x-test-interceptor", "original")) })
    }
}

#[derive(Debug)]
struct ReplacementInterceptor;

impl Interceptor for ReplacementInterceptor {
    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(async move { Ok(response.with_header("x-test-interceptor", "replacement")) })
    }
}

#[derive(Debug)]
struct OriginalFilter;

impl ExceptionFilter for OriginalFilter {
    fn catch(
        &self,
        _context: ExecutionContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async { Ok(Some(BootResponse::text("original-filter"))) })
    }
}

#[derive(Debug)]
struct ReplacementFilter;

impl ExceptionFilter for ReplacementFilter {
    fn catch(
        &self,
        _context: ExecutionContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async { Ok(Some(BootResponse::text("replacement-filter"))) })
    }
}

#[derive(Debug)]
struct WebSocketPipelineOverrideModule;

impl Module for WebSocketPipelineOverrideModule {
    fn name(&self) -> &'static str {
        "WebSocketPipelineOverrideModule"
    }

    fn gateways(&self, _module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        Ok(vec![WebSocketGatewayDefinition::new("/ws-pipeline")?
            .subscribe_definition(
                "ok",
                WebSocketSubscriptionDefinition::new(|message: WebSocketMessage| async move {
                    Ok(WebSocketMessage::new("ok.reply", message.data))
                })
                .with_pipe(OriginalWebSocketPipe)
                .with_guard(DenyWebSocketGuard)
                .with_interceptor(OriginalWebSocketInterceptor),
            )?
            .subscribe_definition(
                "fail",
                WebSocketSubscriptionDefinition::new(|_| async {
                    Err::<WebSocketMessage, _>(BootError::BadRequest(
                        "websocket failed".to_string(),
                    ))
                })
                .with_filter(OriginalWebSocketFilter),
            )?])
    }
}

#[derive(Debug)]
struct OriginalWebSocketPipe;

impl WebSocketPipe for OriginalWebSocketPipe {
    fn transform(
        &self,
        mut message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<WebSocketMessage>> {
        Box::pin(async move {
            message.data = serde_json::json!("original-websocket-pipe");
            Ok(message)
        })
    }
}

#[derive(Debug)]
struct ReplacementWebSocketPipe;

impl WebSocketPipe for ReplacementWebSocketPipe {
    fn transform(
        &self,
        mut message: WebSocketMessage,
    ) -> BoxFuture<'static, Result<WebSocketMessage>> {
        Box::pin(async move {
            message.data = serde_json::json!("replacement-websocket-pipe");
            Ok(message)
        })
    }
}

#[derive(Debug)]
struct DenyWebSocketGuard;

impl WebSocketGuard for DenyWebSocketGuard {
    fn can_activate(&self, _context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[derive(Debug)]
struct AllowWebSocketGuard;

impl WebSocketGuard for AllowWebSocketGuard {
    fn can_activate(&self, _context: WebSocketContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct OriginalWebSocketInterceptor;

impl WebSocketInterceptor for OriginalWebSocketInterceptor {
    fn after(
        &self,
        _context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        Box::pin(async move {
            Ok(reply.map(|mut message| {
                message.data = serde_json::json!({
                    "pipe": message.data,
                    "interceptor": "original",
                });
                message
            }))
        })
    }
}

#[derive(Debug)]
struct ReplacementWebSocketInterceptor;

impl WebSocketInterceptor for ReplacementWebSocketInterceptor {
    fn after(
        &self,
        _context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        Box::pin(async move {
            Ok(reply.map(|mut message| {
                message.data = serde_json::json!({
                    "pipe": message.data,
                    "interceptor": "replacement",
                });
                message
            }))
        })
    }
}

#[derive(Debug)]
struct OriginalWebSocketFilter;

impl a3s_boot::WebSocketExceptionFilter for OriginalWebSocketFilter {
    fn catch(
        &self,
        _context: WebSocketContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
        Box::pin(async {
            Ok(Some(WebSocketExceptionResponse::message(
                WebSocketMessage::text("fail.reply", "original-websocket-filter"),
            )))
        })
    }
}

#[derive(Debug)]
struct ReplacementWebSocketFilter;

impl a3s_boot::WebSocketExceptionFilter for ReplacementWebSocketFilter {
    fn catch(
        &self,
        _context: WebSocketContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
        Box::pin(async {
            Ok(Some(WebSocketExceptionResponse::message(
                WebSocketMessage::text("fail.reply", "replacement-websocket-filter"),
            )))
        })
    }
}

#[derive(Debug)]
struct TransportPipelineOverrideModule;

impl Module for TransportPipelineOverrideModule {
    fn name(&self) -> &'static str {
        "TransportPipelineOverrideModule"
    }

    fn message_patterns(
        &self,
        _module_ref: &ModuleRef,
    ) -> Result<Vec<a3s_boot::MessagePatternDefinition>> {
        Ok(vec![
            a3s_boot::MessagePatternDefinition::request(
                "testing.ok",
                |message: TransportMessage| async move { Ok(TransportReply::new(message.data)) },
            )?
            .with_pipe(OriginalTransportPipe)
            .with_guard(DenyTransportGuard)
            .with_interceptor(OriginalTransportInterceptor),
            a3s_boot::MessagePatternDefinition::request("testing.fail", |_| async {
                Err::<TransportReply, _>(BootError::BadRequest("transport failed".to_string()))
            })?
            .with_filter(OriginalTransportFilter),
        ])
    }
}

#[derive(Debug)]
struct OriginalTransportPipe;

impl TransportPipe for OriginalTransportPipe {
    fn transform(
        &self,
        mut message: TransportMessage,
    ) -> BoxFuture<'static, Result<TransportMessage>> {
        Box::pin(async move {
            message.data = serde_json::json!("original-transport-pipe");
            Ok(message)
        })
    }
}

#[derive(Debug)]
struct ReplacementTransportPipe;

impl TransportPipe for ReplacementTransportPipe {
    fn transform(
        &self,
        mut message: TransportMessage,
    ) -> BoxFuture<'static, Result<TransportMessage>> {
        Box::pin(async move {
            message.data = serde_json::json!("replacement-transport-pipe");
            Ok(message)
        })
    }
}

#[derive(Debug)]
struct DenyTransportGuard;

impl TransportGuard for DenyTransportGuard {
    fn can_activate(&self, _context: TransportContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[derive(Debug)]
struct AllowTransportGuard;

impl TransportGuard for AllowTransportGuard {
    fn can_activate(&self, _context: TransportContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct OriginalTransportInterceptor;

impl TransportInterceptor for OriginalTransportInterceptor {
    fn after(
        &self,
        _context: TransportContext,
        reply: Option<TransportReply>,
    ) -> BoxFuture<'static, Result<Option<TransportReply>>> {
        Box::pin(async move {
            Ok(reply.map(|reply| {
                TransportReply::new(serde_json::json!({
                    "pipe": reply.data,
                    "interceptor": "original",
                }))
            }))
        })
    }
}

#[derive(Debug)]
struct ReplacementTransportInterceptor;

impl TransportInterceptor for ReplacementTransportInterceptor {
    fn after(
        &self,
        _context: TransportContext,
        reply: Option<TransportReply>,
    ) -> BoxFuture<'static, Result<Option<TransportReply>>> {
        Box::pin(async move {
            Ok(reply.map(|reply| {
                TransportReply::new(serde_json::json!({
                    "pipe": reply.data,
                    "interceptor": "replacement",
                }))
            }))
        })
    }
}

#[derive(Debug)]
struct OriginalTransportFilter;

impl a3s_boot::TransportExceptionFilter for OriginalTransportFilter {
    fn catch(
        &self,
        _context: TransportContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        Box::pin(async {
            Ok(Some(TransportExceptionResponse::reply(
                TransportReply::text("original-transport-filter"),
            )))
        })
    }
}

#[derive(Debug)]
struct ReplacementTransportFilter;

impl a3s_boot::TransportExceptionFilter for ReplacementTransportFilter {
    fn catch(
        &self,
        _context: TransportContext,
        _error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        Box::pin(async {
            Ok(Some(TransportExceptionResponse::reply(
                TransportReply::text("replacement-transport-filter"),
            )))
        })
    }
}

#[tokio::test]
async fn testing_module_overrides_providers_before_controllers_are_built() {
    let testing = TestingModule::builder()
        .import(GreetingModule)
        .override_provider(ProviderDefinition::singleton(GreetingService {
            message: "fake",
        }))
        .compile()
        .unwrap();

    assert_eq!(testing.get::<GreetingService>().unwrap().message, "fake");

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/greetings"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "fake");
}

#[tokio::test]
async fn testing_module_overrides_direct_imported_modules_before_graph_is_built() {
    let testing = TestingModule::builder()
        .import(DirectRealModule)
        .override_module("DirectRealModule", DirectFakeModule)
        .compile()
        .unwrap();

    assert!(testing
        .app()
        .module_names()
        .contains(&"DirectFakeModule".to_string()));
    assert!(!testing
        .app()
        .module_names()
        .contains(&"DirectRealModule".to_string()));

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/direct-module"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "fake-module");
}

#[tokio::test]
async fn testing_module_overrides_nested_imported_modules_before_providers_resolve() {
    let testing = TestingModule::builder()
        .import(DependencyConsumerModule)
        .override_module_arc("RealDependencyModule", Arc::new(FakeDependencyModule))
        .compile()
        .unwrap();

    assert!(testing
        .app()
        .module_names()
        .contains(&"FakeDependencyModule".to_string()));
    assert!(!testing
        .app()
        .module_names()
        .contains(&"RealDependencyModule".to_string()));

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/nested-module"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "fake");
}

#[tokio::test]
async fn testing_module_overrides_route_pipeline_components() {
    let testing = TestingModule::builder()
        .import(PipelineOverrideModule)
        .override_pipe::<OriginalPipe, _>(ReplacementPipe)
        .override_guard::<DenyGuard, _>(AllowGuard)
        .override_interceptor::<OriginalInterceptor, _>(ReplacementInterceptor)
        .override_filter::<OriginalFilter, _>(ReplacementFilter)
        .compile()
        .unwrap();

    let ok = testing
        .call(BootRequest::new(HttpMethod::Post, "/pipeline/ok").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(ok.body_text().unwrap(), "replacement-pipe");
    assert_eq!(ok.header("x-test-interceptor"), Some("replacement"));

    let filtered = testing
        .call(BootRequest::new(HttpMethod::Get, "/pipeline/fail"))
        .await
        .unwrap();

    assert_eq!(filtered.body_text().unwrap(), "replacement-filter");
}

#[tokio::test]
async fn testing_module_overrides_websocket_pipeline_components() {
    let testing = TestingModule::builder()
        .import(WebSocketPipelineOverrideModule)
        .override_websocket_pipe::<OriginalWebSocketPipe, _>(ReplacementWebSocketPipe)
        .override_websocket_guard::<DenyWebSocketGuard, _>(AllowWebSocketGuard)
        .override_websocket_interceptor::<OriginalWebSocketInterceptor, _>(
            ReplacementWebSocketInterceptor,
        )
        .override_websocket_filter::<OriginalWebSocketFilter, _>(ReplacementWebSocketFilter)
        .compile()
        .unwrap();

    let gateway = testing.app().gateway_for("/ws-pipeline").unwrap();
    let connection = gateway
        .connect(BootRequest::new(HttpMethod::Get, "/ws-pipeline"))
        .unwrap();

    let ok = connection
        .dispatch(WebSocketMessage::text("ok", "raw"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(ok.event(), "ok.reply");
    assert_eq!(
        ok.data(),
        &serde_json::json!({
            "pipe": "replacement-websocket-pipe",
            "interceptor": "replacement",
        })
    );

    let filtered = connection
        .dispatch(WebSocketMessage::text("fail", "raw"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(filtered.event(), "fail.reply");
    assert_eq!(
        filtered.data(),
        &serde_json::json!("replacement-websocket-filter")
    );
}

#[tokio::test]
async fn testing_module_overrides_transport_pipeline_components() {
    let testing = TestingModule::builder()
        .import(TransportPipelineOverrideModule)
        .override_transport_pipe::<OriginalTransportPipe, _>(ReplacementTransportPipe)
        .override_transport_guard::<DenyTransportGuard, _>(AllowTransportGuard)
        .override_transport_interceptor::<OriginalTransportInterceptor, _>(
            ReplacementTransportInterceptor,
        )
        .override_transport_filter::<OriginalTransportFilter, _>(ReplacementTransportFilter)
        .compile()
        .unwrap();

    let ok = testing
        .app()
        .dispatch_message(TransportMessage::text("testing.ok", "raw"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        ok.data(),
        &serde_json::json!({
            "pipe": "replacement-transport-pipe",
            "interceptor": "replacement",
        })
    );

    let filtered = testing
        .app()
        .dispatch_message(TransportMessage::text("testing.fail", "raw"))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        filtered.data(),
        &serde_json::json!("replacement-transport-filter")
    );
}

#[tokio::test]
async fn testing_module_can_compile_direct_test_routes_and_providers() {
    let testing = TestingModule::builder()
        .provider(ProviderDefinition::singleton(GreetingService {
            message: "direct",
        }))
        .route(
            RouteDefinition::get("/probe", |_| async {
                BootResponse::json(&serde_json::json!({ "ok": true }))
            })
            .unwrap(),
        )
        .compile()
        .unwrap();

    assert_eq!(testing.get::<GreetingService>().unwrap().message, "direct");

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/probe"))
        .await
        .unwrap();

    assert!(response.is_json_content_type());
    assert_eq!(
        response.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({ "ok": true })
    );
}
