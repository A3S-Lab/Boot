use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, ControllerDefinition,
    ExecutionContext, HttpAdapter, HttpMethod, Interceptor, Module, ModuleRef, ProviderDefinition,
    Result, RouteDefinition,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

#[derive(Debug)]
struct HealthModule;

impl Module for HealthModule {
    fn name(&self) -> &'static str {
        "health"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("ok"))
        })?])
    }
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(HealthModule)]
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/", |_| async {
            Ok(BootResponse::text("hello"))
        })?])
    }
}

#[test]
fn registers_imports_before_parent_modules() {
    let app = BootApplication::builder()
        .import(AppModule)
        .build()
        .unwrap();

    assert_eq!(app.module_names(), ["health", "app"]);
    assert_eq!(app.routes().len(), 2);
}

#[test]
fn deduplicates_imported_modules_by_name() {
    let health = Arc::new(HealthModule);
    let app = BootApplication::builder()
        .import_arc(health.clone())
        .import_arc(health)
        .build()
        .unwrap();

    assert_eq!(app.module_names(), ["health"]);
    assert_eq!(app.routes().len(), 1);
}

#[test]
fn rejects_empty_module_names() {
    struct EmptyModule;

    impl Module for EmptyModule {
        fn name(&self) -> &'static str {
            ""
        }
    }

    let result = BootApplication::builder().import(EmptyModule).build();

    assert!(matches!(result, Err(BootError::EmptyModuleName)));
}

#[test]
fn rejects_relative_route_paths() {
    let result = RouteDefinition::get("health", |_| async { Ok(BootResponse::text("ok")) });

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

struct RecordingAdapter;

impl HttpAdapter for RecordingAdapter {
    type Output = Vec<(HttpMethod, String)>;

    fn build(&self, app: BootApplication) -> Result<Self::Output> {
        Ok(app
            .routes()
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect())
    }

    fn serve(
        &self,
        app: BootApplication,
        _addr: std::net::SocketAddr,
    ) -> BoxFuture<'static, Result<()>> {
        let route_count = app.routes().len();
        Box::pin(async move {
            assert!(route_count > 0);
            Ok(())
        })
    }
}

#[test]
fn builds_with_a_custom_http_adapter() {
    let app = BootApplication::builder()
        .import(HealthModule)
        .build()
        .unwrap();

    let routes = app.into_adapter(&RecordingAdapter).unwrap();

    assert_eq!(routes, vec![(HttpMethod::Get, "/health".to_string())]);
}

#[derive(Debug)]
struct CatsService;

impl CatsService {
    fn find_all(&self) -> &'static str {
        "cat-a,cat-b"
    }
}

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(CatsService)])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let cats = module_ref.get::<CatsService>()?;
        Ok(vec![ControllerDefinition::new("/cats")?.get(
            "/",
            move |_| {
                let cats = Arc::clone(&cats);
                async move { Ok(BootResponse::text(cats.find_all())) }
            },
        )?])
    }
}

#[tokio::test]
async fn registers_controller_routes_with_provider_injection() {
    let app = BootApplication::builder()
        .import(CatsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes()[0].path(), "/cats");
    assert!(app.get::<CatsService>().is_ok());

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap();

    assert_eq!(response.body, b"cat-a,cat-b");
}

#[tokio::test]
async fn route_pipeline_runs_pipes_guards_interceptors_and_filters() {
    struct HeaderInterceptor;

    impl Interceptor for HeaderInterceptor {
        fn after(
            &self,
            _context: ExecutionContext,
            response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            Box::pin(async move { Ok(response.with_header("x-boot", "ok")) })
        }
    }

    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        Ok(BootResponse::text(request.text()?))
    })
    .unwrap()
    .with_pipe(|request: BootRequest| async move { Ok(request.with_body("transformed")) })
    .with_guard(|context: ExecutionContext| async move { Ok(context.request_path == "/") })
    .with_interceptor(HeaderInterceptor)
    .with_filter(|_, error: BootError| async move {
        Ok(Some(BootResponse::text(error.to_string()).with_status(500)))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.body, b"transformed");
    assert_eq!(
        response.headers.get("x-boot").map(String::as_str),
        Some("ok")
    );
}

#[derive(Clone)]
struct TraceInterceptor {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl TraceInterceptor {
    fn new(name: &'static str, log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { name, log }
    }
}

impl Interceptor for TraceInterceptor {
    fn before(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("before:{name}"));
            Ok(())
        })
    }

    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("after:{name}"));
            Ok(response)
        })
    }
}

#[derive(Debug)]
struct PipelineModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for PipelineModule {
    fn name(&self) -> &'static str {
        "pipeline"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ControllerDefinition::new("/pipeline")?
            .with_interceptor(TraceInterceptor::new("controller", Arc::clone(&self.log)))
            .get("/", move |_| {
                let log = Arc::clone(&log);
                async move {
                    log.lock().unwrap().push("handler".to_string());
                    Ok(BootResponse::text("ok"))
                }
            })?])
    }
}

#[tokio::test]
async fn global_and_controller_interceptors_wrap_route_in_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .use_global_interceptor(TraceInterceptor::new("global", Arc::clone(&log)))
        .import(PipelineModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/pipeline"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "before:global",
            "before:controller",
            "handler",
            "after:controller",
            "after:global"
        ]
    );
}

#[tokio::test]
async fn global_guards_and_filters_apply_to_controller_routes() {
    let app = BootApplication::builder()
        .use_global_guard(|_| async { Ok(false) })
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(403),
            ))
        })
        .import(CatsModule)
        .build()
        .unwrap();

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap();

    assert_eq!(response.status, 403);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/cats: request was forbidden: GET /cats"
    );
}

#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct CatDto {
    name: String,
    adopted: bool,
}

#[tokio::test]
async fn json_controller_routes_decode_dtos_and_encode_responses() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .post_json("/", |dto: CreateCatDto| async move {
            Ok(CatDto {
                name: dto.name,
                adopted: false,
            })
        })
        .unwrap();
    let route = controller.routes()[0].clone();

    let response = route
        .call(
            BootRequest::new(HttpMethod::Post, "/cats")
                .with_header("content-type", "application/json")
                .with_body(r#"{"name":"Milo"}"#),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        serde_json::from_slice::<CatDto>(&response.body).unwrap(),
        CatDto {
            name: "Milo".to_string(),
            adopted: false,
        }
    );
}

#[tokio::test]
async fn json_controller_routes_reject_invalid_json_as_bad_request() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .post_json("/", |dto: CreateCatDto| async move {
            Ok(CatDto {
                name: dto.name,
                adopted: false,
            })
        })
        .unwrap();

    let error = controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/cats")
                .with_header("content-type", "application/json")
                .with_body("{"),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[derive(Debug, Deserialize)]
struct CatQueryDto {
    verbose: bool,
}

#[tokio::test]
async fn route_calls_extract_path_params_and_query_params() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .get("/{id}", |request: BootRequest| async move {
            let query: CatQueryDto = request.query()?;
            Ok(BootResponse::text(format!(
                "{}:{}:{}",
                request.param("id").unwrap_or("missing"),
                request.query_param("verbose").unwrap_or("missing"),
                query.verbose
            )))
        })
        .unwrap();

    let response = controller.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/cats/milo?verbose=true"))
        .await
        .unwrap();

    assert_eq!(response.body, b"milo:true:true");
}
