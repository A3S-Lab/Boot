#![cfg(feature = "macros")]

use std::collections::BTreeMap;
use std::sync::Arc;

use a3s_boot::{
    controller, injectable, BootApplication, BootError, BootRequest, BootResponse,
    ControllerDefinition, MessagePatternDefinition, Module, ModuleRef, OpenApiInfo,
    ProviderDefinition, Result, SseEvent, SseStream, TransportMessage, TransportReply, Validate,
    WebSocketGatewayDefinition, WebSocketMessage,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[injectable]
#[derive(Debug)]
struct MacroCatsService;

impl MacroCatsService {
    fn find_one(&self, id: &str) -> MacroCatDto {
        MacroCatDto {
            id: id.to_string(),
            name: "Milo".to_string(),
        }
    }

    fn create(&self, dto: MacroCreateCatDto) -> MacroCatDto {
        MacroCatDto {
            id: "generated".to_string(),
            name: dto.name,
        }
    }
}

#[derive(Debug)]
struct MacroCatsController {
    cats: Arc<MacroCatsService>,
}

#[derive(Debug)]
struct MacroCatsGateway {
    cats: Arc<MacroCatsService>,
}

#[derive(Debug)]
struct MacroCatsMessages {
    cats: Arc<MacroCatsService>,
}

#[a3s_boot::websocket_gateway("/macro-cats/ws")]
impl MacroCatsGateway {
    #[a3s_boot::subscribe_message("cat.find")]
    async fn find(&self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        let id = message
            .data()
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let cat = self.cats.find_one(id);
        WebSocketMessage::json("cat.found", &cat)
    }
}

#[a3s_boot::message_controller]
impl MacroCatsMessages {
    #[a3s_boot::message_pattern("macro.cat.find")]
    async fn find(&self, payload: MacroFindCatMessage) -> Result<MacroCatDto> {
        Ok(self.cats.find_one(&payload.id))
    }

    #[a3s_boot::message_pattern("macro.cat.raw", raw)]
    async fn raw(&self, message: TransportMessage) -> Result<TransportReply> {
        Ok(TransportReply::text(format!(
            "{}:{}",
            message.pattern(),
            message.data()["id"].as_str().unwrap_or("unknown")
        )))
    }

    #[a3s_boot::event_pattern("macro.cat.seen")]
    async fn seen(&self, payload: MacroFindCatMessage) -> Result<()> {
        let _ = self.cats.find_one(&payload.id);
        Ok(())
    }
}

#[controller("/macro-cats")]
#[tag("macro-cats")]
impl MacroCatsController {
    #[get("/{id}", raw)]
    async fn find_one_text(&self, request: BootRequest) -> Result<BootResponse> {
        let id = request.param("id").unwrap_or("unknown");
        let cat = self.cats.find_one(id);
        Ok(BootResponse::text(format!("{}:{}", cat.id, cat.name)))
    }

    #[get("/{id}/json")]
    async fn find_one_json(&self, request: BootRequest) -> Result<MacroCatDto> {
        let id = request.param("id").unwrap_or("unknown");
        Ok(self.cats.find_one(id))
    }

    #[get("/{id}/details")]
    #[operation(
        summary = "Find macro cat details",
        description = "Returns a macro cat with query and header metadata.",
        operation_id = "findMacroCatDetails"
    )]
    #[response(
        status = 200,
        description = "Cat details",
        schema = MacroCatDetailsDto
    )]
    #[bearer_auth]
    async fn details(
        &self,
        #[param("id")] id: String,
        #[query] query: MacroFindCatQuery,
        #[query("page")] page: u16,
        #[query("tag")] tag: Option<String>,
        #[header("x-request-id")] request_id: Option<String>,
    ) -> Result<MacroCatDetailsDto> {
        Ok(MacroCatDetailsDto {
            id,
            include_toys: query.include_toys,
            page,
            tag,
            request_id,
        })
    }

    #[get("/params/{id}/{kind}")]
    async fn params(&self, #[params] params: MacroCatParams) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: params.id,
            name: params.kind,
        })
    }

    #[get("/{id}/raw-details", raw)]
    async fn raw_details(
        &self,
        #[param("id")] id: String,
        #[header("x-request-id")] request_id: String,
        #[headers] headers: BTreeMap<String, String>,
        #[request] request: BootRequest,
    ) -> Result<BootResponse> {
        Ok(BootResponse::text(format!(
            "{}:{}:{}:{}",
            id,
            request_id,
            headers
                .get("user-agent")
                .map(String::as_str)
                .unwrap_or("unknown"),
            request.path()
        )))
    }

    #[post("/", status = 201)]
    #[operation(summary = "Create a macro cat", operation_id = "createMacroCat")]
    #[request_body(schema = MacroCreateCatDto, description = "Cat creation payload")]
    #[response(status = 201, description = "Cat created", schema = MacroCatDto)]
    async fn create(&self, dto: MacroCreateCatDto) -> Result<MacroCatDto> {
        Ok(self.cats.create(dto))
    }

    #[post("/{id}/adoptions", status = 201)]
    #[response(status = 201, description = "Cat adopted", schema = MacroCatDto)]
    async fn adopt(
        &self,
        #[param("id")] id: String,
        #[body] dto: MacroCreateCatDto,
    ) -> Result<MacroCatDto> {
        Ok(MacroCatDto { id, name: dto.name })
    }

    #[sse("/events")]
    #[hide_from_openapi]
    async fn events(&self) -> Result<impl futures_core::Stream<Item = Result<SseEvent>>> {
        Ok(futures_util::stream::iter([Ok::<_, BootError>(
            SseEvent::new("Milo").with_event("cat.found"),
        )]))
    }

    #[sse("/{id}/events")]
    async fn cat_events(&self, #[param("id")] id: String) -> Result<SseStream> {
        Ok(SseEvent::stream([
            SseEvent::new(id).with_event("cat.selected")
        ]))
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MacroCreateCatDto {
    name: String,
}

#[derive(Debug, Deserialize)]
struct MacroFindCatQuery {
    include_toys: Option<bool>,
}

#[derive(Debug, Deserialize)]
struct MacroCatParams {
    id: String,
    kind: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct MacroFindCatMessage {
    id: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MacroCatDto {
    id: String,
    name: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct MacroCatDetailsDto {
    id: String,
    include_toys: Option<bool>,
    page: u16,
    tag: Option<String>,
    request_id: Option<String>,
}

#[derive(Debug)]
struct MacroCatsModule;

impl Module for MacroCatsModule {
    fn name(&self) -> &'static str {
        "macro-cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![MacroCatsService.into_provider()])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let cats = module_ref.get::<MacroCatsService>()?;
        Ok(vec![Arc::new(MacroCatsController { cats }).controller()?])
    }

    fn gateways(&self, module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        let cats = module_ref.get::<MacroCatsService>()?;
        Ok(vec![Arc::new(MacroCatsGateway { cats }).gateway()?])
    }

    fn message_patterns(&self, module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        let cats = module_ref.get::<MacroCatsService>()?;
        Arc::new(MacroCatsMessages { cats }).message_patterns()
    }
}

#[derive(Debug)]
struct MacroValidationController;

#[controller("/macro-validation")]
#[validate]
impl MacroValidationController {
    #[post("/", status = 201)]
    async fn create(&self, #[body] dto: MacroValidatedCreateDto) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "validated".to_string(),
            name: dto.name,
        })
    }

    #[get("/search")]
    async fn search(&self, #[query] query: MacroValidatedSearch) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: query.page.to_string(),
            name: "search".to_string(),
        })
    }

    #[get("/skip")]
    #[skip_validation]
    async fn skipped(&self, #[query] query: MacroValidatedSearch) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: query.page.to_string(),
            name: "skipped".to_string(),
        })
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct MacroValidatedCreateDto {
    name: String,
}

