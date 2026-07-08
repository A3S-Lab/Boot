#![cfg(feature = "http-client")]

use a3s_boot::{
    BootApplication, BoxFuture, HttpClientBackend, HttpClientOptions, HttpClientRequest,
    HttpClientResponse, HttpMethod, HttpModule, HttpService, Result,
};
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[tokio::test]
async fn http_module_exports_service_and_applies_default_request_options() {
    let backend =
        RecordingBackend::new(HttpClientResponse::json(&json!({ "name": "Milo" })).unwrap());
    let app = BootApplication::builder()
        .import(HttpModule::with_backend_and_options(
            "http",
            backend.clone(),
            HttpClientOptions::new()
                .with_base_url("https://api.example/v1")
                .with_header("x-api-key", "secret")
                .with_timeout(Duration::from_secs(3)),
        ))
        .build()
        .unwrap();

    let client = app.get::<HttpService>().unwrap();
    let cat: Value = client.get_json("/cats/1").await.unwrap();
    let request = backend.requests()[0].clone();

    assert_eq!(cat, json!({ "name": "Milo" }));
    assert_eq!(request.method(), HttpMethod::Get);
    assert_eq!(request.url(), "https://api.example/v1/cats/1");
    assert_eq!(request.header("x-api-key"), Some("secret"));
    assert_eq!(request.timeout(), Some(Duration::from_secs(3)));
}

#[tokio::test]
async fn http_service_json_helpers_send_and_decode_json_bodies() {
    let backend = RecordingBackend::new(
        HttpClientResponse::json_with_status(201, &json!({ "id": 7 })).unwrap(),
    );
    let client = HttpService::from_backend(backend.clone(), HttpClientOptions::default());

    let value: Value = client
        .post_json("https://api.example/cats", &json!({ "name": "Milo" }))
        .await
        .unwrap();
    let request = backend.requests()[0].clone();

    assert_eq!(value, json!({ "id": 7 }));
    assert_eq!(request.method(), HttpMethod::Post);
    assert_eq!(request.url(), "https://api.example/cats");
    assert_eq!(request.header("content-type"), Some("application/json"));
    assert_eq!(request.body(), br#"{"name":"Milo"}"#);
}

#[tokio::test]
async fn http_module_supports_named_global_exports() {
    let app = BootApplication::builder()
        .import(
            HttpModule::with_backend(
                "http",
                RecordingBackend::new(HttpClientResponse::new(204, [])),
            )
            .named("external-http")
            .global(),
        )
        .build()
        .unwrap();

    let client = app.get_named::<HttpService>("external-http").unwrap();

    assert_eq!(
        client
            .get("https://api.example/ping")
            .await
            .unwrap()
            .status(),
        204
    );
}

#[tokio::test]
async fn http_module_async_options_builds_service_during_async_graph_build() {
    let app = BootApplication::builder()
        .import(HttpModule::async_options("http", |_| async {
            Ok(HttpClientOptions::new().with_base_url("https://async.example"))
        }))
        .build_async()
        .await
        .unwrap();

    let service = app.get::<HttpService>().unwrap();

    assert_eq!(service.options().base_url(), Some("https://async.example"));
}

#[tokio::test]
async fn http_service_rejects_relative_urls_without_base_url() {
    let client = HttpService::from_backend(
        RecordingBackend::new(HttpClientResponse::new(200, [])),
        HttpClientOptions::default(),
    );

    let error = client.get("/cats").await.unwrap_err();

    assert!(error
        .to_string()
        .contains("relative HTTP client URL `/cats` requires a base URL"));
}

#[derive(Clone)]
struct RecordingBackend {
    requests: Arc<Mutex<Vec<HttpClientRequest>>>,
    response: HttpClientResponse,
}

impl RecordingBackend {
    fn new(response: HttpClientResponse) -> Self {
        Self {
            requests: Arc::new(Mutex::new(Vec::new())),
            response,
        }
    }

    fn requests(&self) -> Vec<HttpClientRequest> {
        self.requests.lock().unwrap().clone()
    }
}

impl HttpClientBackend for RecordingBackend {
    fn send(&self, request: HttpClientRequest) -> BoxFuture<'static, Result<HttpClientResponse>> {
        let requests = Arc::clone(&self.requests);
        let response = self.response.clone();
        Box::pin(async move {
            requests.lock().unwrap().push(request);
            Ok(response)
        })
    }
}
