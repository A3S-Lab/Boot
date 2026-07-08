#![cfg(feature = "axum")]

use a3s_boot::{
    ApiVersioning, AxumAdapter, BootApplication, BootError, BootRequest, BootResponse,
    ExecutionContext, HttpMethod, MiddlewareOutcome, RouteDefinition, SseEvent, StreamableFile,
    WebSocketGatewayDefinition, WebSocketMessage,
};
use std::sync::Arc;

#[test]
fn axum_method_conversion_rejects_unsupported_methods() {
    use axum::http::Method;

    let error = HttpMethod::try_from(Method::TRACE).unwrap_err();

    assert!(matches!(
        error,
        BootError::MethodNotAllowed(message) if message == "TRACE"
    ));
}

#[tokio::test]
async fn axum_adapter_serves_multiple_methods_on_the_same_path() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("list")) }).unwrap(),
        )
        .route(
            RouteDefinition::post("/items", |_| async { Ok(BootResponse::text("create")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let post_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(post_response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(get_response.into_body(), 1024).await.unwrap(),
        "list"
    );
    assert_eq!(
        to_bytes(post_response.into_body(), 1024).await.unwrap(),
        "create"
    );
}

#[tokio::test]
async fn axum_adapter_dispatches_all_routes_with_exact_method_precedence() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::all("/items", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "all:{}",
                    request.method().as_str()
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("exact:get")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let post_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let patch_response = router
        .oneshot(
            Request::builder()
                .method("PATCH")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(post_response.status(), StatusCode::OK);
    assert_eq!(patch_response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(get_response.into_body(), 1024).await.unwrap(),
        "exact:get"
    );
    assert_eq!(
        to_bytes(post_response.into_body(), 1024).await.unwrap(),
        "all:POST"
    );
    assert_eq!(
        to_bytes(patch_response.into_body(), 1024).await.unwrap(),
        "all:PATCH"
    );
}

#[tokio::test]
async fn axum_adapter_dispatches_host_scoped_routes_with_duplicate_paths() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "tenant:{}",
                    request.host_param("tenant").unwrap_or("missing")
                )))
            })
            .unwrap()
            .with_host("{tenant}.example.com")
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("api")) })
                .unwrap()
                .with_host("api.internal.test")
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let tenant_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items")
                .header("host", "acme.example.com")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let api_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items")
                .header("host", "api.internal.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let missing_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items")
                .header("host", "other.test")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(tenant_response.status(), StatusCode::OK);
    assert_eq!(api_response.status(), StatusCode::OK);
    assert_eq!(missing_response.status(), StatusCode::NOT_FOUND);
    assert_eq!(
        to_bytes(tenant_response.into_body(), 1024).await.unwrap(),
        "tenant:acme"
    );
    assert_eq!(
        to_bytes(api_response.into_body(), 1024).await.unwrap(),
        "api"
    );
}

#[tokio::test]
async fn axum_adapter_serves_multiple_methods_on_the_same_path_shape() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "get:{}",
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/{slug}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "post:{}",
                    request.param("slug").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let get_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items/hammer")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let post_response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items/anvil")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(get_response.status(), StatusCode::OK);
    assert_eq!(post_response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(get_response.into_body(), 1024).await.unwrap(),
        "get:hammer"
    );
    assert_eq!(
        to_bytes(post_response.into_body(), 1024).await.unwrap(),
        "post:anvil"
    );
}

#[tokio::test]
async fn axum_adapter_prefers_static_routes_over_param_routes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "dynamic:{}",
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/items/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "static");
}

#[tokio::test]
async fn axum_adapter_dispatches_catch_all_routes_after_static_and_param_routes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{*path}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "catch:{}",
                    request.param("path").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/files/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "param:{}",
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/files/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let static_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/files/new")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let param_response = router
        .clone()
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/files/readme")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let catch_response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/files/docs/readme%2Emd")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(static_response.status(), StatusCode::OK);
    assert_eq!(param_response.status(), StatusCode::OK);
    assert_eq!(catch_response.status(), StatusCode::OK);
    assert_eq!(
        to_bytes(static_response.into_body(), 1024).await.unwrap(),
        "static"
    );
    assert_eq!(
        to_bytes(param_response.into_body(), 1024).await.unwrap(),
        "param:readme"
    );
    assert_eq!(
        to_bytes(catch_response.into_body(), 1024).await.unwrap(),
        "catch:docs/readme.md"
    );
}

#[tokio::test]
async fn axum_adapter_dispatches_uri_versioned_routes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::uri())
        .route(
            RouteDefinition::get("/cats/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.param("id").unwrap_or("missing").to_string(),
                ))
            })
            .unwrap()
            .with_version("1"),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/v1/cats/milo")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "milo");
}

