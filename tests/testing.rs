use a3s_boot::{
    BootError, BootRequest, BootResponse, BoxFuture, ControllerDefinition, ExceptionFilter,
    ExecutionContext, Guard, HttpMethod, Interceptor, Module, ModuleRef, Pipe, ProviderDefinition,
    Result, RouteDefinition, TestingModule,
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
