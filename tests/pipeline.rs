use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, ControllerDefinition,
    ExecutionContext, HttpMethod, Interceptor, Middleware, MiddlewareOutcome, Module, ModuleRef,
    Result, RouteDefinition,
};
use serde_json::json;
use std::sync::Arc;

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

#[tokio::test]
async fn middleware_runs_before_pipes_and_can_mutate_requests() {
    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        Ok(BootResponse::text(format!(
            "{}:{}",
            request.header("x-middleware").unwrap_or("missing"),
            request.text()?
        )))
    })
    .unwrap()
    .with_middleware(|request: BootRequest| async move {
        Ok(MiddlewareOutcome::next(
            request
                .with_header("x-middleware", "route")
                .with_body("from-middleware"),
        ))
    })
    .with_pipe(|request: BootRequest| async move {
        let body = request.text()?;
        Ok(request.with_body(format!("{body}+pipe")))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.body, b"route:from-middleware+pipe");
}

#[tokio::test]
async fn middleware_can_short_circuit_before_guards() {
    let guard_calls = Arc::new(std::sync::Mutex::new(0usize));
    let guard_log = Arc::clone(&guard_calls);
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_middleware(|_| async {
            Ok(MiddlewareOutcome::response(
                BootResponse::text("middleware").with_status(202),
            ))
        })
        .with_guard(move |_| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                *guard_log.lock().unwrap() += 1;
                Ok(false)
            }
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 202);
    assert_eq!(response.body, b"middleware");
    assert_eq!(*guard_calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn route_filters_handle_middleware_errors() {
    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_middleware(|_| async { Err(BootError::BadRequest("bad middleware".to_string())) })
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status(), 400);
    assert_eq!(response.body, b"/: bad request: bad middleware");
}

#[tokio::test]
async fn route_filters_handle_pipe_errors() {
    let route = RouteDefinition::post("/", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_pipe(|_| async { Err(BootError::BadRequest("invalid input".to_string())) })
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Post, "/").with_body("raw"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"/: bad request: invalid input");
}

#[tokio::test]
async fn route_filters_handle_interceptor_errors() {
    struct FailingAfterInterceptor;

    impl Interceptor for FailingAfterInterceptor {
        fn after(
            &self,
            _context: ExecutionContext,
            _response: BootResponse,
        ) -> BoxFuture<'static, Result<BootResponse>> {
            Box::pin(async { Err(BootError::Internal("response failed".to_string())) })
        }
    }

    let route = RouteDefinition::get("/", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_interceptor(FailingAfterInterceptor)
        .with_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(500),
            ))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status, 500);
    assert_eq!(response.body, b"/: internal error: response failed");
}

#[tokio::test]
async fn route_filters_can_decline_and_next_filter_sees_original_error() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let catch_log = Arc::clone(&log);
    let decline_log = Arc::clone(&log);
    let route = RouteDefinition::get("/", |_| async {
        Err(BootError::BadRequest("bad input".to_string()))
    })
    .unwrap()
    .with_filter(move |context: ExecutionContext, error: BootError| {
        let catch_log = Arc::clone(&catch_log);
        async move {
            catch_log.lock().unwrap().push(format!("catch:{error}"));
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        }
    })
    .with_filter(move |_, _| {
        let decline_log = Arc::clone(&decline_log);
        async move {
            decline_log.lock().unwrap().push("decline".to_string());
            Ok(None)
        }
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(response.body, b"/: bad request: bad input");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["decline", "catch:bad request: bad input"]
    );
}

#[tokio::test]
async fn route_filters_return_original_error_when_all_decline() {
    let route = RouteDefinition::get("/", |_| async {
        Err(BootError::BadRequest("bad input".to_string()))
    })
    .unwrap()
    .with_filter(|_, _| async { Ok(None) });

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(message) if message == "bad input"));
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

fn trace_middleware(
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
) -> impl Fn(BootRequest) -> std::future::Ready<Result<MiddlewareOutcome>> + Send + Sync + 'static {
    move |request| {
        log.lock().unwrap().push(format!("middleware:{name}"));
        std::future::ready(Ok(MiddlewareOutcome::next(request)))
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

#[derive(Debug)]
struct MiddlewareOrderModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for MiddlewareOrderModule {
    fn name(&self) -> &'static str {
        "middleware-order"
    }

    fn middleware(&self) -> Vec<Arc<dyn Middleware>> {
        vec![Arc::new(trace_middleware("module", Arc::clone(&self.log)))]
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let log = Arc::clone(&self.log);
        let pipe_log = Arc::clone(&self.log);
        let guard_log = Arc::clone(&self.log);
        let handler_log = Arc::clone(&self.log);

        let route = RouteDefinition::get("/", move |_| {
            let handler_log = Arc::clone(&handler_log);
            async move {
                handler_log.lock().unwrap().push("handler".to_string());
                Ok(BootResponse::text("ok"))
            }
        })?
        .with_middleware(trace_middleware("route", Arc::clone(&log)))
        .with_pipe(move |request: BootRequest| {
            let pipe_log = Arc::clone(&pipe_log);
            async move {
                pipe_log.lock().unwrap().push("pipe".to_string());
                Ok(request)
            }
        })
        .with_guard(move |_| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                guard_log.lock().unwrap().push("guard".to_string());
                Ok(true)
            }
        })
        .with_interceptor(TraceInterceptor::new("route", Arc::clone(&self.log)));

        Ok(vec![ControllerDefinition::new("/middleware-order")?
            .with_middleware(trace_middleware("controller", Arc::clone(&self.log)))
            .route(route)?])
    }
}

