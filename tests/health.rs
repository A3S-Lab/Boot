#![cfg(feature = "health")]

use a3s_boot::{
    BootApplication, BootError, BootRequest, HealthCheckService, HealthIndicatorResult,
    HealthModule, HealthStatus, HttpMethod,
};
use serde_json::{json, Value};

#[tokio::test]
async fn health_module_exposes_json_route_for_up_indicators() {
    let app = BootApplication::builder()
        .import(HealthModule::new("health").indicator("database", || async {
            Ok(HealthIndicatorResult::up().with_detail_value("latency_ms", 2))
        }))
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/health").with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(body["status"], json!("up"));
    assert_eq!(body["checks"]["database"]["status"], json!("up"));
    assert_eq!(
        body["checks"]["database"]["details"]["latency_ms"],
        json!(2)
    );
}

#[tokio::test]
async fn health_route_returns_service_unavailable_for_down_or_failed_indicators() {
    let app = BootApplication::builder()
        .import(
            HealthModule::new("health")
                .indicator("cache", || async {
                    Ok(HealthIndicatorResult::down().with_detail_value("reason", "miss"))
                })
                .indicator("database", || async {
                    Err(BootError::Internal("connection refused".to_string()))
                }),
        )
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/health").with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(response.status(), 503);
    assert_eq!(body["status"], json!("down"));
    assert_eq!(body["checks"]["cache"]["status"], json!("down"));
    assert_eq!(
        body["checks"]["database"]["details"]["error"],
        json!("internal error: connection refused")
    );
}

#[tokio::test]
async fn health_check_service_can_be_resolved_without_a_route() {
    let app = BootApplication::builder()
        .import(
            HealthModule::new("health")
                .without_route()
                .indicator("database", || async { Ok(HealthIndicatorResult::up()) }),
        )
        .build()
        .unwrap();
    let service = app.get::<HealthCheckService>().unwrap();

    let report = service.check().await.unwrap();
    let missing_route = app
        .handle(
            BootRequest::new(HttpMethod::Get, "/health").with_header("accept", "application/json"),
        )
        .await;

    assert_eq!(report.status, HealthStatus::Up);
    assert_eq!(report.checks.len(), 1);
    assert_eq!(missing_route.status(), 404);
}

#[test]
fn health_module_supports_named_and_global_exports() {
    let app = BootApplication::builder()
        .import(
            HealthModule::new("health")
                .named("readiness")
                .global()
                .without_route(),
        )
        .build()
        .unwrap();
    let service = app.get_named::<HealthCheckService>("readiness").unwrap();

    assert_eq!(service.indicator_count().unwrap(), 0);
}
