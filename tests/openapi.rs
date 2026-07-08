use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, HttpMethod, MiddlewareRoute,
    OpenApiInfo, OpenApiResponse, OpenApiSchema, RouteDefinition,
};
use serde::Serialize;
use serde_json::{json, Value};

#[derive(Debug, Serialize)]
struct OpenApiCatDto {
    id: String,
    name: String,
}

#[test]
fn controller_openapi_tags_apply_to_routes() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .with_tag("cats")
        .get("/{id}", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap();

    assert_eq!(controller.routes()[0].openapi().tags, vec!["cats"]);
}

#[tokio::test]
async fn openapi_documents_include_route_metadata_and_auto_path_params() {
    let route = RouteDefinition::get_json("/cats/{id}", |request: BootRequest| async move {
        Ok(OpenApiCatDto {
            id: request.param("id").unwrap_or("unknown").to_string(),
            name: "Milo".to_string(),
        })
    })
    .unwrap()
    .with_tag("cats")
    .with_operation_id("findCat")
    .with_summary("Find a cat")
    .with_description("Returns one cat by id.")
    .with_query_parameter("include_toys", false, OpenApiSchema::boolean())
    .with_header_parameter("x-request-id", false, OpenApiSchema::string())
    .with_json_response(200, "Cat found", OpenApiSchema::object())
    .with_response(404, OpenApiResponse::description("Cat not found"))
    .with_schema_component("OpenApiCatDto", OpenApiSchema::object())
    .with_bearer_auth();

    let app = BootApplication::builder()
        .route(route)
        .serve_openapi(
            "/openapi.json",
            OpenApiInfo::new("Cats API", "1.0.0").with_description("Cat service API"),
        )
        .build()
        .unwrap();

    let document = app.openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats/{id}"]["get"];

    assert_eq!(value["openapi"], "3.0.3");
    assert_eq!(operation["tags"], json!(["cats"]));
    assert_eq!(operation["operationId"], "findCat");
    assert_eq!(operation["summary"], "Find a cat");
    assert_eq!(operation["description"], "Returns one cat by id.");
    assert!(has_parameter(
        operation,
        "id",
        "path",
        true,
        json!({ "type": "string" })
    ));
    assert!(has_parameter(
        operation,
        "include_toys",
        "query",
        false,
        json!({ "type": "boolean" })
    ));
    assert!(has_parameter(
        operation,
        "x-request-id",
        "header",
        false,
        json!({ "type": "string" })
    ));
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "type": "object" })
    );
    assert_eq!(
        operation["responses"]["404"]["description"],
        "Cat not found"
    );
    assert_eq!(operation["security"][0]["bearerAuth"], json!([]));
    assert_eq!(
        value["components"]["schemas"]["OpenApiCatDto"],
        json!({ "type": "object" })
    );

    let served = app
        .call(BootRequest::new(HttpMethod::Get, "/openapi.json"))
        .await
        .unwrap()
        .body_json::<Value>()
        .unwrap();
    assert_eq!(served["info"]["description"], "Cat service API");
    assert!(served["paths"]
        .as_object()
        .unwrap()
        .contains_key("/cats/{id}"));
    assert!(!served["paths"]
        .as_object()
        .unwrap()
        .contains_key("/openapi.json"));
}

#[test]
fn openapi_expands_all_routes_and_keeps_exact_method_metadata() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::all_json("/catch", |request: BootRequest| async move {
                Ok(OpenApiCatDto {
                    id: request.method().as_str().to_string(),
                    name: "Catch".to_string(),
                })
            })
            .unwrap()
            .with_summary("Catch all methods"),
        )
        .route(
            RouteDefinition::get_json("/catch", |_| async {
                Ok(OpenApiCatDto {
                    id: "GET".to_string(),
                    name: "Exact".to_string(),
                })
            })
            .unwrap()
            .with_summary("Exact GET"),
        )
        .build()
        .unwrap();

    let document = app.openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operations = value["paths"]["/catch"].as_object().unwrap();

    for method in ["get", "post", "put", "patch", "delete", "options", "head"] {
        assert!(
            operations.contains_key(method),
            "{method} operation is missing"
        );
    }
    assert_eq!(operations["get"]["summary"], "Exact GET");
    assert_eq!(operations["post"]["summary"], "Catch all methods");
}

