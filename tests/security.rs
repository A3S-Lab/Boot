#![cfg(feature = "security")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, CorsOptions, CsrfOptions, HttpMethod,
    RateLimitOptions, RouteDefinition, SecurityHeadersOptions,
};
use serde_json::json;
use std::time::Duration;

#[tokio::test]
async fn cors_preflight_routes_are_generated_for_registered_routes() {
    let app = BootApplication::builder()
        .use_global_cors(
            CorsOptions::new()
                .allow_origin("https://app.example")
                .allow_methods([HttpMethod::Get, HttpMethod::Post])
                .allow_headers(["content-type", "x-csrf-token"])
                .with_max_age(600),
        )
        .route(RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Options, "/cats")
                .with_header("origin", "https://app.example")
                .with_header("access-control-request-method", "GET")
                .with_header("access-control-request-headers", "Content-Type"),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 204);
    assert_eq!(
        response.header("access-control-allow-origin"),
        Some("https://app.example")
    );
    assert_eq!(
        response.header("access-control-allow-methods"),
        Some("GET,POST")
    );
    assert_eq!(
        response.header("access-control-allow-headers"),
        Some("content-type,x-csrf-token")
    );
    assert_eq!(response.header("access-control-max-age"), Some("600"));
    assert_eq!(
        response.header("vary"),
        Some("origin, access-control-request-method, access-control-request-headers")
    );
}

#[tokio::test]
async fn cors_actual_responses_include_allowed_origin_headers() {
    let app = BootApplication::builder()
        .use_global_cors(
            CorsOptions::new()
                .allow_origin("https://app.example")
                .allow_credentials()
                .expose_header("x-request-id"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async {
                BootResponse::json(&json!({ "name": "Milo" }))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/cats").with_header("origin", "https://app.example"),
        )
        .await
        .unwrap();

    assert_eq!(
        response.header("access-control-allow-origin"),
        Some("https://app.example")
    );
    assert_eq!(
        response.header("access-control-allow-credentials"),
        Some("true")
    );
    assert_eq!(
        response.header("access-control-expose-headers"),
        Some("x-request-id")
    );
    assert_eq!(response.header("vary"), Some("origin"));
}

#[tokio::test]
async fn security_headers_interceptor_adds_defaults_without_overwriting_handlers() {
    let app = BootApplication::builder()
        .use_global_security_headers(
            SecurityHeadersOptions::new()
                .with_content_security_policy("default-src 'self'")
                .with_strict_transport_security("max-age=31536000"),
        )
        .route(
            RouteDefinition::get("/secure", |_| async {
                Ok(BootResponse::text("ok").with_header("x-frame-options", "SAMEORIGIN"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/secure"))
        .await
        .unwrap();

    assert_eq!(response.header("x-content-type-options"), Some("nosniff"));
    assert_eq!(response.header("x-frame-options"), Some("SAMEORIGIN"));
    assert_eq!(response.header("referrer-policy"), Some("no-referrer"));
    assert_eq!(
        response.header("cross-origin-opener-policy"),
        Some("same-origin")
    );
    assert_eq!(
        response.header("cross-origin-resource-policy"),
        Some("same-origin")
    );
    assert_eq!(
        response.header("content-security-policy"),
        Some("default-src 'self'")
    );
    assert_eq!(
        response.header("strict-transport-security"),
        Some("max-age=31536000")
    );
}

#[tokio::test]
async fn csrf_guard_allows_matching_tokens_and_rejects_missing_tokens() {
    let app = BootApplication::builder()
        .use_global_csrf(CsrfOptions::new())
        .route(
            RouteDefinition::post("/cats", |_| async { Ok(BootResponse::text("created")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let allowed = app
        .call(
            BootRequest::new(HttpMethod::Post, "/cats")
                .with_header("x-csrf-token", "token-1")
                .with_header("cookie", "csrf-token=token-1"),
        )
        .await
        .unwrap();
    let rejected = app
        .handle(BootRequest::new(HttpMethod::Post, "/cats"))
        .await;

    assert_eq!(allowed.status(), 200);
    assert_eq!(allowed.body_text().unwrap(), "created");
    assert_eq!(rejected.status(), 403);
    assert_eq!(
        rejected.body_json::<serde_json::Value>().unwrap(),
        json!({
            "statusCode": 403,
            "message": "invalid CSRF token",
            "error": "Forbidden"
        })
    );
}

#[tokio::test]
async fn rate_limit_guard_rejects_requests_after_the_window_limit() {
    let app = BootApplication::builder()
        .use_global_rate_limit(
            RateLimitOptions::new()
                .with_max_requests(2)
                .with_window(Duration::from_secs(60))
                .with_key_header("x-user-id")
                .without_bearer_token(),
        )
        .route(
            RouteDefinition::get("/limited", |_| async { Ok(BootResponse::text("ok")) }).unwrap(),
        )
        .build()
        .unwrap();

    for _ in 0..2 {
        let response = app
            .call(BootRequest::new(HttpMethod::Get, "/limited").with_header("x-user-id", "u1"))
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
    }

    let rejected = app
        .handle(BootRequest::new(HttpMethod::Get, "/limited").with_header("x-user-id", "u1"))
        .await;
    let separate_key = app
        .call(BootRequest::new(HttpMethod::Get, "/limited").with_header("x-user-id", "u2"))
        .await
        .unwrap();

    assert_eq!(rejected.status(), 429);
    assert_eq!(
        rejected.body_json::<serde_json::Value>().unwrap(),
        json!({
            "statusCode": 429,
            "message": "rate limit exceeded",
            "error": "Too Many Requests"
        })
    );
    assert_eq!(separate_key.status(), 200);
}
