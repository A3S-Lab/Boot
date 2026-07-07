#![cfg(feature = "logging")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, HttpMethod, InMemoryLogSink, LogFields, LogLevel,
    Logger, LoggingModule, RequestLoggingInterceptor, RequestLoggingMiddleware, RouteDefinition,
};
use serde_json::Value;
use std::sync::Arc;

#[test]
fn logger_writes_structured_records_to_in_memory_sink() {
    let sink = InMemoryLogSink::new();
    let logger = Logger::new(sink.clone())
        .with_target("cats")
        .with_default_field("service", "catalog")
        .unwrap();

    logger
        .log_with_fields(
            LogLevel::Info,
            "cat created",
            LogFields::new().with("cat_id", "42").unwrap(),
        )
        .unwrap();

    let records = sink.records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].level, LogLevel::Info);
    assert_eq!(records[0].target, "cats");
    assert_eq!(records[0].message, "cat created");
    assert_eq!(records[0].field("service"), Some(&Value::from("catalog")));
    assert_eq!(records[0].field("cat_id"), Some(&Value::from("42")));
}

#[test]
fn logging_module_exports_named_and_global_logger_providers() {
    let named = BootApplication::builder()
        .import(LoggingModule::noop("named-logging").named("app-logger"))
        .build()
        .unwrap();
    assert!(named.get_named::<Logger>("app-logger").is_ok());
    assert!(named.get_optional::<Logger>().unwrap().is_none());

    let global = BootApplication::builder()
        .import(LoggingModule::noop("global-logging").global())
        .build()
        .unwrap();
    assert!(global.get::<Logger>().is_ok());
}

#[tokio::test]
async fn request_logging_interceptor_records_request_and_response() {
    let sink = InMemoryLogSink::new();
    let logger = Arc::new(Logger::new(sink.clone()).with_target("http"));
    let app = BootApplication::builder()
        .use_global_interceptor(RequestLoggingInterceptor::new(Arc::clone(&logger)))
        .route(
            RouteDefinition::post("/cats", |_| async {
                BootResponse::json_with_status(201, &serde_json::json!({ "id": "42" }))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Post, "/cats?debug=true"))
        .await
        .unwrap();
    assert_eq!(response.status(), 201);

    let records = sink.records().unwrap();
    assert_eq!(records.len(), 2);
    assert_eq!(records[0].message, "request started");
    assert_eq!(records[0].field("method"), Some(&Value::from("POST")));
    assert_eq!(
        records[0].field("request_path"),
        Some(&Value::from("/cats"))
    );
    assert_eq!(records[0].field("route_path"), Some(&Value::from("/cats")));

    assert_eq!(records[1].message, "request completed");
    assert_eq!(records[1].field("status"), Some(&Value::from(201)));
}

#[tokio::test]
async fn request_logging_middleware_records_requests_before_handlers() {
    let sink = InMemoryLogSink::new();
    let logger = Arc::new(Logger::new(sink.clone()).with_target("http"));
    let app = BootApplication::builder()
        .use_global_middleware(RequestLoggingMiddleware::new(logger))
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/health").with_text("ping"))
        .await
        .unwrap();
    assert_eq!(response.status(), 200);

    let records = sink.records().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].message, "request received");
    assert_eq!(records[0].field("method"), Some(&Value::from("GET")));
    assert_eq!(records[0].field("path"), Some(&Value::from("/health")));
    assert_eq!(records[0].field("body_bytes"), Some(&Value::from(4)));
}