impl Validate for MacroValidatedCreateDto {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct MacroValidatedSearch {
    page: u16,
}

impl Validate for MacroValidatedSearch {
    fn validate(&self) -> Result<()> {
        if self.page == 0 {
            return Err(BootError::BadRequest(
                "page must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct MacroValidationModule;

impl Module for MacroValidationModule {
    fn name(&self) -> &'static str {
        "macro-validation"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![
            Arc::new(MacroValidationController).controller()?,
            Arc::new(MacroRouteValidationController).controller()?,
        ])
    }
}

#[derive(Debug)]
struct MacroRouteValidationController;

#[controller("/macro-route-validation")]
impl MacroRouteValidationController {
    #[post("/", status = 201)]
    #[validate]
    async fn create(&self, #[body] dto: MacroValidatedCreateDto) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "route".to_string(),
            name: dto.name,
        })
    }
}

#[tokio::test]
async fn macros_register_injectable_services_and_controller_routes() {
    let app = BootApplication::builder()
        .import(MacroCatsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 9);
    assert_eq!(app.gateways().len(), 1);
    assert_eq!(app.message_patterns().len(), 3);

    let text = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42",
        ))
        .await
        .unwrap();
    assert_eq!(text.body_text().unwrap(), "42:Milo");

    let json = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42/json",
        ))
        .await
        .unwrap();
    assert_eq!(
        json.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Milo".to_string(),
        }
    );

    let details = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Get,
                "/macro-cats/42/details?include_toys=true&page=3&tag=quiet",
            )
            .with_header("x-request-id", "req-1"),
        )
        .await
        .unwrap();
    assert_eq!(
        details.body_json::<MacroCatDetailsDto>().unwrap(),
        MacroCatDetailsDto {
            id: "42".to_string(),
            include_toys: Some(true),
            page: 3,
            tag: Some("quiet".to_string()),
            request_id: Some("req-1".to_string()),
        }
    );

    let params = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/params/99/tabby",
        ))
        .await
        .unwrap();
    assert_eq!(
        params.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "99".to_string(),
            name: "tabby".to_string(),
        }
    );

    let raw = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/42/raw-details")
                .with_header("x-request-id", "req-raw")
                .with_header("user-agent", "macro-test"),
        )
        .await
        .unwrap();
    assert_eq!(
        raw.body_text().unwrap(),
        "42:req-raw:macro-test:/macro-cats/42/raw-details"
    );

    let create = BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats")
        .with_json(&MacroCreateCatDto {
            name: "Luna".to_string(),
        })
        .unwrap();
    let created = app.call(create).await.unwrap();

    assert_eq!(created.status(), 201);
    assert_eq!(
        created.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "generated".to_string(),
            name: "Luna".to_string(),
        }
    );

    let adopt = BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats/42/adoptions")
        .with_json(&MacroCreateCatDto {
            name: "Nori".to_string(),
        })
        .unwrap();
    let adopted = app.call(adopt).await.unwrap();
    assert_eq!(adopted.status(), 201);
    assert_eq!(
        adopted.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Nori".to_string(),
        }
    );

    let events = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/events")
                .with_header("accept", "text/event-stream"),
        )
        .await
        .unwrap();
    let mut stream = events.into_sse_stream().unwrap();

    assert_eq!(
        String::from_utf8(stream.next().await.unwrap().unwrap().encode()).unwrap(),
        "event: cat.found\ndata: Milo\n\n"
    );
    assert!(stream.next().await.is_none());

    let cat_events = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/42/events")
                .with_header("accept", "text/event-stream"),
        )
        .await
        .unwrap();
    let mut cat_stream = cat_events.into_sse_stream().unwrap();
    assert_eq!(
        String::from_utf8(cat_stream.next().await.unwrap().unwrap().encode()).unwrap(),
        "event: cat.selected\ndata: 42\n\n"
    );
    assert!(cat_stream.next().await.is_none());

    let missing_query = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42/details",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(missing_query, BootError::BadRequest(message) if message == "missing query parameter: page")
    );

    let missing_header = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42/raw-details",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(missing_header, BootError::BadRequest(message) if message == "missing header: x-request-id")
    );

    let document = app.openapi(OpenApiInfo::new("Macro Cats", "1.0.0"));
    let document = serde_json::to_value(document).unwrap();
    let details_operation = &document["paths"]["/macro-cats/{id}/details"]["get"];
    assert_eq!(details_operation["tags"], json!(["macro-cats"]));
    assert_eq!(
        details_operation["operationId"],
        json!("findMacroCatDetails")
    );
    assert_eq!(
        details_operation["summary"],
        json!("Find macro cat details")
    );
    assert_eq!(
        details_operation["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCatDetailsDto" })
    );
    assert_eq!(details_operation["security"][0]["bearerAuth"], json!([]));
    assert!(has_openapi_parameter(
        details_operation,
        "id",
        "path",
        true,
        json!({ "type": "string" }),
    ));
    assert!(has_openapi_parameter(
        details_operation,
        "page",
        "query",
        true,
        json!({ "type": "integer" }),
    ));
    assert!(has_openapi_parameter(
        details_operation,
        "tag",
        "query",
        false,
        json!({ "type": "string" }),
    ));
    assert!(has_openapi_parameter(
        details_operation,
        "x-request-id",
        "header",
        false,
        json!({ "type": "string" }),
    ));

    let create_operation = &document["paths"]["/macro-cats"]["post"];
    assert_eq!(create_operation["operationId"], json!("createMacroCat"));
    assert_eq!(
        create_operation["requestBody"]["description"],
        json!("Cat creation payload")
    );
    assert_eq!(
        create_operation["requestBody"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCreateCatDto" })
    );
    assert_eq!(
        create_operation["responses"]["201"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCatDto" })
    );

    let adopt_operation = &document["paths"]["/macro-cats/{id}/adoptions"]["post"];
    assert_eq!(
        adopt_operation["requestBody"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCreateCatDto" })
    );
    assert!(!document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/macro-cats/events"));

    let ws_reply = app.gateways()[0]
        .dispatch(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/ws"),
            WebSocketMessage::new("cat.find", json!({ "id": "42" })),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(ws_reply.event(), "cat.found");
    assert_eq!(ws_reply.data()["id"], json!("42"));
    assert_eq!(ws_reply.data()["name"], json!("Milo"));

    let message_reply = app
        .dispatch_message(
            TransportMessage::json(
                "macro.cat.find",
                &MacroFindCatMessage {
                    id: "42".to_string(),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message_reply.data_as::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Milo".to_string(),
        }
    );

    let raw_reply = app
        .dispatch_message(TransportMessage::new(
            "macro.cat.raw",
            json!({ "id": "raw" }),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(raw_reply.data(), &json!("macro.cat.raw:raw"));

    let event_reply = app
        .dispatch_message(
            TransportMessage::json(
                "macro.cat.seen",
                &MacroFindCatMessage {
                    id: "42".to_string(),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(event_reply, None);
}

fn has_openapi_parameter(
    operation: &serde_json::Value,
    name: &str,
    location: &str,
    required: bool,
    schema: serde_json::Value,
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

#[tokio::test]
async fn validate_macro_enables_body_and_query_dto_validation() {
    let app = BootApplication::builder()
        .import(MacroValidationModule)
        .build()
        .unwrap();

    let body_error = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-validation")
                .with_json(&MacroValidatedCreateDto {
                    name: " ".to_string(),
                })
                .unwrap(),
        )
        .await
        .unwrap_err();
    let query_error = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-validation/search?page=0",
        ))
        .await
        .unwrap_err();
    let skipped = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-validation/skip?page=0",
        ))
        .await
        .unwrap();
    let route_error = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-route-validation")
                .with_json(&MacroValidatedCreateDto {
                    name: "".to_string(),
                })
                .unwrap(),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(body_error, BootError::BadRequest(message) if message.contains("name is required"))
    );
    assert!(
        matches!(query_error, BootError::BadRequest(message) if message.contains("page must be greater than zero"))
    );
    assert_eq!(
        skipped.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "0".to_string(),
            name: "skipped".to_string(),
        }
    );
    assert!(
        matches!(route_error, BootError::BadRequest(message) if message.contains("name is required"))
    );
}
