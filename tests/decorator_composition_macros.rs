#![cfg(feature = "macros")]

use std::sync::Arc;

use a3s_boot::{
    controller, BootApplication, BootRequest, BoxFuture, ControllerDefinition, ExecutionContext,
    Guard, HttpMethod, Module, ModuleRef, OpenApiInfo, Result,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize, Serialize)]
struct ComposedCatDto {
    id: String,
    name: String,
}

struct ComposedMetadataGuard;

impl Guard for ComposedMetadataGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async move {
            let resource = context
                .metadata_as::<String>("resource")?
                .unwrap_or_default();
            let roles = context
                .metadata_as::<Vec<String>>("roles")?
                .unwrap_or_default();

            Ok(resource == "cats" && roles == ["admin".to_string()])
        })
    }
}

#[derive(Debug)]
struct ComposedController;

#[controller("/composed")]
#[a3s_boot::apply_decorators(
    tag("composed-cats"),
    metadata("resource", "cats"),
    use_guard(ComposedMetadataGuard)
)]
impl ComposedController {
    #[a3s_boot::apply_decorators(
        get("/{id}"),
        http_code(202),
        header("x-composed", "yes"),
        metadata("roles", ["admin"]),
        operation(summary = "Find a composed cat", operation_id = "findComposedCat"),
        response(status = 202, description = "Composed cat", schema = ComposedCatDto),
        bearer_auth,
        api_key_auth(name = "x-api-key"),
    )]
    async fn find_one(&self, #[a3s_boot::param("id")] id: String) -> Result<ComposedCatDto> {
        Ok(ComposedCatDto {
            id,
            name: "Milo".to_string(),
        })
    }
}

#[derive(Debug)]
struct ComposedModule;

impl Module for ComposedModule {
    fn name(&self) -> &'static str {
        "composed"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(ComposedController).controller()?])
    }
}

#[tokio::test]
async fn apply_decorators_composes_controller_and_route_attributes() {
    let app = BootApplication::builder()
        .import(ComposedModule)
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/composed/42"))
        .await
        .unwrap();
    assert_eq!(response.status(), 202);
    assert_eq!(response.header("x-composed"), Some("yes"));
    assert_eq!(
        response.body_json::<ComposedCatDto>().unwrap().id,
        "42".to_string()
    );

    let reflector = app.reflector().unwrap();
    assert_eq!(
        reflector.metadata_value(HttpMethod::Get, "/composed/{id}", "resource"),
        Some(&json!("cats"))
    );
    assert_eq!(
        reflector.metadata_value(HttpMethod::Get, "/composed/{id}", "roles"),
        Some(&json!(["admin"]))
    );

    let document = app.openapi(OpenApiInfo::new("Composed API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/composed/{id}"]["get"];

    assert_eq!(operation["tags"], json!(["composed-cats"]));
    assert_eq!(operation["operationId"], "findComposedCat");
    assert_eq!(operation["summary"], "Find a composed cat");
    assert_eq!(
        operation["responses"]["202"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/ComposedCatDto" })
    );
    assert_eq!(operation["security"][0]["bearerAuth"], json!([]));
    assert_eq!(operation["security"][1]["apiKeyAuth"], json!([]));
    assert_eq!(
        value["components"]["securitySchemes"]["bearerAuth"],
        json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        })
    );
    assert_eq!(
        value["components"]["securitySchemes"]["apiKeyAuth"],
        json!({
            "type": "apiKey",
            "in": "header",
            "name": "x-api-key"
        })
    );
}
