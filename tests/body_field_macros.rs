#![cfg(feature = "macros")]

use std::sync::Arc;

#[allow(unused_imports)]
use a3s_boot::{body, post};
use a3s_boot::{
    controller, injectable, BootApplication, BootError, BootRequest, ControllerDefinition,
    HttpMethod, Module, OpenApiInfo, ParseIntPipe, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Serialize, Deserialize, PartialEq, Eq)]
struct BodyFieldReply {
    name: String,
    age: Option<u8>,
    page: u16,
    kind: Option<String>,
}

fn normalize_body_kind(value: String) -> Result<String> {
    let value = value.trim();
    if value.is_empty() {
        return Err(BootError::BadRequest("body kind is required".to_string()));
    }

    Ok(value.to_ascii_uppercase())
}

#[injectable]
#[derive(Debug)]
struct BodyFieldController;

#[controller("/body-fields")]
impl BodyFieldController {
    #[post("/one")]
    async fn one(
        &self,
        #[body("name")] name: String,
        #[body("age")] age: Option<u8>,
        #[body("page", default = 1, pipe = ParseIntPipe)] page: u16,
        #[body("kind", pipe = normalize_body_kind)] kind: Option<String>,
    ) -> Result<BodyFieldReply> {
        Ok(BodyFieldReply {
            name,
            age,
            page,
            kind,
        })
    }

    #[post("/number-pipe")]
    async fn number_pipe(&self, #[body("page", pipe = ParseIntPipe)] page: u16) -> Result<u16> {
        Ok(page)
    }
}

#[derive(Debug)]
struct BodyFieldFeatureModule;

impl Module for BodyFieldFeatureModule {
    fn name(&self) -> &'static str {
        "body-field-feature"
    }

    fn controllers(&self, _module_ref: &a3s_boot::ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(BodyFieldController).controller()?])
    }
}

fn body_field_app() -> BootApplication {
    BootApplication::builder()
        .import(BodyFieldFeatureModule)
        .build()
        .unwrap()
}

#[tokio::test]
async fn body_field_macro_extracts_named_json_body_fields() {
    let request = BootRequest::new(HttpMethod::Post, "/body-fields/one")
        .with_json(&json!({
            "name": "Milo",
            "age": null,
            "kind": "tabby"
        }))
        .unwrap();
    let response = body_field_app().call(request).await.unwrap();

    assert_eq!(
        response.body_json::<BodyFieldReply>().unwrap(),
        BodyFieldReply {
            name: "Milo".to_string(),
            age: None,
            page: 1,
            kind: Some("TABBY".to_string()),
        }
    );
}

#[tokio::test]
async fn body_field_macro_pipes_json_numbers_as_request_values() {
    let request = BootRequest::new(HttpMethod::Post, "/body-fields/number-pipe")
        .with_json(&json!({ "page": 42 }))
        .unwrap();
    let response = body_field_app().call(request).await.unwrap();

    assert_eq!(response.body_json::<u16>().unwrap(), 42);
}

#[tokio::test]
async fn body_field_macro_rejects_missing_required_body_fields() {
    let request = BootRequest::new(HttpMethod::Post, "/body-fields/one")
        .with_json(&json!({ "page": 2 }))
        .unwrap();
    let error = body_field_app().call(request).await.unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message == "missing body field: name")
    );
}

#[tokio::test]
async fn body_field_macro_rejects_non_object_json_bodies() {
    let request = BootRequest::new(HttpMethod::Post, "/body-fields/one")
        .with_json(&json!(["Milo"]))
        .unwrap();
    let error = body_field_app().call(request).await.unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message == "expected JSON object body")
    );
}

#[test]
fn body_field_macro_documents_json_object_request_body_fields() {
    let document = body_field_app().openapi(OpenApiInfo::new("Body Fields", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let schema = &value["paths"]["/body-fields/one"]["post"]["requestBody"]["content"]
        ["application/json"]["schema"];

    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["name"], json!({ "type": "string" }));
    assert_eq!(schema["properties"]["age"], json!({ "type": "integer" }));
    assert_eq!(schema["properties"]["page"], json!({ "type": "string" }));
    assert_eq!(schema["properties"]["kind"], json!({ "type": "string" }));
    assert_eq!(schema["required"], json!(["name"]));
}

#[test]
fn request_body_field_helpers_read_typed_json_fields() {
    let request = BootRequest::new(HttpMethod::Post, "/body-fields")
        .with_json(&json!({
            "name": "Milo",
            "page": 3,
            "enabled": true
        }))
        .unwrap();

    assert_eq!(request.body_field_as::<String>("name").unwrap(), "Milo");
    assert_eq!(request.body_field_as::<u16>("page").unwrap(), 3);
    assert_eq!(request.body_field_string("enabled").unwrap(), "true");
    assert_eq!(
        request.optional_body_field_as::<u8>("missing").unwrap(),
        None
    );
}
