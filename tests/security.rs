#![cfg(feature = "security")]

use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, CorsOptions, CsrfOptions,
    HttpMethod, InMemoryRateLimitProvider, RateLimitDecision, RateLimitOptions, RateLimitProvider,
    RateLimitRequest, Result, RouteDefinition, SecurityHeadersOptions,
};
use serde_json::json;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
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

#[derive(Clone, Default)]
struct SharedRateLimitProvider {
    counts: Arc<Mutex<BTreeMap<(String, String), u32>>>,
    requests: Arc<Mutex<Vec<RateLimitRequest>>>,
}

impl RateLimitProvider for SharedRateLimitProvider {
    fn acquire(&self, request: RateLimitRequest) -> BoxFuture<'static, Result<RateLimitDecision>> {
        let counts = Arc::clone(&self.counts);
        let requests = Arc::clone(&self.requests);
        Box::pin(async move {
            requests.lock().unwrap().push(request.clone());
            let key = (
                request.policy_id().to_string(),
                request.subject_hash().to_string(),
            );
            let mut counts = counts.lock().unwrap();
            let count = counts.entry(key).or_default();
            if *count >= request.max_requests() {
                return Ok(RateLimitDecision::Limited);
            }
            *count += 1;
            Ok(RateLimitDecision::Allowed)
        })
    }
}

fn limited_app<P>(provider: P, policy_id: &str, max_requests: u32) -> BootApplication
where
    P: RateLimitProvider,
{
    BootApplication::builder()
        .use_global_rate_limit_provider(
            RateLimitOptions::new()
                .with_policy_id(policy_id)
                .with_max_requests(max_requests)
                .with_window(Duration::from_secs(60)),
            provider,
        )
        .route(
            RouteDefinition::get("/limited", |_| async { Ok(BootResponse::text("ok")) }).unwrap(),
        )
        .build()
        .unwrap()
}

#[tokio::test]
async fn public_rate_limit_provider_shares_state_without_receiving_credentials() {
    let provider = SharedRateLimitProvider::default();
    let first_process = limited_app(provider.clone(), "cloud-api", 1);
    let second_process = limited_app(provider.clone(), "cloud-api", 1);
    let request = || {
        BootRequest::new(HttpMethod::Get, "/limited")
            .with_header("authorization", "Bearer tenant-secret-token")
    };

    assert_eq!(first_process.call(request()).await.unwrap().status(), 200);
    assert_eq!(second_process.handle(request()).await.status(), 429);

    let requests = provider.requests.lock().unwrap();
    assert_eq!(requests.len(), 2);
    assert!(requests
        .iter()
        .all(|request| request.policy_id() == "cloud-api"));
    assert!(requests
        .iter()
        .all(|request| request.subject_hash().len() == 64));
    assert!(requests
        .iter()
        .all(|request| !request.subject_hash().contains("tenant-secret-token")));
    assert_eq!(requests[0].subject_hash(), requests[1].subject_hash());
}

#[tokio::test]
async fn in_memory_provider_rejects_conflicting_clients_for_one_policy() {
    let provider = InMemoryRateLimitProvider::new();
    let first_client = limited_app(provider.clone(), "cloud-api", 1);
    let conflicting_client = limited_app(provider, "cloud-api", 2);

    assert_eq!(
        first_client
            .call(BootRequest::new(HttpMethod::Get, "/limited"))
            .await
            .unwrap()
            .status(),
        200
    );

    let error = conflicting_client
        .call(BootRequest::new(HttpMethod::Get, "/limited"))
        .await
        .unwrap_err();
    assert!(matches!(error, BootError::Internal(_)));
}

#[derive(Clone)]
struct UnavailableRateLimitProvider;

impl RateLimitProvider for UnavailableRateLimitProvider {
    fn acquire(&self, _request: RateLimitRequest) -> BoxFuture<'static, Result<RateLimitDecision>> {
        Box::pin(async {
            Err(BootError::ServiceUnavailable(
                "rate limit provider unavailable".to_string(),
            ))
        })
    }
}

#[tokio::test]
async fn provider_failure_rejects_work_instead_of_bypassing_the_limit() {
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let handler_calls_for_route = Arc::clone(&handler_calls);
    let app = BootApplication::builder()
        .use_global_rate_limit_provider(
            RateLimitOptions::new().with_policy_id("cloud-api"),
            UnavailableRateLimitProvider,
        )
        .route(
            RouteDefinition::get("/limited", move |_| {
                let handler_calls = Arc::clone(&handler_calls_for_route);
                async move {
                    handler_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(BootResponse::text("ok"))
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .handle(BootRequest::new(HttpMethod::Get, "/limited"))
        .await;
    assert_eq!(response.status(), 503);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
}