#[tokio::test]
async fn axum_adapter_dispatches_header_versioned_routes_with_duplicate_shapes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version"))
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v2")) })
                .unwrap()
                .with_version("2"),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/cats")
                .header("x-api-version", "2")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "v2");
}

#[tokio::test]
async fn axum_adapter_preserves_actual_head_request_method() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::ALLOW, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(RouteDefinition::get("/probe", |_| async { Ok(BootResponse::text("get")) }).unwrap())
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/probe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let allow = response
        .headers()
        .get(ALLOW)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(allow.as_deref(), Some("GET"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn axum_adapter_uses_boot_method_not_allowed_fallback() {
    use axum::body::{to_bytes, Body};
    use axum::http::{
        header::{ALLOW, CONTENT_TYPE},
        Request, StatusCode,
    };
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(RouteDefinition::get("/probe", |_| async { Ok(BootResponse::text("get")) }).unwrap())
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/probe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let allow = response
        .headers()
        .get(ALLOW)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(allow.as_deref(), Some("GET"));
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body, "POST /probe");
}

#[tokio::test]
async fn axum_method_not_allowed_allow_header_uses_exact_boot_methods() {
    use axum::body::Body;
    use axum::http::{header::ALLOW, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("list")) }).unwrap(),
        )
        .route(
            RouteDefinition::post("/items", |_| async { Ok(BootResponse::text("create")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("PUT")
                .uri("/items")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let allow = response
        .headers()
        .get(ALLOW)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);

    assert_eq!(response.status(), StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(allow.as_deref(), Some("GET,POST"));
}

#[tokio::test]
async fn axum_method_not_allowed_fallback_uses_route_filters() {
    use axum::body::{to_bytes, Body};
    use axum::http::{
        header::{ALLOW, CONTENT_TYPE},
        Request, StatusCode,
    };
    use tower::ServiceExt;

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
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/items/hammer")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let allow = response
        .headers()
        .get(ALLOW)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::METHOD_NOT_ALLOWED);
    assert_eq!(allow.as_deref(), Some("GET"));
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(
        body,
        "/items/{id}: method is not allowed: POST /items/hammer"
    );
}

#[tokio::test]
async fn axum_adapter_serves_explicit_head_routes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(RouteDefinition::get("/probe", |_| async { Ok(BootResponse::text("get")) }).unwrap())
        .route(
            RouteDefinition::head("/probe", |_| async {
                Ok(BootResponse::text("head body").with_header("x-head", "yes"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/probe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let header = response
        .headers()
        .get("x-head")
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(header.as_deref(), Some("yes"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn axum_adapter_validates_head_responses_before_stripping_bodies() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::head("/probe", |_| async {
                Ok(BootResponse::text("invalid").with_status(204))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/probe")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.is_empty());
}

#[tokio::test]
async fn axum_adapter_preserves_repeated_response_headers() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/cookies", |_| async {
                Ok(BootResponse::text("ok")
                    .append_header("Set-Cookie", "session=abc; Path=/")
                    .append_header("Set-Cookie", "theme=dark; Path=/"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/cookies")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    let cookies = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .map(|value| value.to_str().unwrap())
        .collect::<Vec<_>>();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(cookies, ["session=abc; Path=/", "theme=dark; Path=/"]);
}

#[tokio::test]
async fn axum_adapter_maps_body_limit_failures_to_payload_too_large() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |request: BootRequest| async move {
                Ok(BootResponse::text(request.text()?))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .body(Body::from("too large"))
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn axum_adapter_rejects_oversized_content_length_before_reading_body() {
    use axum::body::Body;
    use axum::http::{header::CONTENT_LENGTH, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header(CONTENT_LENGTH, "5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn axum_adapter_validates_requests_before_middleware() {
    use axum::body::Body;
    use axum::http::{header::CONTENT_LENGTH, Request, StatusCode};
    use tower::ServiceExt;

    let calls = Arc::new(std::sync::Mutex::new(0usize));
    let middleware_calls = Arc::clone(&calls);
    let app = BootApplication::builder()
        .use_global_middleware(move |request: BootRequest| {
            let middleware_calls = Arc::clone(&middleware_calls);
            async move {
                *middleware_calls.lock().unwrap() += 1;
                Ok(MiddlewareOutcome::next(request))
            }
        })
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header(CONTENT_LENGTH, "5")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    assert_eq!(*calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header(CONTENT_LENGTH, "nope")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "invalid content-length header: nope");
}

#[tokio::test]
async fn axum_adapter_accepts_matching_repeated_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, HeaderValue, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |request: BootRequest| async move {
                Ok(BootResponse::text(request.text()?))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let mut request = Request::builder()
        .method("POST")
        .uri("/echo")
        .body(Body::from("data"))
        .unwrap();
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("4"));
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("4"));

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "data");
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_repeated_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, HeaderValue, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(4))
        .unwrap();

    let mut request = Request::builder()
        .method("POST")
        .uri("/echo")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("4"));
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("nope"));

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "invalid content-length header: nope");
}

#[tokio::test]
async fn axum_adapter_rejects_conflicting_repeated_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, HeaderValue, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(8))
        .unwrap();

    let mut request = Request::builder()
        .method("POST")
        .uri("/echo")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("4"));
    request
        .headers_mut()
        .append(CONTENT_LENGTH, HeaderValue::from_static("5"));

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(body, "conflicting content-length headers: 4 != 5");
}

#[tokio::test]
async fn axum_adapter_rejects_shorter_bodies_than_declared_content_length() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(8))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header(CONTENT_LENGTH, "5")
                .body(Body::from("data"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        "content-length header does not match request body length: expected 5, got 4"
    );
}

#[tokio::test]
async fn axum_adapter_rejects_longer_bodies_than_declared_content_length() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_LENGTH, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/echo", |_| async { Ok(BootResponse::text("unreachable")) })
                .unwrap(),
        )
        .build()
        .unwrap();
    let router = app
        .into_adapter(&AxumAdapter::new().with_body_limit(8))
        .unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/echo")
                .header(CONTENT_LENGTH, "4")
                .body(Body::from("data!"))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(
        body,
        "content-length header does not match request body length: expected 4, got 5"
    );
}

#[tokio::test]
async fn axum_adapter_error_responses_are_plain_text() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad", |_| async {
                Err(a3s_boot::BootError::BadRequest("invalid input".to_string()))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body, "invalid input");
}