#[tokio::test]
async fn middleware_runs_before_pipes_guards_interceptors_and_handlers() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .use_global_middleware(trace_middleware("global", Arc::clone(&log)))
        .import(MiddlewareOrderModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/middleware-order"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "middleware:global",
            "middleware:module",
            "middleware:controller",
            "middleware:route",
            "pipe",
            "guard",
            "before:route",
            "handler",
            "after:route"
        ]
    );
}

#[tokio::test]
async fn route_scoped_middleware_only_applies_to_that_route() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/scoped", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.header("x-route").unwrap_or("missing"),
                ))
            })
            .unwrap()
            .with_middleware(|request: BootRequest| async move {
                Ok(MiddlewareOutcome::next(
                    request.with_header("x-route", "scoped"),
                ))
            }),
        )
        .route(
            RouteDefinition::get("/plain", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.header("x-route").unwrap_or("missing"),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let scoped = app
        .call(BootRequest::new(HttpMethod::Get, "/scoped"))
        .await
        .unwrap();
    let plain = app
        .call(BootRequest::new(HttpMethod::Get, "/plain"))
        .await
        .unwrap();

    assert_eq!(scoped.body, b"scoped");
    assert_eq!(plain.body, b"missing");
}

#[derive(Debug)]
struct GuardedControllerModule;

impl Module for GuardedControllerModule {
    fn name(&self) -> &'static str {
        "guarded-controller"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/items")?
            .get("/", |_| async { Ok(BootResponse::text("ok")) })?])
    }
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
        .import(GuardedControllerModule)
        .build()
        .unwrap();

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();

    assert_eq!(response.status, 403);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/items: request was forbidden: GET /items"
    );
}

#[tokio::test]
async fn application_call_method_not_allowed_errors_use_route_filters() {
    let app = BootApplication::builder()
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(405),
            ))
        })
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await
        .unwrap();

    assert_eq!(response.status, 405);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/items/{id}: method is not allowed: POST /items/hammer"
    );
}

#[tokio::test]
async fn application_call_path_param_decode_errors_use_route_filters() {
    let app = BootApplication::builder()
        .use_global_filter(|context: ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(400),
            ))
        })
        .route(
            RouteDefinition::get("/files/{path}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await
        .unwrap();

    assert_eq!(response.status, 400);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/files/{path}: bad request: invalid percent encoding: %ZZ"
    );
}

#[tokio::test]
async fn guard_forbidden_errors_use_actual_request_path() {
    let route = RouteDefinition::get("/items/{id}", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap()
    .with_guard(|_| async { Ok(false) });

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::Forbidden(message) if message == "GET /items/hammer"
    ));
}

#[tokio::test]
async fn guard_unauthorized_errors_use_route_filters() {
    let route = RouteDefinition::get("/private", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap()
    .with_guard(|context: ExecutionContext| async move {
        let _token = context.request.require_bearer_token()?;
        Ok(true)
    })
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("{}: {error}", context.route_path)).with_status(401),
        ))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/private"))
        .await
        .unwrap();
    let ok = route
        .call(
            BootRequest::new(HttpMethod::Get, "/private")
                .with_header("authorization", "Bearer token-123"),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 401);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/private: request was unauthorized: missing bearer token"
    );
    assert_eq!(ok.body, b"unreachable");
}

#[tokio::test]
async fn guards_can_read_route_metadata_from_execution_context() {
    let route = RouteDefinition::get("/admin", |_| async { Ok(BootResponse::text("admin")) })
        .unwrap()
        .with_metadata_value("roles", json!(["admin"]))
        .with_guard(|context: ExecutionContext| async move {
            let roles = context
                .metadata_as::<Vec<String>>("roles")?
                .unwrap_or_default();
            Ok(roles.iter().any(|role| role == "admin"))
        });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/admin"))
        .await
        .unwrap();

    assert_eq!(response.body, b"admin");
}

#[tokio::test]
async fn guard_session_cookie_errors_use_route_filters() {
    let route = RouteDefinition::get("/session", |_| async {
        Ok(BootResponse::text("session ok"))
    })
    .unwrap()
    .with_guard(|context: ExecutionContext| async move {
        let _session = context.request.require_cookie("session")?;
        Ok(true)
    })
    .with_filter(|context: ExecutionContext, error: BootError| async move {
        Ok(Some(
            BootResponse::text(format!("{}: {error}", context.route_path)).with_status(401),
        ))
    });

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/session"))
        .await
        .unwrap();
    let ok = route
        .call(BootRequest::new(HttpMethod::Get, "/session").with_header("cookie", "session=abc"))
        .await
        .unwrap();

    assert_eq!(response.status, 401);
    assert_eq!(
        String::from_utf8(response.body).unwrap(),
        "/session: request was unauthorized: missing cookie: session"
    );
    assert_eq!(ok.body, b"session ok");
}
