#![cfg(feature = "http-client")]

use a3s_boot::{
    BootApplication, BoxFuture, CircuitBreakerOptions, CircuitState, DegradePolicy,
    FallbackResponse, HttpClientBackend, HttpClientRequest, HttpClientResponse, HttpService,
    Result, RetryOptions, ServiceClient, ServiceClientModule, ServiceClientOptions,
    ServiceInstance, StaticServiceDiscovery,
};
use serde_json::json;
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[derive(Clone)]
struct FlakyBackend {
    failures_before_success: Arc<Mutex<u32>>,
    requests: Arc<Mutex<Vec<HttpClientRequest>>>,
}

impl FlakyBackend {
    fn new(failures_before_success: u32) -> Self {
        Self {
            failures_before_success: Arc::new(Mutex::new(failures_before_success)),
            requests: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn request_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

impl HttpClientBackend for FlakyBackend {
    fn send(&self, request: HttpClientRequest) -> BoxFuture<'static, Result<HttpClientResponse>> {
        self.requests.lock().unwrap().push(request);
        let remaining = self.failures_before_success.clone();
        Box::pin(async move {
            let mut guard = remaining.lock().unwrap();
            if *guard > 0 {
                *guard -= 1;
                Ok(HttpClientResponse::new(503, br#"{"error":"unavailable"}"#))
            } else {
                HttpClientResponse::json(&json!({ "ok": true }))
            }
        })
    }
}

#[tokio::test]
async fn service_client_discovers_retries_and_succeeds() {
    let discovery = StaticServiceDiscovery::new();
    discovery
        .upsert(ServiceInstance::new("billing", "1", "127.0.0.1", 9000))
        .unwrap();
    let backend = FlakyBackend::new(2);
    let client = ServiceClient::new(
        HttpService::from_backend(backend.clone(), Default::default()),
        Arc::new(discovery),
        ServiceClientOptions::new()
            .without_degrade()
            .with_retry(
                RetryOptions::new()
                    .with_max_retries(3)
                    .with_initial_delay(Duration::from_millis(1)),
            )
            .with_circuit_breaker(
                CircuitBreakerOptions::new()
                    .with_failure_threshold(10)
                    .with_cooldown(Duration::from_secs(1)),
            ),
    );

    let response = client.get("billing", "/invoice").await.unwrap();
    assert_eq!(response.status(), 200);
    assert_eq!(backend.request_count(), 3);
    assert!(response.body_text().unwrap().contains("\"ok\":true"));
}

#[tokio::test]
async fn service_client_degrades_when_circuit_is_open() {
    let discovery = StaticServiceDiscovery::new();
    discovery
        .upsert(ServiceInstance::new("billing", "1", "127.0.0.1", 9000))
        .unwrap();
    let backend = FlakyBackend::new(100);
    let client = ServiceClient::new(
        HttpService::from_backend(backend, Default::default()),
        Arc::new(discovery),
        ServiceClientOptions::new()
            .with_retry(RetryOptions::disabled())
            .with_circuit_breaker(
                CircuitBreakerOptions::new()
                    .with_failure_threshold(1)
                    .with_cooldown(Duration::from_secs(60)),
            )
            .with_degrade(DegradePolicy::new(FallbackResponse::json(
                200,
                br#"{"degraded":true}"#.to_vec(),
            ))),
    );

    let first = client.get("billing", "/invoice").await.unwrap();
    assert_eq!(first.status(), 200);
    assert_eq!(first.header("x-a3s-degraded"), Some("1"));
    assert_eq!(client.circuit_state(), CircuitState::Open);

    let second = client.get("billing", "/invoice").await.unwrap();
    assert_eq!(second.header("x-a3s-degraded"), Some("1"));
}

#[tokio::test]
async fn service_client_propagates_trace_and_request_ids() {
    use a3s_boot::{BootRequest, HttpMethod, RequestContext};

    let discovery = StaticServiceDiscovery::new();
    discovery
        .upsert(ServiceInstance::new("billing", "1", "127.0.0.1", 9000))
        .unwrap();
    let backend = FlakyBackend::new(0);
    let client = ServiceClient::new(
        HttpService::from_backend(backend.clone(), Default::default()),
        Arc::new(discovery),
        ServiceClientOptions::new().without_degrade(),
    );

    let mut inbound = BootRequest::new(HttpMethod::Get, "/api/demo");
    inbound.headers.insert(
        "traceparent".into(),
        "00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01".into(),
    );
    inbound
        .headers
        .insert("x-request-id".into(), "req-42".into());
    let ctx =
        RequestContext::from_route_request(&inbound, "/api/demo", None, None, Default::default());
    RequestContext::scope(ctx, async {
        let _ = client.get("billing", "/invoice").await.unwrap();
    })
    .await;

    let captured = backend.requests.lock().unwrap();
    let req = captured.last().expect("outbound request");
    assert_eq!(
        req.header("traceparent"),
        Some("00-aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa-bbbbbbbbbbbbbbbb-01")
    );
    assert_eq!(req.header("x-request-id"), Some("req-42"));
    assert_eq!(req.header("x-correlation-id"), Some("req-42"));
}

#[tokio::test]
async fn service_client_module_registers_provider() {
    let discovery = StaticServiceDiscovery::new();
    discovery
        .upsert(ServiceInstance::new("catalog", "1", "127.0.0.1", 8080))
        .unwrap();
    let backend = FlakyBackend::new(0);
    let app = BootApplication::builder()
        .import(
            ServiceClientModule::new("service-client", Arc::new(discovery))
                .with_backend(backend)
                .with_client_options(ServiceClientOptions::new().without_degrade())
                .global(),
        )
        .build()
        .unwrap();

    let client = app.get::<ServiceClient>().unwrap();
    let body = client
        .get_json::<serde_json::Value>("catalog", "/items")
        .await
        .unwrap();
    assert_eq!(body, json!({ "ok": true }));
}