#[tokio::test]
async fn axum_adapter_maps_unsupported_json_content_type_to_unsupported_media_type() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    #[derive(Debug, serde::Deserialize, serde::Serialize)]
    struct ItemDto {
        name: String,
    }

    let app = BootApplication::builder()
        .route(RouteDefinition::post_json("/", |dto: ItemDto| async move { Ok(dto) }).unwrap())
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/")
                .header(CONTENT_TYPE, "text/plain")
                .body(Body::from(r#"{"name":"Hammer"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::UNSUPPORTED_MEDIA_TYPE);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body, "expected JSON content type, got text/plain");
}

#[tokio::test]
async fn axum_adapter_maps_unaccepted_json_responses_to_not_acceptable() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    #[derive(Debug, serde::Serialize)]
    struct ItemDto {
        name: String,
    }

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get_json("/", |_| async {
                Ok(ItemDto {
                    name: "Hammer".to_string(),
                })
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/")
                .header("accept", "text/plain")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::NOT_ACCEPTABLE);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body, "expected client to accept JSON response");
}

#[tokio::test]
async fn axum_adapter_streams_sse_responses() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::sse("/events", |_| async {
                Ok(futures_util::stream::iter([
                    Ok::<_, BootError>(SseEvent::new("ready").with_event("app.ready")),
                    Ok::<_, BootError>(
                        SseEvent::json(&serde_json::json!({
                            "name": "Milo"
                        }))?
                        .with_id("cat-1"),
                    ),
                ]))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/events")
                .header("accept", "text/event-stream")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(
        content_type.as_deref(),
        Some("text/event-stream; charset=utf-8")
    );
    assert_eq!(
        body,
        "event: app.ready\ndata: ready\n\nid: cat-1\ndata: {\"name\":\"Milo\"}\n\n"
    );
}

#[tokio::test]
async fn axum_adapter_registers_websocket_gateway_routes() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let gateway = WebSocketGatewayDefinition::new("/ws")
        .unwrap()
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })
        .unwrap();
    let app = BootApplication::builder().gateway(gateway).build().unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ws")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_adapter_registers_catch_all_websocket_gateway_routes() {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let gateway = WebSocketGatewayDefinition::new("/ws/{*topic}")
        .unwrap()
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })
        .unwrap();
    let app = BootApplication::builder().gateway(gateway).build().unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/ws/rooms/general")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn axum_adapter_uses_boot_not_found_fallback() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(RouteDefinition::get("/known", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert_eq!(body, "GET /missing");
}

