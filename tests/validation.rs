use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition, HttpMethod,
    Result, RouteDefinition, Validate, ValidationOptions, ValidationSchema,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug, Deserialize, Serialize)]
struct ValidatedCreateItemDto {
    name: String,
}

impl Validate for ValidatedCreateItemDto {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

impl ValidationSchema for ValidatedCreateItemDto {
    fn allowed_fields() -> &'static [&'static str] {
        &["name"]
    }
}

#[derive(Debug, Deserialize)]
struct ValidatedItemQuery {
    page: u16,
}

impl Validate for ValidatedItemQuery {
    fn validate(&self) -> Result<()> {
        if self.page == 0 {
            return Err(BootError::BadRequest(
                "page must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

impl ValidationSchema for ValidatedItemQuery {
    fn allowed_fields() -> &'static [&'static str] {
        &["page"]
    }
}

#[derive(Debug, Deserialize)]
struct ValidatedItemParams {
    id: String,
}

impl Validate for ValidatedItemParams {
    fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty() || self.id == "0" {
            return Err(BootError::BadRequest(
                "id must be a non-zero value".to_string(),
            ));
        }
        Ok(())
    }
}

impl ValidationSchema for ValidatedItemParams {
    fn allowed_fields() -> &'static [&'static str] {
        &["id"]
    }
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ItemDto {
    name: String,
}

#[tokio::test]
async fn validated_json_routes_reject_invalid_body_dtos() {
    let route = RouteDefinition::post_validated_json_with_status(
        "/items",
        201,
        |dto: ValidatedCreateItemDto| async move { Ok(ItemDto { name: dto.name }) },
    )
    .unwrap();

    let error = route
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_content_type("application/json")
                .with_body(r#"{"name":""}"#),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("name is required"))
    );
}

#[tokio::test]
async fn controller_validation_enables_registered_body_validators() {
    let called = Arc::new(std::sync::Mutex::new(false));
    let called_handler = Arc::clone(&called);
    let route = RouteDefinition::post_json("/", move |dto: ValidatedCreateItemDto| {
        let called_handler = Arc::clone(&called_handler);
        async move {
            *called_handler.lock().unwrap() = true;
            Ok(ItemDto { name: dto.name })
        }
    })
    .unwrap()
    .with_body_validation::<ValidatedCreateItemDto>();
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .with_validation()
        .route(route)
        .unwrap();

    let error = controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_content_type("application/json")
                .with_body(r#"{"name":"   "}"#),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("name is required"))
    );
    assert!(!*called.lock().unwrap());
}

#[tokio::test]
async fn global_validation_enables_registered_body_validators() {
    let route = RouteDefinition::post_json("/", |dto: ValidatedCreateItemDto| async move {
        Ok(ItemDto { name: dto.name })
    })
    .unwrap()
    .with_body_validation::<ValidatedCreateItemDto>();
    let app = BootApplication::builder()
        .use_global_validation()
        .route(route)
        .build()
        .unwrap();

    let error = app
        .call(
            BootRequest::new(HttpMethod::Post, "/")
                .with_content_type("application/json")
                .with_body(r#"{"name":""}"#),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("name is required"))
    );
}

#[tokio::test]
async fn route_validation_rejects_query_and_param_dtos() {
    let route = RouteDefinition::get("/{id}", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_params_validation::<ValidatedItemParams>()
        .with_query_validation::<ValidatedItemQuery>()
        .with_validation();

    let param_error = route
        .call(BootRequest::new(HttpMethod::Get, "/0?page=1"))
        .await
        .unwrap_err();
    let query_error = route
        .call(BootRequest::new(HttpMethod::Get, "/42?page=0"))
        .await
        .unwrap_err();

    assert!(
        matches!(param_error, BootError::BadRequest(message) if message.contains("id must be a non-zero value"))
    );
    assert!(
        matches!(query_error, BootError::BadRequest(message) if message.contains("page must be greater than zero"))
    );
}

#[tokio::test]
async fn raw_handlers_do_not_validate_without_registered_validators() {
    let controller = ControllerDefinition::new("/raw")
        .unwrap()
        .with_validation()
        .post("/", |request: BootRequest| async move {
            Ok(BootResponse::text(request.text()?))
        })
        .unwrap();

    let response = controller.routes()[0]
        .call(BootRequest::new(HttpMethod::Post, "/raw").with_body(r#"{"name":""}"#))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), r#"{"name":""}"#);
}

#[tokio::test]
async fn validation_options_whitelist_unknown_body_properties() {
    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        BootResponse::json(&request.json::<serde_json::Value>()?)
    })
    .unwrap()
    .with_body_validation_options::<ValidatedCreateItemDto>(
        ValidationOptions::new().whitelist(true),
    )
    .with_validation();

    let response = route
        .call(
            BootRequest::new(HttpMethod::Post, "/")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Milo","role":"admin"}"#),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_json::<serde_json::Value>().unwrap(),
        json!({ "name": "Milo" })
    );
}

#[tokio::test]
async fn validation_options_whitelist_updates_body_content_length() {
    let route = RouteDefinition::post("/", |request: BootRequest| async move {
        Ok(BootResponse::text(
            request.strict_content_length()?.unwrap().to_string(),
        ))
    })
    .unwrap()
    .with_body_validation_options::<ValidatedCreateItemDto>(
        ValidationOptions::new().whitelist(true),
    )
    .with_validation();

    let body = r#"{"name":"Milo","role":"admin"}"#;
    let stripped_body = serde_json::to_vec(&json!({ "name": "Milo" })).unwrap();
    let response = route
        .call(
            BootRequest::new(HttpMethod::Post, "/")
                .with_content_type("application/json")
                .with_body(body)
                .with_content_length(body.len() as u64)
                .append_header("Content-Length", body.len().to_string()),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_text().unwrap(),
        stripped_body.len().to_string()
    );
}

#[tokio::test]
async fn validation_options_forbid_unknown_body_properties() {
    let called = Arc::new(std::sync::Mutex::new(false));
    let called_handler = Arc::clone(&called);
    let route = RouteDefinition::post("/", move |_| {
        let called_handler = Arc::clone(&called_handler);
        async move {
            *called_handler.lock().unwrap() = true;
            Ok(BootResponse::text("unreachable"))
        }
    })
    .unwrap()
    .with_body_validation_options::<ValidatedCreateItemDto>(
        ValidationOptions::new()
            .whitelist(true)
            .forbid_non_whitelisted(true),
    )
    .with_validation();

    let error = route
        .call(
            BootRequest::new(HttpMethod::Post, "/")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Milo","role":"admin"}"#),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message == "non-whitelisted body properties: role")
    );
    assert!(!*called.lock().unwrap());
}

#[tokio::test]
async fn validation_options_whitelist_unknown_query_parameters() {
    let route = RouteDefinition::get("/", |request: BootRequest| async move {
        Ok(BootResponse::text(format!(
            "{}:{}",
            request.query_param("page").unwrap_or("missing"),
            request.query_param("extra").unwrap_or("stripped")
        )))
    })
    .unwrap()
    .with_query_validation_options::<ValidatedItemQuery>(ValidationOptions::new().whitelist(true))
    .with_validation();

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/?page=2&extra=yes"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "2:stripped");
}

#[tokio::test]
async fn validation_options_forbid_unknown_path_parameters() {
    let route = RouteDefinition::get("/{id}/{extra}", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_params_validation_options::<ValidatedItemParams>(
            ValidationOptions::new().forbid_non_whitelisted(true),
        )
        .with_validation();

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/42/ignored"))
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message == "non-whitelisted path parameters: extra")
    );
}
