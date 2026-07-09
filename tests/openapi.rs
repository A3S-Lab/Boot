use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, HttpMethod, MiddlewareRoute,
    OpenApiExample, OpenApiHeader, OpenApiInfo, OpenApiParameter, OpenApiRequestBody,
    OpenApiResponse, OpenApiSchema, RouteDefinition,
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

#[test]
fn controller_can_hide_all_routes_from_openapi() {
    let controller = ControllerDefinition::new("/hidden-cats")
        .unwrap()
        .hide_from_openapi()
        .route(
            RouteDefinition::get_json("/", |_| async {
                Ok(OpenApiCatDto {
                    id: "hidden".to_string(),
                    name: "Hidden".to_string(),
                })
            })
            .unwrap(),
        )
        .unwrap();

    let document = BootApplication::builder()
        .route(controller.routes()[0].clone())
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();

    assert!(!value["paths"]
        .as_object()
        .unwrap()
        .contains_key("/hidden-cats"));
}

#[test]
fn controller_openapi_components_apply_to_routes() {
    let controller = ControllerDefinition::new("/component-cats")
        .unwrap()
        .try_with_openapi_extension("x-controller-default", json!({ "source": "controller" }))
        .unwrap()
        .with_parameter_component(
            "PageQuery",
            OpenApiParameter::query("page", false, OpenApiSchema::integer()),
        )
        .with_response_component("NotFound", OpenApiResponse::description("Cat not found"))
        .route(
            RouteDefinition::get_json("/", |_| async {
                Ok(OpenApiCatDto {
                    id: "cat-1".to_string(),
                    name: "Milo".to_string(),
                })
            })
            .unwrap()
            .with_query_parameter_ref("page", "PageQuery")
            .try_with_openapi_extension("x-controller-default", json!({ "source": "route" }))
            .unwrap()
            .with_default_response_ref("NotFound"),
        )
        .unwrap();

    let document = BootApplication::builder()
        .route(controller.routes()[0].clone())
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();

    assert_eq!(
        value["paths"]["/component-cats"]["get"]["parameters"],
        json!([{ "$ref": "#/components/parameters/PageQuery" }])
    );
    assert_eq!(
        value["paths"]["/component-cats"]["get"]["responses"]["default"],
        json!({ "$ref": "#/components/responses/NotFound" })
    );
    assert_eq!(
        value["paths"]["/component-cats"]["get"]["x-controller-default"],
        json!({ "source": "route" })
    );
    assert_eq!(
        value["components"]["parameters"]["PageQuery"]["schema"],
        json!({ "type": "integer" })
    );
    assert_eq!(
        value["components"]["responses"]["NotFound"]["description"],
        "Cat not found"
    );
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
    .with_openapi_server_description("https://edge.example.com", "Edge")
    .with_openapi_external_docs("Find cat guide", "https://docs.example.com/cats/find")
    .try_with_openapi_extension(
        "x-codeSamples",
        json!([{ "lang": "bash", "source": "curl https://api.example.com/cats/1" }]),
    )
    .unwrap()
    .with_query_parameter("include_toys", false, OpenApiSchema::boolean())
    .with_header_parameter("x-request-id", false, OpenApiSchema::string())
    .with_parameter(
        OpenApiParameter::query("filter", false, OpenApiSchema::string())
            .with_style("form")
            .with_explode(false)
            .with_allow_reserved()
            .with_named_example_ref("default", "FilterExample"),
    )
    .with_json_response(200, "Cat found", OpenApiSchema::object())
    .with_openapi_response_header(
        200,
        "x-rate-limit-remaining",
        OpenApiHeader::new(OpenApiSchema::integer()).with_description("Remaining requests"),
    )
    .with_response(404, OpenApiResponse::description("Cat not found"))
    .with_example_component(
        "FilterExample",
        OpenApiExample::value(json!("name:Milo")).with_summary("Default filter"),
    )
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

    let document = app.openapi(
        OpenApiInfo::new("Cats API", "1.0.0")
            .with_server_description("https://api.example.com", "Production")
            .with_external_docs("Cats guide", "https://docs.example.com/cats")
            .with_tag_description("cats", "Cat operations")
            .with_tag_external_docs(
                "cats",
                "Cat tag guide",
                "https://docs.example.com/tags/cats",
            ),
    );
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats/{id}"]["get"];

    assert_eq!(value["openapi"], "3.0.3");
    assert_eq!(operation["tags"], json!(["cats"]));
    assert_eq!(operation["operationId"], "findCat");
    assert_eq!(operation["summary"], "Find a cat");
    assert_eq!(operation["description"], "Returns one cat by id.");
    assert_eq!(
        operation["servers"],
        json!([{ "url": "https://edge.example.com", "description": "Edge" }])
    );
    assert_eq!(
        operation["externalDocs"],
        json!({
            "description": "Find cat guide",
            "url": "https://docs.example.com/cats/find"
        })
    );
    assert_eq!(
        operation["x-codeSamples"][0],
        json!({
            "lang": "bash",
            "source": "curl https://api.example.com/cats/1"
        })
    );
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
    let filter = find_parameter(operation, "filter", "query").unwrap();
    assert_eq!(filter["style"], "form");
    assert_eq!(filter["explode"], false);
    assert_eq!(filter["allowReserved"], true);
    assert_eq!(
        filter["examples"]["default"],
        json!({ "$ref": "#/components/examples/FilterExample" })
    );
    assert_eq!(
        operation["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "type": "object" })
    );
    assert_eq!(
        operation["responses"]["200"]["headers"]["x-rate-limit-remaining"],
        json!({
            "schema": { "type": "integer" },
            "description": "Remaining requests"
        })
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
    assert_eq!(
        value["components"]["examples"]["FilterExample"]["summary"],
        "Default filter"
    );
    assert_eq!(
        value["servers"],
        json!([{ "url": "https://api.example.com", "description": "Production" }])
    );
    assert_eq!(
        value["externalDocs"],
        json!({ "description": "Cats guide", "url": "https://docs.example.com/cats" })
    );
    assert_eq!(
        value["tags"],
        json!([{
            "name": "cats",
            "description": "Cat operations",
            "externalDocs": {
                "description": "Cat tag guide",
                "url": "https://docs.example.com/tags/cats"
            }
        }])
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

#[test]
fn openapi_documents_include_named_request_and_response_examples() {
    let route = RouteDefinition::post_json_with_status(
        "/cats/examples",
        201,
        |dto: OpenApiCreateCatDto| async move {
            Ok(OpenApiCatDto {
                id: "generated".to_string(),
                name: dto.name,
            })
        },
    )
    .unwrap()
    .try_with_json_request_body_named_example(
        OpenApiSchema::reference("OpenApiCreateCatDto"),
        "milo",
        json!({ "name": "Milo" }),
    )
    .unwrap()
    .try_with_json_response_named_example(
        201,
        "Cat created",
        OpenApiSchema::reference("OpenApiCatDto"),
        "created",
        json!({ "id": "generated", "name": "Milo" }),
    )
    .unwrap();

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats/examples"]["post"];
    let request_media = &operation["requestBody"]["content"]["application/json"];
    let response_media = &operation["responses"]["201"]["content"]["application/json"];

    assert!(request_media["example"].is_null());
    assert_eq!(
        request_media["examples"]["milo"]["value"],
        json!({ "name": "Milo" })
    );
    assert!(response_media["example"].is_null());
    assert_eq!(
        response_media["examples"]["created"]["value"],
        json!({ "id": "generated", "name": "Milo" })
    );
}

#[test]
fn openapi_documents_include_request_and_response_media_types() {
    let route = RouteDefinition::post_json_with_status(
        "/cats/import",
        202,
        |dto: OpenApiCreateCatDto| async move {
            Ok(OpenApiCatDto {
                id: "imported".to_string(),
                name: dto.name,
            })
        },
    )
    .unwrap()
    .with_request_body_content_type("multipart/form-data", OpenApiSchema::object())
    .try_with_response_content_type_example(
        202,
        "Cat import accepted",
        "application/vnd.a3s.cat+json",
        OpenApiSchema::reference("OpenApiCatDto"),
        json!({ "id": "imported", "name": "Milo" }),
    )
    .unwrap();

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats/import"]["post"];

    assert_eq!(
        operation["requestBody"]["content"]["multipart/form-data"]["schema"],
        json!({ "type": "object" })
    );
    assert!(operation["requestBody"]["content"]["application/json"].is_null());
    assert_eq!(
        operation["responses"]["202"]["content"]["application/vnd.a3s.cat+json"]["schema"],
        json!({ "$ref": "#/components/schemas/OpenApiCatDto" })
    );
    assert_eq!(
        operation["responses"]["202"]["content"]["application/vnd.a3s.cat+json"]["example"],
        json!({ "id": "imported", "name": "Milo" })
    );
}

#[test]
fn openapi_schema_helpers_support_composition_and_extra_models() {
    let cat_schema = OpenApiSchema::object()
        .with_title("Cat")
        .with_extension_value("x-rust-type", json!("OpenApiCatDto"))
        .with_property("id", OpenApiSchema::string().with_format("uuid"))
        .with_property("name", OpenApiSchema::string())
        .with_property(
            "kind",
            OpenApiSchema::string_enum(["house", "feral"]).nullable(),
        )
        .with_required("id")
        .with_required("name");

    let route = RouteDefinition::get_json("/cats/page", |_| async {
        Ok(vec![OpenApiCatDto {
            id: "cat-1".to_string(),
            name: "Milo".to_string(),
        }])
    })
    .unwrap()
    .with_json_response(
        200,
        "Cat page",
        OpenApiSchema::reference("OpenApiCatPageDto"),
    )
    .with_schema_component("OpenApiCatDto", cat_schema.clone())
    .with_schema_component(
        "OpenApiUpdateCatDto",
        cat_schema.clone().omit_properties(["id"]).partial(),
    )
    .with_schema_component(
        "OpenApiCatSummaryDto",
        cat_schema.clone().pick_properties(["id", "name"]),
    )
    .with_schema_component(
        "OpenApiPaginationMetaDto",
        OpenApiSchema::object()
            .with_description("Pagination metadata")
            .with_property("total", OpenApiSchema::integer())
            .with_additional_properties(OpenApiSchema::string())
            .with_required("total"),
    )
    .with_schema_component(
        "OpenApiCatPageDto",
        OpenApiSchema::all_of([
            OpenApiSchema::reference("OpenApiPaginationMetaDto"),
            OpenApiSchema::object_with_properties(
                [(
                    "items",
                    OpenApiSchema::array(OpenApiSchema::reference("OpenApiCatDto")),
                )],
                ["items"],
            ),
        ]),
    )
    .with_schema_component(
        "OpenApiSearchResultDto",
        OpenApiSchema::one_of([
            OpenApiSchema::reference("OpenApiCatDto"),
            OpenApiSchema::reference("OpenApiPaginationMetaDto"),
        ]),
    )
    .with_schema_component(
        "OpenApiFlexibleResultDto",
        OpenApiSchema::any_of([
            OpenApiSchema::reference("OpenApiCatDto"),
            OpenApiSchema::object().nullable(),
        ]),
    )
    .with_schema_component(
        "OpenApiPetDto",
        OpenApiSchema::one_of([
            OpenApiSchema::reference("OpenApiCatDto"),
            OpenApiSchema::reference("OpenApiPaginationMetaDto"),
        ])
        .with_discriminator_mapping(
            "kind",
            [
                ("cat", "#/components/schemas/OpenApiCatDto"),
                ("page", "#/components/schemas/OpenApiPaginationMetaDto"),
            ],
        ),
    );

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let components = &value["components"]["schemas"];

    assert_eq!(
        value["paths"]["/cats/page"]["get"]["responses"]["200"]["content"]["application/json"]
            ["schema"],
        json!({ "$ref": "#/components/schemas/OpenApiCatPageDto" })
    );
    assert_eq!(components["OpenApiCatDto"]["title"], "Cat");
    assert_eq!(components["OpenApiCatDto"]["x-rust-type"], "OpenApiCatDto");
    assert_eq!(
        components["OpenApiCatDto"]["properties"]["id"],
        json!({ "type": "string", "format": "uuid" })
    );
    assert_eq!(
        components["OpenApiCatDto"]["properties"]["kind"],
        json!({ "type": "string", "enum": ["house", "feral"], "nullable": true })
    );
    assert_eq!(components["OpenApiUpdateCatDto"]["required"], Value::Null);
    assert_eq!(
        components["OpenApiUpdateCatDto"]["properties"],
        json!({
            "name": { "type": "string" },
            "kind": { "type": "string", "enum": ["house", "feral"], "nullable": true }
        })
    );
    assert_eq!(
        components["OpenApiCatSummaryDto"]["properties"],
        json!({
            "id": { "type": "string", "format": "uuid" },
            "name": { "type": "string" }
        })
    );
    assert_eq!(
        components["OpenApiCatSummaryDto"]["required"],
        json!(["id", "name"])
    );
    assert_eq!(
        components["OpenApiPaginationMetaDto"]["additionalProperties"],
        json!({ "type": "string" })
    );
    assert_eq!(
        components["OpenApiCatPageDto"]["allOf"],
        json!([
            { "$ref": "#/components/schemas/OpenApiPaginationMetaDto" },
            {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/OpenApiCatDto" }
                    }
                },
                "required": ["items"]
            }
        ])
    );
    assert_eq!(
        components["OpenApiSearchResultDto"]["oneOf"],
        json!([
            { "$ref": "#/components/schemas/OpenApiCatDto" },
            { "$ref": "#/components/schemas/OpenApiPaginationMetaDto" }
        ])
    );
    assert_eq!(
        components["OpenApiFlexibleResultDto"]["anyOf"],
        json!([
            { "$ref": "#/components/schemas/OpenApiCatDto" },
            { "type": "object", "nullable": true }
        ])
    );
    assert_eq!(
        components["OpenApiPetDto"]["discriminator"],
        json!({
            "propertyName": "kind",
            "mapping": {
                "cat": "#/components/schemas/OpenApiCatDto",
                "page": "#/components/schemas/OpenApiPaginationMetaDto"
            }
        })
    );
}

#[test]
fn openapi_documents_include_reusable_components_and_refs() {
    let request_body = OpenApiRequestBody::json(OpenApiSchema::reference("OpenApiCreateCatDto"))
        .with_json_named_example_ref("milo", "CreateCatExample");
    let response = OpenApiResponse::json("Cat accepted", OpenApiSchema::reference("OpenApiCatDto"))
        .with_header_ref("x-trace-id", "Trace/Header")
        .with_json_named_example_ref("accepted", "CatExample");

    let route = RouteDefinition::post_json_with_status(
        "/cats/{id}/components",
        202,
        |dto: OpenApiCreateCatDto| async move {
            Ok(OpenApiCatDto {
                id: "accepted".to_string(),
                name: dto.name,
            })
        },
    )
    .unwrap()
    .with_path_parameter_ref("id", "CatId")
    .with_request_body_ref("CreateCatBody")
    .with_response_ref(202, "AcceptedCat")
    .with_schema_component("OpenApiCreateCatDto", OpenApiSchema::object())
    .with_schema_component("OpenApiCatDto", OpenApiSchema::object())
    .with_parameter_component(
        "CatId",
        OpenApiParameter::path("id", OpenApiSchema::string()).with_description("Cat identifier"),
    )
    .with_header_component(
        "Trace/Header",
        OpenApiHeader::new(OpenApiSchema::string()).with_description("Trace identifier"),
    )
    .with_example_component(
        "CatExample",
        OpenApiExample::value(json!({ "id": "accepted", "name": "Milo" }))
            .with_summary("Accepted cat"),
    )
    .try_with_example_component("CreateCatExample", json!({ "name": "Milo" }))
    .unwrap()
    .with_request_body_component("CreateCatBody", request_body)
    .with_response_component("AcceptedCat", response);

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Cats API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/cats/{id}/components"]["post"];
    let components = &value["components"];

    assert_eq!(
        operation["parameters"],
        json!([{ "$ref": "#/components/parameters/CatId" }])
    );
    assert_eq!(
        operation["requestBody"],
        json!({ "$ref": "#/components/requestBodies/CreateCatBody" })
    );
    assert_eq!(
        operation["responses"]["202"],
        json!({ "$ref": "#/components/responses/AcceptedCat" })
    );
    assert_eq!(
        components["parameters"]["CatId"],
        json!({
            "name": "id",
            "in": "path",
            "required": true,
            "schema": { "type": "string" },
            "description": "Cat identifier"
        })
    );
    assert_eq!(
        components["requestBodies"]["CreateCatBody"]["content"]["application/json"]["examples"]
            ["milo"],
        json!({ "$ref": "#/components/examples/CreateCatExample" })
    );
    assert_eq!(
        components["responses"]["AcceptedCat"]["headers"]["x-trace-id"],
        json!({ "$ref": "#/components/headers/Trace~1Header" })
    );
    assert_eq!(
        components["responses"]["AcceptedCat"]["content"]["application/json"]["examples"]
            ["accepted"],
        json!({ "$ref": "#/components/examples/CatExample" })
    );
    assert_eq!(
        components["headers"]["Trace/Header"],
        json!({
            "schema": { "type": "string" },
            "description": "Trace identifier"
        })
    );
    assert_eq!(
        components["examples"]["CatExample"]["summary"],
        "Accepted cat"
    );
    assert_eq!(
        components["examples"]["CreateCatExample"]["value"],
        json!({ "name": "Milo" })
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

fn find_parameter<'a>(operation: &'a Value, name: &str, location: &str) -> Option<&'a Value> {
    operation["parameters"]
        .as_array()?
        .iter()
        .find(|parameter| parameter["name"] == name && parameter["in"] == location)
}