#[tokio::test]
async fn axum_adapter_strips_head_not_found_fallback_bodies() {
    use axum::body::{to_bytes, Body};
    use axum::http::{header::CONTENT_TYPE, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(RouteDefinition::get("/known", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("HEAD")
                .uri("/missing")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let content_type = response
        .headers()
        .get(CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .map(str::to_string);
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::NOT_FOUND);
    assert_eq!(content_type.as_deref(), Some("text/plain; charset=utf-8"));
    assert!(body.is_empty());
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_response_header_names() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-header", |_| async {
                Ok(BootResponse::text("ok").with_header("bad header", "value"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-header")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("invalid response header name"));
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_response_header_values() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-header", |_| async {
                Ok(BootResponse::text("ok").with_header("x-mode", "fast\nslow"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-header")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("invalid response header value"));
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_response_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-content-length", |_| async {
                Ok(BootResponse::text("ok").with_header("Content-Length", "nope"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-content-length")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("invalid response content-length header: nope"));
}

#[tokio::test]
async fn axum_adapter_rejects_conflicting_response_content_length_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-content-length", |_| async {
                Ok(BootResponse::text("ok")
                    .with_content_length(2)
                    .append_header("Content-Length", "3"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-content-length")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("conflicting response content-length headers: 2 != 3"));
}

#[tokio::test]
async fn axum_adapter_rejects_response_content_length_mismatches() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-content-length", |_| async {
                Ok(BootResponse::text("ok").with_content_length(3))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-content-length")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains(
        "response content-length header does not match response body length: expected 3, got 2"
    ));
}

#[tokio::test]
async fn axum_adapter_rejects_bodies_on_no_body_response_statuses() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/informational", |_| async {
                Ok(BootResponse::text("processing").with_status(102))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/no-content", |_| async {
                Ok(BootResponse::text("not empty").with_status(204))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/not-modified", |_| async {
                Ok(BootResponse::text("cached").with_status(304))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    for (path, status) in [
        ("/informational", 102),
        ("/no-content", 204),
        ("/not-modified", 304),
    ] {
        let response = router
            .clone()
            .oneshot(
                Request::builder()
                    .method("GET")
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let response_status = response.status();
        let body = to_bytes(response.into_body(), 1024).await.unwrap();
        let body = String::from_utf8(body.to_vec()).unwrap();

        assert_eq!(response_status, StatusCode::INTERNAL_SERVER_ERROR);
        assert!(body.contains(&format!("response status {status} must not include a body")));
    }
}

#[tokio::test]
async fn axum_adapter_streams_file_responses() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/download", |_| async {
                Ok(BootResponse::streamable_file(
                    StreamableFile::stream(futures_util::stream::iter([
                        Ok(Vec::from("hello ")),
                        Ok(Vec::from("file")),
                    ]))
                    .with_content_type("text/plain; charset=utf-8")
                    .with_content_length(10)
                    .with_attachment("hello.txt")
                    .unwrap(),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/download")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response.headers().get("content-type").unwrap(),
        "text/plain; charset=utf-8"
    );
    assert_eq!(response.headers().get("content-length").unwrap(), "10");
    assert_eq!(
        response.headers().get("content-disposition").unwrap(),
        r#"attachment; filename="hello.txt""#
    );
    assert_eq!(
        to_bytes(response.into_body(), 1024).await.unwrap(),
        "hello file"
    );
}

#[tokio::test]
async fn axum_adapter_rejects_invalid_response_status_codes() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/bad-status", |_| async {
                Ok(BootResponse::text("ok").with_status(99))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let response = router
        .oneshot(
            Request::builder()
                .method("GET")
                .uri("/bad-status")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::INTERNAL_SERVER_ERROR);
    assert!(body.contains("invalid response status"));
}

#[tokio::test]
async fn axum_adapter_preserves_repeated_request_headers() {
    use axum::body::{to_bytes, Body};
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/headers", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.header_values("x-mode").join(","),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let mut request = Request::builder()
        .method("GET")
        .uri("/headers")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .append("x-mode", "fast".parse().unwrap());
    request
        .headers_mut()
        .append("x-mode", "safe".parse().unwrap());

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();

    assert_eq!(status, StatusCode::OK);
    assert_eq!(body, "fast,safe");
}

#[tokio::test]
async fn axum_adapter_rejects_non_text_request_header_values() {
    use axum::body::{to_bytes, Body};
    use axum::http::{HeaderValue, Request, StatusCode};
    use tower::ServiceExt;

    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/headers", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let router = app.into_adapter(&AxumAdapter::new()).unwrap();

    let mut request = Request::builder()
        .method("GET")
        .uri("/headers")
        .body(Body::empty())
        .unwrap();
    request
        .headers_mut()
        .insert("x-mode", HeaderValue::from_bytes(b"fast\xffslow").unwrap());

    let response = router.oneshot(request).await.unwrap();
    let status = response.status();
    let body = to_bytes(response.into_body(), 1024).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();

    assert_eq!(status, StatusCode::BAD_REQUEST);
    assert!(body.contains("invalid request header value"));
}