#[test]
fn openapi_documents_catch_all_routes_with_valid_path_parameters() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get_json("/files/{*path}", |request: BootRequest| async move {
                Ok(OpenApiCatDto {
                    id: request.param("path").unwrap_or("unknown").to_string(),
                    name: "File".to_string(),
                })
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let document = app.openapi(OpenApiInfo::new("Files API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let paths = value["paths"].as_object().unwrap();
    let operation = &value["paths"]["/files/{path}"]["get"];

    assert!(paths.contains_key("/files/{path}"));
    assert!(!paths.contains_key("/files/{*path}"));
    assert!(has_parameter(
        operation,
        "path",
        "path",
        true,
        json!({ "type": "string" })
    ));
}

#[test]
fn openapi_documents_include_request_and_response_examples() {
    let route = RouteDefinition::post_json_with_status(
        "/cats",
        201,
        |dto: OpenApiCreateCatDto| async move {
            Ok(OpenApiCatDto {
                id: "generated".to_string(),
                name: dto.name,
            })
        },
    )
    .unwrap()
    .try_with_json_request_body_example(
        OpenApiSchema::reference("OpenApiCreateCatDto"),
        json!({ "name": "Milo" }),
    )
    .unwrap()
    .try_with_json_response_example(
        201,
        "Cat created",
        OpenApiSchema::reference("OpenApiCatDto"),
        json!({ "id": "generated", "name": "Milo" }),
    )
    .unwrap();

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats"]["post"];

    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/OpenApiCreateCatDto" })
    );
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["example"],
        json!({ "name": "Milo" })
    );
    assert_eq!(
        operation["responses"]["201"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/OpenApiCatDto" })
    );
    assert_eq!(
        operation["responses"]["201"]["content"]["application/json"]["example"],
        json!({ "id": "generated", "name": "Milo" })
    );
}

#[cfg(feature = "openapi-schemas")]
#[test]
fn openapi_schema_components_can_be_generated_from_schemars() {
    #[derive(Debug, serde::Serialize, schemars::JsonSchema)]
    struct SchemarsCatDto {
        name: String,
        age: Option<u8>,
    }

    let route = RouteDefinition::get_json("/cats/{id}", |request: BootRequest| async move {
        Ok(OpenApiCatDto {
            id: request.param("id").unwrap_or("unknown").to_string(),
            name: "Milo".to_string(),
        })
    })
    .unwrap()
    .with_json_response(200, "Cat found", OpenApiSchema::reference("SchemarsCatDto"))
    .try_with_json_schema_component::<SchemarsCatDto>()
    .unwrap();

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let schema = &value["components"]["schemas"]["SchemarsCatDto"];

    assert_eq!(
        value["paths"]["/cats/{id}"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"],
        json!({ "$ref": "#/components/schemas/SchemarsCatDto" })
    );
    assert_eq!(schema["type"], "object");
    assert_eq!(schema["properties"]["name"]["type"], "string");
    assert_eq!(
        schema["properties"]["age"]["type"],
        json!(["integer", "null"])
    );
}

#[tokio::test]
async fn openapi_endpoint_uses_final_global_prefix_paths() {
    let app = BootApplication::builder()
        .global_prefix("/api/v1")
        .route(
            RouteDefinition::post_json_with_status(
                "/",
                201,
                |dto: OpenApiCreateCatDto| async move {
                    Ok(OpenApiCatDto {
                        id: "generated".to_string(),
                        name: dto.name,
                    })
                },
            )
            .unwrap()
            .with_tag("cats")
            .with_json_request_body(OpenApiSchema::object())
            .with_json_response(201, "Cat created", OpenApiSchema::object()),
        )
        .route(
            RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) })
                .unwrap()
                .hide_from_openapi(),
        )
        .serve_openapi("/openapi.json", OpenApiInfo::new("Cats API", "1.0.0"))
        .build()
        .unwrap();

    assert!(app
        .routes()
        .iter()
        .any(|route| route.path() == "/api/v1/openapi.json"));

    let served = app
        .call(BootRequest::new(HttpMethod::Get, "/api/v1/openapi.json"))
        .await
        .unwrap()
        .body_json::<Value>()
        .unwrap();
    let operation = &served["paths"]["/api/v1"]["post"];

    assert_eq!(operation["tags"], json!(["cats"]));
    assert_eq!(
        operation["requestBody"]["content"]["application/json"]["schema"],
        json!({ "type": "object" })
    );
    assert_eq!(operation["responses"]["201"]["description"], "Cat created");
    assert!(!served["paths"]
        .as_object()
        .unwrap()
        .contains_key("/api/v1/health"));
    assert!(!served["paths"]
        .as_object()
        .unwrap()
        .contains_key("/api/v1/openapi.json"));
}

