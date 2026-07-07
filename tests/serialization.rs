use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, HttpMethod, RouteDefinition,
    SerializationInterceptor, SerializationOptions,
};
use serde_json::{json, Value};

#[tokio::test]
async fn serialization_interceptor_uses_route_metadata_for_json_objects() {
    let app = BootApplication::builder()
        .use_global_serialization()
        .route(
            RouteDefinition::get_json("/users/{id}", |_| async {
                Ok(json!({
                    "id": "u1",
                    "email": "milo@example.com",
                    "password": "secret",
                    "nickname": null
                }))
            })
            .unwrap()
            .with_serialization(
                SerializationOptions::new()
                    .exclude_field("password")
                    .skip_null_fields(),
            ),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/users/u1"))
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(
        body,
        json!({
            "id": "u1",
            "email": "milo@example.com"
        })
    );
}

#[tokio::test]
async fn serialization_interceptor_includes_fields_for_json_arrays() {
    let app = BootApplication::builder()
        .use_global_serialization()
        .route(
            RouteDefinition::get_json("/cats", |_| async {
                Ok(json!([
                    { "id": "1", "name": "Milo", "secret": "likes boxes" },
                    { "id": "2", "name": "Luna", "secret": "opens doors" }
                ]))
            })
            .unwrap()
            .with_serialization(SerializationOptions::new().include_fields(["id", "name"])),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(
        body,
        json!([
            { "id": "1", "name": "Milo" },
            { "id": "2", "name": "Luna" }
        ])
    );
}

#[tokio::test]
async fn controller_serialization_options_are_inherited_by_routes() {
    let controller = ControllerDefinition::new("/sessions")
        .unwrap()
        .with_serialization(SerializationOptions::new().exclude_field("token"))
        .get_json("/", |_| async {
            Ok(json!({
                "id": "session-1",
                "token": "private"
            }))
        })
        .unwrap();
    let app = BootApplication::builder()
        .use_global_serialization()
        .route(controller.routes()[0].clone())
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/sessions"))
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(body, json!({ "id": "session-1" }));
}

#[tokio::test]
async fn serialization_interceptor_defaults_apply_without_route_metadata() {
    let app = BootApplication::builder()
        .use_global_interceptor(SerializationInterceptor::with_options(
            SerializationOptions::new().exclude_field("internal"),
        ))
        .route(
            RouteDefinition::get_json("/status", |_| async {
                Ok(json!({
                    "state": "ready",
                    "internal": "hidden"
                }))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/status"))
        .await
        .unwrap();
    let body = response.body_json::<Value>().unwrap();

    assert_eq!(body, json!({ "state": "ready" }));
}

#[tokio::test]
async fn serialization_interceptor_leaves_non_json_responses_unchanged() {
    let app = BootApplication::builder()
        .use_global_serialization()
        .route(
            RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) })
                .unwrap()
                .with_serialization(SerializationOptions::new().exclude_field("anything")),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/health"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "ok");
}

#[tokio::test]
async fn serialization_interceptor_updates_content_length_when_body_changes() {
    let app = BootApplication::builder()
        .use_global_serialization()
        .route(
            RouteDefinition::get("/users", |_| async {
                let response = BootResponse::json(&json!({
                    "id": "u1",
                    "secret": "hidden"
                }))?;
                let content_length = response.body().len() as u64;
                Ok(response.with_content_length(content_length))
            })
            .unwrap()
            .with_serialization(SerializationOptions::new().exclude_field("secret")),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/users"))
        .await
        .unwrap();

    response.validate_content_length().unwrap();
    assert_eq!(
        response.content_length().unwrap(),
        Some(response.body().len() as u64)
    );
    assert_eq!(
        response.body_json::<Value>().unwrap(),
        json!({ "id": "u1" })
    );
}