#[tokio::test]
async fn openapi_ui_serves_html_and_document_route() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get_json("/cats", |_| async {
                Ok(OpenApiCatDto {
                    id: "cat-1".to_string(),
                    name: "Milo".to_string(),
                })
            })
            .unwrap()
            .with_tag("cats"),
        )
        .serve_openapi_ui(
            "/docs",
            "/docs/openapi.json",
            OpenApiInfo::new("Cats <API>", "1.0.0"),
        )
        .build()
        .unwrap();

    let html = app
        .call(BootRequest::new(HttpMethod::Get, "/docs"))
        .await
        .unwrap();
    let document = app
        .call(BootRequest::new(HttpMethod::Get, "/docs/openapi.json"))
        .await
        .unwrap()
        .body_json::<Value>()
        .unwrap();

    assert_eq!(
        html.header("content-type"),
        Some("text/html; charset=utf-8")
    );
    let html = html.body_text().unwrap();
    assert!(html.contains("SwaggerUIBundle"));
    assert!(html.contains(r#"url: "/docs/openapi.json""#));
    assert!(html.contains("Cats &lt;API&gt;"));
    assert!(document["paths"].as_object().unwrap().contains_key("/cats"));
    assert!(!document["paths"].as_object().unwrap().contains_key("/docs"));
    assert!(!document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/docs/openapi.json"));
}

#[tokio::test]
async fn openapi_ui_uses_global_prefix_for_html_and_document_urls() {
    let app = BootApplication::builder()
        .global_prefix("/api")
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .serve_openapi_ui(
            "/docs",
            "/docs/openapi.json",
            OpenApiInfo::new("Cats API", "1.0.0"),
        )
        .build()
        .unwrap();

    assert!(app.routes().iter().any(|route| route.path() == "/api/docs"));
    assert!(app
        .routes()
        .iter()
        .any(|route| route.path() == "/api/docs/openapi.json"));

    let html = app
        .call(BootRequest::new(HttpMethod::Get, "/api/docs"))
        .await
        .unwrap()
        .body_text()
        .unwrap();
    let document = app
        .call(BootRequest::new(HttpMethod::Get, "/api/docs/openapi.json"))
        .await
        .unwrap()
        .body_json::<Value>()
        .unwrap();

    assert!(html.contains(r#"url: "/api/docs/openapi.json""#));
    assert!(document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/api/health"));
    assert!(!document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/api/docs"));
}

#[tokio::test]
async fn openapi_ui_respects_global_prefix_exclusions_for_document_url() {
    let app = BootApplication::builder()
        .global_prefix("/api")
        .exclude_global_prefix([MiddlewareRoute::get("/docs/openapi.json").unwrap()])
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .serve_openapi_ui(
            "/docs",
            "/docs/openapi.json",
            OpenApiInfo::new("Cats API", "1.0.0"),
        )
        .build()
        .unwrap();

    assert!(app.routes().iter().any(|route| route.path() == "/api/docs"));
    assert!(app
        .routes()
        .iter()
        .any(|route| route.path() == "/docs/openapi.json"));

    let html = app
        .call(BootRequest::new(HttpMethod::Get, "/api/docs"))
        .await
        .unwrap()
        .body_text()
        .unwrap();
    let document = app
        .call(BootRequest::new(HttpMethod::Get, "/docs/openapi.json"))
        .await
        .unwrap()
        .body_json::<Value>()
        .unwrap();

    assert!(html.contains(r#"url: "/docs/openapi.json""#));
    assert!(document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/api/health"));
}

#[derive(Debug, serde::Deserialize)]
struct OpenApiCreateCatDto {
    name: String,
}

fn has_parameter(
    operation: &Value,
    name: &str,
    location: &str,
    required: bool,
    schema: Value,
) -> bool {
    operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .any(|parameter| {
            parameter["name"] == name
                && parameter["in"] == location
                && parameter["required"] == required
                && parameter["schema"] == schema
        })
}
