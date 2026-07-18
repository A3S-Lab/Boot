#![cfg(feature = "macros")]

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use a3s_boot::{
    controller, injectable, ApiVersioning, BootApplication, BootError, BootErrorKind, BootRequest,
    BootResponse, BoxFuture, CallHandler, ControllerDefinition, ExceptionFilter, ExecutionContext,
    FromModuleRef, Guard, Interceptor, Module, ModuleRef, OpenApiInfo, ParseArrayPipe,
    ParseBoolPipe, ParseEnumPipe, ParseFloatPipe, ParseIntPipe, ParseUuidPipe, Pipe,
    ProviderDefinition, ProviderRef, ProviderScope, ProviderToken, Result, SseEvent, SseStream,
    StringTemplateViewEngine, TransportContext, TransportExceptionFilter,
    TransportExceptionResponse, TransportMessage, TransportReply, UuidVersion, Validate,
    ViewModule, WebSocketContext, WebSocketExceptionFilter, WebSocketExceptionResponse,
    WebSocketGatewayConnection, WebSocketGatewayInitContext, WebSocketGatewayServer,
    WebSocketMessage,
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
struct MacroMissingCatsService;

#[injectable]
#[derive(Debug)]
struct MacroAutoCatsReader {
    cats: Arc<MacroCatsService>,
    #[inject("readonly-cats")]
    readonly: Arc<MacroCatsService>,
    missing: Option<Arc<MacroMissingCatsService>>,
    #[inject("missing-cats")]
    missing_named: Option<Arc<MacroCatsService>>,
    lazy: ProviderRef<MacroCatsService>,
    #[inject("readonly-cats")]
    lazy_readonly: ProviderRef<MacroCatsService>,
    missing_lazy: Option<ProviderRef<MacroMissingCatsService>>,
}

impl MacroAutoCatsReader {
    fn summary(&self) -> String {
        let cat = self.cats.find_one("auto");
        let readonly = self.readonly.find_one("readonly");
        let lazy = self.lazy.get().unwrap().find_one("lazy");
        let lazy_readonly = self.lazy_readonly.get().unwrap().find_one("lazy-readonly");
        format!(
            "{}:{}:{}:{}:{}:{}:{}",
            cat.id,
            readonly.id,
            self.missing.is_none(),
            self.missing_named.is_none(),
            lazy.id,
            lazy_readonly.id,
            self.missing_lazy.is_none(),
        )
    }
}

#[injectable]
#[derive(Debug)]
struct MacroCatsController {
    cats: Arc<MacroCatsService>,
    reader: Arc<MacroAutoCatsReader>,
}

#[injectable]
#[derive(Debug)]
struct MacroCatsGateway {
    cats: Arc<MacroCatsService>,
    lifecycle_log: Arc<MacroGatewayLifecycleLog>,
}

#[injectable]
#[derive(Debug)]
struct MacroCatsMessages {
    cats: Arc<MacroCatsService>,
}

#[derive(Debug, Default)]
struct MacroGatewayLifecycleLog {
    entries: std::sync::Mutex<Vec<String>>,
}

impl MacroGatewayLifecycleLog {
    fn push(&self, entry: impl Into<String>) {
        self.entries.lock().unwrap().push(entry.into());
    }

    fn entries(&self) -> Vec<String> {
        self.entries.lock().unwrap().clone()
    }
}

fn current_tenant(request: &BootRequest) -> Result<String> {
    Ok(request.header("x-tenant").unwrap_or("public").to_string())
}

#[derive(Debug, PartialEq, Eq)]
struct MacroCatId(String);

fn parse_macro_cat_id(value: String) -> Result<MacroCatId> {
    if !value.starts_with("cat_") {
        return Err(BootError::BadRequest(
            "cat id must start with cat_".to_string(),
        ));
    }

    Ok(MacroCatId(value))
}

fn parse_macro_page(value: String) -> Result<u16> {
    let page = value
        .parse::<u16>()
        .map_err(|error| BootError::BadRequest(format!("invalid page: {error}")))?;
    if page == 0 {
        return Err(BootError::BadRequest(
            "page must be greater than zero".to_string(),
        ));
    }

    Ok(page)
}

fn normalize_macro_kind(value: String) -> Result<String> {
    let kind = value.trim();
    if kind.is_empty() {
        return Err(BootError::BadRequest("cat kind is required".to_string()));
    }

    Ok(kind.to_ascii_uppercase())
}

fn normalize_macro_ip(value: String) -> Result<String> {
    Ok(format!("ip:{value}"))
}

#[derive(Debug, PartialEq, Eq)]
enum MacroCatKind {
    Tabby,
    Tuxedo,
}

impl MacroCatKind {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Tabby => "tabby",
            Self::Tuxedo => "tuxedo",
        }
    }
}

impl FromStr for MacroCatKind {
    type Err = String;

    fn from_str(value: &str) -> std::result::Result<Self, Self::Err> {
        match value {
            "tabby" => Ok(Self::Tabby),
            "tuxedo" => Ok(Self::Tuxedo),
            _ => Err(format!("unknown cat kind: {value}")),
        }
    }
}

#[a3s_boot::websocket_gateway("/macro-cats/ws", namespace = "/macro-cats")]
#[a3s_boot::metadata("resource", "macro-gateway")]
impl MacroCatsGateway {
    #[a3s_boot::on_gateway_init]
    async fn after_init(
        &self,
        context: WebSocketGatewayInitContext,
        server: WebSocketGatewayServer,
    ) -> Result<()> {
        self.lifecycle_log.push(format!(
            "init:{}:{}:{}",
            context.gateway_path,
            context.events.join(","),
            server.active_connection_count()?
        ));
        Ok(())
    }

    #[a3s_boot::on_gateway_connection]
    async fn handle_connection(&self, connection: WebSocketGatewayConnection) -> Result<()> {
        self.lifecycle_log
            .push(format!("connect:{}", connection.request().path()));
        Ok(())
    }

    #[a3s_boot::subscribe_message("cat.find")]
    #[a3s_boot::metadata("action", "find")]
    async fn find(&self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        let id = message
            .data()
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let cat = self.cats.find_one(id);
        WebSocketMessage::json("cat.found", &cat)
    }

    #[a3s_boot::subscribe_message("cat.typed")]
    async fn typed(&self, payload: MacroFindCatMessage) -> Result<WebSocketMessage> {
        let cat = self.cats.find_one(&payload.id);
        WebSocketMessage::json("cat.typed", &cat)
    }

    #[a3s_boot::subscribe_message("cat.server")]
    async fn server(&self, server: WebSocketGatewayServer) -> Result<WebSocketMessage> {
        Ok(WebSocketMessage::new(
            "cat.server",
            json!({
                "path": server.path(),
                "namespace": server.namespace(),
                "connections": server.active_connection_count()?,
            }),
        ))
    }

    #[a3s_boot::subscribe_message("cat.field")]
    async fn field(
        &self,
        #[a3s_boot::message_body("id")] id: String,
        #[a3s_boot::message_body("page", default = 1, pipe = ParseIntPipe)] page: u16,
        #[a3s_boot::message_body("tag")] tag: Option<String>,
    ) -> Result<WebSocketMessage> {
        Ok(WebSocketMessage::new(
            "cat.field",
            json!({
                "id": id,
                "page": page,
                "tag": tag.unwrap_or_else(|| "none".to_string()),
            }),
        ))
    }

    #[a3s_boot::subscribe_message("cat.connection")]
    async fn connection(
        &self,
        connection: WebSocketGatewayConnection,
        payload: MacroFindCatMessage,
    ) -> Result<WebSocketMessage> {
        Ok(WebSocketMessage::new(
            "cat.connection",
            json!({
                "connectionId": connection.id(),
                "path": connection.request().path(),
                "id": payload.id,
            }),
        ))
    }

    #[a3s_boot::subscribe_message("cat.create")]
    #[a3s_boot::validate(transform)]
    async fn create(&self, payload: MacroTransformCreateDto) -> Result<WebSocketMessage> {
        Ok(WebSocketMessage::new(
            "cat.created",
            json!({
                "name": payload.name,
                "kind": payload.kind,
            }),
        ))
    }

    #[a3s_boot::subscribe_message("cat.validate")]
    #[a3s_boot::validate]
    async fn validate(&self, payload: MacroValidatedCreateDto) -> Result<WebSocketMessage> {
        Ok(WebSocketMessage::new(
            "cat.validated",
            json!({ "name": payload.name }),
        ))
    }

    #[a3s_boot::subscribe_message("cat.fail")]
    #[a3s_boot::use_filter(MacroBadRequestFilter::catch_filter())]
    async fn fail(&self) -> Result<WebSocketMessage> {
        Err(BootError::BadRequest("macro websocket filter".to_string()))
    }

    #[a3s_boot::on_gateway_disconnect]
    async fn handle_disconnect(&self, connection: WebSocketGatewayConnection) -> Result<()> {
        self.lifecycle_log
            .push(format!("disconnect:{}", connection.request().path()));
        Ok(())
    }
}

#[a3s_boot::message_controller]
#[a3s_boot::metadata("resource", "macro-messages")]
impl MacroCatsMessages {
    #[a3s_boot::message_pattern("macro.cat.find")]
    #[a3s_boot::metadata("action", "find")]
    async fn find(&self, payload: MacroFindCatMessage) -> Result<MacroCatDto> {
        Ok(self.cats.find_one(&payload.id))
    }

    #[a3s_boot::message_pattern("macro.cat.field")]
    async fn field(
        &self,
        #[a3s_boot::payload("id")] id: String,
        #[a3s_boot::payload("page", default = 1, pipe = ParseIntPipe)] page: u16,
        #[a3s_boot::payload("tag")] tag: Option<String>,
    ) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id,
            name: format!("{}:{}", page, tag.unwrap_or_else(|| "none".to_string())),
        })
    }

    #[a3s_boot::message_pattern("macro.cat.raw", raw)]
    async fn raw(&self, message: TransportMessage) -> Result<TransportReply> {
        Ok(TransportReply::text(format!(
            "{}:{}",
            message.pattern(),
            message.data()["id"].as_str().unwrap_or("unknown")
        )))
    }

    #[a3s_boot::message_pattern("macro.cat.fail")]
    #[a3s_boot::use_filter(MacroBadRequestFilter::catch_filter())]
    async fn fail(&self) -> Result<MacroCatDto> {
        Err(BootError::BadRequest("macro message filter".to_string()))
    }

    #[a3s_boot::event_pattern("macro.cat.seen")]
    async fn seen(&self, payload: MacroFindCatMessage) -> Result<()> {
        let _ = self.cats.find_one(&payload.id);
        Ok(())
    }
}

#[controller("/macro-cats")]
#[tag("macro-cats")]
#[api_extra_model(
    name = "MacroCatPageDto",
    schema = ::a3s_boot::OpenApiSchema::all_of([
        ::a3s_boot::OpenApiSchema::object_with_properties(
            [(
                "items",
                ::a3s_boot::OpenApiSchema::array(
                    ::a3s_boot::OpenApiSchema::reference("MacroCatDto")
                )
            )],
            ["items"]
        ),
        ::a3s_boot::OpenApiSchema::object()
            .with_property("next_cursor", ::a3s_boot::OpenApiSchema::string().nullable())
            .with_property("status", ::a3s_boot::OpenApiSchema::string_enum(["fresh", "stale"]))
    ])
)]
#[api_extension(
    name = "x-controller-default",
    value = json!({ "source": "controller" })
)]
#[metadata("resource", "cats")]
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

    #[get("/{id}/card")]
    #[render("cats/card")]
    async fn find_one_card(&self, #[param("id")] id: String) -> Result<MacroCatDto> {
        Ok(self.cats.find_one(&id))
    }

    #[get("/{id}/details")]
    #[metadata("roles", ["admin"])]
    #[operation(
        summary = "Find macro cat details",
        description = "Returns a macro cat with query and header metadata.",
        operation_id = "findMacroCatDetails",
        server_url = "https://edge.example.com",
        server_description = "Edge",
        external_docs_description = "Macro cat details guide",
        external_docs_url = "https://docs.example.com/macro-cats/details"
    )]
    #[response(
        status = 200,
        description = "Cat details",
        schema = MacroCatDetailsDto
    )]
    #[a3s_boot::api_param(name = "id", schema = String, description = "Cat identifier")]
    #[a3s_boot::api_query(
        name = "include_toys",
        schema = bool,
        required = false,
        description = "Include toy data",
        deprecated = true,
        allow_reserved = true,
        style = "form",
        explode = false,
        example_name = "with_toys",
        example = json!(true)
    )]
    #[a3s_boot::api_header(
        name = "x-request-id",
        schema = String,
        required = false,
        description = "Request correlation id"
    )]
    #[api_response_header(
        status = 200,
        name = "x-rate-limit-remaining",
        schema = u16,
        description = "Remaining requests"
    )]
    #[api_extension(
        name = "x-codeSamples",
        value = json!([{ "lang": "bash", "source": "curl /macro-cats/42/details" }])
    )]
    #[bearer_auth]
    async fn details(
        &self,
        #[param("id")] id: String,
        #[query] query: MacroFindCatQuery,
        #[query("page")] page: u16,
        #[query("tag")] tag: Option<String>,
        #[header("x-request-id")] request_id: Option<String>,
        #[extract(current_tenant)] tenant: String,
    ) -> Result<MacroCatDetailsDto> {
        Ok(MacroCatDetailsDto {
            id,
            include_toys: query.include_toys,
            page,
            tag,
            request_id,
            tenant,
        })
    }

    #[get("/pipe/{id}")]
    async fn piped_extractors(
        &self,
        #[param("id", pipe = parse_macro_cat_id)] id: MacroCatId,
        #[query("page", pipe = parse_macro_page)] page: Option<u16>,
        #[header("x-cat-kind", pipe = normalize_macro_kind)] kind: String,
    ) -> Result<MacroCatDto> {
        let page = page
            .map(|page| page.to_string())
            .unwrap_or_else(|| "none".to_string());
        Ok(MacroCatDto {
            id: id.0,
            name: format!("{kind}:{page}"),
        })
    }

    #[get("/builtin-pipes/{id}")]
    async fn builtin_pipes(
        &self,
        #[param("id", pipe = ParseIntPipe)] id: u64,
        #[query("active", pipe = ParseBoolPipe)] active: bool,
        #[query("ratio", pipe = ParseFloatPipe)] ratio: Option<f64>,
        #[query("page", default = 1, pipe = ParseIntPipe)] page: u16,
        #[header("x-retry", default = 3, pipe = ParseIntPipe)] retry: u8,
    ) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: id.to_string(),
            name: format!(
                "{}:{}:{}:{}",
                active,
                ratio
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "none".to_string()),
                page,
                retry
            ),
        })
    }

    #[get("/uuid/{id}")]
    async fn uuid_pipe(
        &self,
        #[param("id", pipe = ParseUuidPipe)] id: String,
        #[query("request", pipe = ParseUuidPipe::version(UuidVersion::V4))] request: String,
    ) -> Result<MacroCatDto> {
        Ok(MacroCatDto { id, name: request })
    }

    #[get("/array-enum")]
    async fn array_enum_pipe(
        &self,
        #[query("ids", pipe = ParseArrayPipe)] ids: Vec<u16>,
        #[query("kind", pipe = ParseEnumPipe)] kind: MacroCatKind,
    ) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: ids
                .into_iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>()
                .join("|"),
            name: kind.as_str().to_string(),
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

    #[all("/catch")]
    async fn catch_all(&self, #[request] request: BootRequest) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: request.method().as_str().to_string(),
            name: "Catch".to_string(),
        })
    }

    #[all("/raw-catch", raw)]
    async fn raw_catch_all(&self, request: BootRequest) -> Result<BootResponse> {
        Ok(BootResponse::text(format!(
            "raw:{}",
            request.method().as_str()
        )))
    }

    #[post("/", status = 201)]
    #[operation(summary = "Create a macro cat", operation_id = "createMacroCat")]
    #[request_body(
        schema = MacroCreateCatDto,
        description = "Cat creation payload",
        example_name = "milo",
        example = json!({ "name": "Milo" })
    )]
    #[response(
        status = 201,
        description = "Cat created",
        schema = MacroCatDto,
        example_name = "created",
        example = json!({ "id": "generated", "name": "Milo" })
    )]
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

    #[post("/imports", status = 202)]
    #[request_body(
        content_type = "multipart/form-data",
        schema = MacroCreateCatDto,
        description = "Cat import form"
    )]
    #[response(
        status = 202,
        description = "Cat import accepted",
        content_type = "application/vnd.a3s.cat+json",
        schema = MacroCatDto,
        example = json!({ "id": "imported", "name": "Milo" })
    )]
    async fn import(&self, dto: MacroCreateCatDto) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "imported".to_string(),
            name: dto.name,
        })
    }

    #[post("/{id}/touch")]
    #[http_code(202)]
    async fn touch(&self, #[param("id")] id: String) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id,
            name: "Touched".to_string(),
        })
    }

    #[get("/cache")]
    #[header("cache-control", "max-age=60")]
    async fn cached(&self) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "cache".to_string(),
            name: "Cached".to_string(),
        })
    }

    #[get("/legacy")]
    #[redirect("/macro-cats/42", status = 301)]
    async fn legacy(&self) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "legacy".to_string(),
            name: "Legacy".to_string(),
        })
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
    tenant: String,
}

#[a3s_boot::module(
    name = "macro-cats",
    imports = [
        ViewModule::new(
            "macro-views",
            StringTemplateViewEngine::new()
                .with_template("cats/card", "<article>{{ id }}:{{ name }}</article>"),
        )
    ],
    providers = [
        MacroCatsService,
        MacroCatsService.into_named_provider("readonly-cats"),
        ProviderDefinition::singleton(MacroGatewayLifecycleLog::default()),
        MacroAutoCatsReader,
        MacroCatsController,
        MacroCatsGateway,
        MacroCatsMessages,
    ],
    controllers = [MacroCatsController],
    gateways = [MacroCatsGateway],
    message_controllers = [MacroCatsMessages],
    exports = [MacroCatsService, "readonly-cats"],
)]
#[derive(Debug)]
struct MacroCatsModule;

#[a3s_boot::module(
    name = "macro-provider-flavors",
    imports = [
        ViewModule::new(
            "macro-provider-views",
            StringTemplateViewEngine::new()
                .with_template("cats/card", "<article>{{ id }}:{{ name }}</article>"),
        )
    ],
    providers = [
        MacroCatsService,
        MacroCatsService.into_named_provider("readonly-cats"),
        MacroAutoCatsReader,
        MacroCatsController::request_scoped_provider(),
    ],
    controllers = [MacroCatsController],
)]
#[derive(Debug)]
struct MacroProviderFlavorModule;

#[derive(Debug)]
struct MacroHiddenOpenApiController;

#[controller("/macro-hidden-openapi")]
#[hide_from_openapi]
impl MacroHiddenOpenApiController {
    #[get("/")]
    async fn hidden(&self) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "hidden".to_string(),
            name: "Hidden".to_string(),
        })
    }
}

#[derive(Debug)]
struct MacroHiddenOpenApiModule;

impl Module for MacroHiddenOpenApiModule {
    fn name(&self) -> &'static str {
        "macro-hidden-openapi"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(MacroHiddenOpenApiController).controller()?])
    }
}

#[injectable]
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

#[derive(Debug, Deserialize, Serialize, a3s_boot::ValidationSchema)]
struct MacroWhitelistCreateDto {
    name: String,
    #[serde(rename = "displayName")]
    display_name: Option<String>,
}

impl Validate for MacroWhitelistCreateDto {}

#[derive(Debug, Deserialize, Serialize, a3s_boot::ValidationSchema)]
struct MacroTransformCreateDto {
    name: String,
    #[serde(default = "macro_default_kind")]
    kind: String,
}

impl Validate for MacroTransformCreateDto {}

fn macro_default_kind() -> String {
    "cat".to_string()
}

#[injectable]
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

#[injectable]
#[derive(Debug)]
struct MacroWhitelistValidationController;

#[controller("/macro-whitelist-validation")]
impl MacroWhitelistValidationController {
    #[post("/", status = 201)]
    #[validate(whitelist)]
    async fn create(
        &self,
        #[request] request: BootRequest,
        #[body] _dto: MacroWhitelistCreateDto,
    ) -> Result<serde_json::Value> {
        request.json::<serde_json::Value>()
    }

    #[post("/strict", status = 201)]
    #[validate(forbidNonWhitelisted)]
    async fn create_strict(&self, #[body] dto: MacroWhitelistCreateDto) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id: "strict".to_string(),
            name: dto.name,
        })
    }

    #[post("/transform", status = 201)]
    #[validate(transform)]
    async fn create_transform(
        &self,
        #[request] request: BootRequest,
        #[body] _dto: MacroTransformCreateDto,
    ) -> Result<serde_json::Value> {
        request.json::<serde_json::Value>()
    }
}

#[a3s_boot::module(
    name = "macro-validation",
    providers = [
        MacroValidationController::request_scoped_provider(),
        MacroRouteValidationController::request_scoped_provider(),
        MacroWhitelistValidationController::request_scoped_provider(),
    ],
    controllers = [
        MacroValidationController,
        MacroRouteValidationController,
        MacroWhitelistValidationController,
    ],
)]
#[derive(Debug)]
struct MacroValidationModule;

struct MacroControllerHeaderInterceptor;

impl Interceptor for MacroControllerHeaderInterceptor {
    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(async move { Ok(response.with_header("x-macro-controller", "yes")) })
    }
}

struct MacroAllowGuard;

impl Guard for MacroAllowGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

struct MacroRequestPipe;

impl Pipe for MacroRequestPipe {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        Box::pin(async move { Ok(request.with_header("x-macro-pipe", "yes")) })
    }
}

#[a3s_boot::catch(BadRequest)]
#[derive(Default)]
struct MacroBadRequestFilter;

impl ExceptionFilter for MacroBadRequestFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(499),
            ))
        })
    }
}

impl TransportExceptionFilter for MacroBadRequestFilter {
    fn catch(
        &self,
        context: TransportContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<TransportExceptionResponse>>> {
        Box::pin(async move {
            let reply = TransportReply::json(&json!({
                "pattern": context.pattern,
                "message": error.to_string(),
            }))?;
            Ok(Some(TransportExceptionResponse::reply(reply)))
        })
    }
}

impl WebSocketExceptionFilter for MacroBadRequestFilter {
    fn catch(
        &self,
        context: WebSocketContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<WebSocketExceptionResponse>>> {
        Box::pin(async move {
            Ok(Some(WebSocketExceptionResponse::message(
                WebSocketMessage::new(
                    "macro.error",
                    json!({
                        "event": context.event,
                        "message": error.to_string(),
                    }),
                ),
            )))
        })
    }
}

#[a3s_boot::catch(Conflict)]
#[derive(Default)]
struct MacroConflictFilter;

impl ExceptionFilter for MacroConflictFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        Box::pin(async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(409),
            ))
        })
    }
}

#[injectable]
#[derive(Debug)]
struct MacroPipelineController;

#[controller("/macro-pipeline")]
#[use_interceptor(MacroControllerHeaderInterceptor)]
impl MacroPipelineController {
    #[get("/guarded")]
    #[use_guard(MacroAllowGuard)]
    async fn guarded(&self) -> Result<String> {
        Ok("guarded".to_string())
    }

    #[get("/piped", raw)]
    #[use_pipe(MacroRequestPipe)]
    async fn piped(&self, request: BootRequest) -> Result<BootResponse> {
        Ok(BootResponse::text(
            request.header("x-macro-pipe").unwrap_or("missing"),
        ))
    }

    #[get("/filtered")]
    #[use_filter(MacroBadRequestFilter::catch_filter())]
    async fn filtered(&self) -> Result<String> {
        Err(BootError::BadRequest("macro filter".to_string()))
    }

    #[get("/filtered-conflict")]
    #[use_filter(MacroConflictFilter::catch_filter())]
    async fn filtered_conflict(&self) -> Result<String> {
        Err(BootError::Conflict("macro conflict".to_string()))
    }

    #[get("/unfiltered")]
    #[use_filter(MacroBadRequestFilter::catch_filter())]
    async fn unfiltered(&self) -> Result<String> {
        Err(BootError::Unauthorized("macro private".to_string()))
    }
}

#[a3s_boot::module(
    name = "macro-pipeline",
    providers = [MacroPipelineController::request_scoped_provider()],
    controllers = [MacroPipelineController],
)]
#[derive(Debug)]
struct MacroPipelineModule;

#[derive(Debug)]
struct MacroHostController;

#[derive(Debug, Deserialize, PartialEq, Serialize)]
struct MacroHostDto {
    tenant: String,
    ip: Option<String>,
}

#[controller("/macro-host")]
#[host("{tenant}.example.com")]
impl MacroHostController {
    #[get("/who")]
    async fn who(
        &self,
        #[host_param("tenant")] tenant: String,
        #[ip] ip: Option<String>,
    ) -> Result<MacroHostDto> {
        Ok(MacroHostDto { tenant, ip })
    }

    #[get("/api")]
    #[host("api.example.com")]
    async fn api(&self, #[ip] ip: Option<String>) -> Result<String> {
        Ok(ip.unwrap_or_else(|| "missing".to_string()))
    }

    #[get("/pipe-ip")]
    #[host("api.example.com")]
    async fn pipe_ip(&self, #[ip(pipe = normalize_macro_ip)] ip: Option<String>) -> Result<String> {
        Ok(ip.unwrap_or_else(|| "missing".to_string()))
    }
}

#[derive(Debug)]
struct MacroHostModule;

impl Module for MacroHostModule {
    fn name(&self) -> &'static str {
        "macro-host"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(MacroHostController).controller()?])
    }
}

#[derive(Debug)]
struct MacroVersionController;

#[controller("/macro-version")]
#[version("1")]
impl MacroVersionController {
    #[get("/cats")]
    async fn cats_v1(&self) -> Result<String> {
        Ok("v1".to_string())
    }

    #[get("/cats")]
    #[version("2")]
    async fn cats_v2(&self) -> Result<String> {
        Ok("v2".to_string())
    }

    #[get("/health")]
    #[version_neutral]
    async fn health(&self) -> Result<String> {
        Ok("ok".to_string())
    }

    #[get("/multi")]
    #[versions("2", "3")]
    async fn multi(&self) -> Result<String> {
        Ok("multi".to_string())
    }
}

#[derive(Debug)]
struct MacroVersionModule;

impl Module for MacroVersionModule {
    fn name(&self) -> &'static str {
        "macro-version"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(MacroVersionController).controller()?])
    }
}

#[derive(Debug)]
struct MacroSerializationController;

#[controller("/macro-serialization")]
#[serialize(exclude = ["password"], skip_null)]
impl MacroSerializationController {
    #[get("/user")]
    async fn user(&self) -> Result<serde_json::Value> {
        Ok(json!({
            "id": "u1",
            "email": "milo@example.com",
            "password": "secret",
            "nickname": null
        }))
    }

    #[get("/public")]
    #[serialize(include = ["id", "email"])]
    async fn public_user(&self) -> Result<serde_json::Value> {
        Ok(json!({
            "id": "u1",
            "email": "milo@example.com",
            "password": "secret"
        }))
    }
}

#[derive(Debug)]
struct MacroSerializationModule;

impl Module for MacroSerializationModule {
    fn name(&self) -> &'static str {
        "macro-serialization"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(MacroSerializationController).controller()?])
    }
}

#[injectable]
#[derive(Debug)]
struct MacroPrefixedController;

#[controller("/dogs")]
impl MacroPrefixedController {
    #[get("/{id}")]
    async fn find(&self, #[param("id")] id: String) -> Result<MacroCatDto> {
        Ok(MacroCatDto {
            id,
            name: "Rex".to_string(),
        })
    }
}

#[a3s_boot::module(
    name = "macro-prefixed",
    route_prefix = "/api",
    providers = [MacroPrefixedController],
    controllers = [MacroPrefixedController]
)]
#[derive(Debug)]
struct MacroPrefixedModule;

#[injectable]
#[derive(Debug)]
struct MacroForwardRootService;

#[injectable]
#[derive(Debug)]
struct MacroForwardFeatureService {
    root: ProviderRef<MacroForwardRootService>,
}

#[a3s_boot::module(
    name = "macro-forward-root",
    forward_imports = [MacroForwardFeatureModule],
    providers = [MacroForwardRootService],
    exports = [MacroForwardRootService, MacroForwardFeatureService],
)]
#[derive(Debug)]
struct MacroForwardRootModule;

#[a3s_boot::module(
    name = "macro-forward-feature",
    forward_imports = [MacroForwardRootModule],
    providers = [MacroForwardFeatureService],
    exports = [MacroForwardFeatureService],
)]
#[derive(Debug)]
struct MacroForwardFeatureModule;

static MACRO_BUBBLED_STATE_CALLS: AtomicUsize = AtomicUsize::new(0);
static MACRO_REQUEST_STATE_CALLS: AtomicUsize = AtomicUsize::new(0);
static MACRO_RETRY_CONTROLLER_CALLS: AtomicUsize = AtomicUsize::new(0);
static MACRO_RETRY_CONTROLLER_ATTEMPTS: Mutex<Vec<usize>> = Mutex::new(Vec::new());

#[derive(Debug)]
struct MacroBubbledState {
    id: usize,
}

#[injectable]
#[derive(Debug)]
struct MacroBubbledController {
    first: Arc<MacroBubbledState>,
    second: Arc<MacroBubbledState>,
}

#[controller("/macro-bubbled-controller")]
#[a3s_boot::metadata("controller-scope", "bubbled")]
#[a3s_boot::use_interceptor(MacroControllerHeaderInterceptor)]
impl MacroBubbledController {
    #[a3s_boot::get("/", raw)]
    #[a3s_boot::metadata("route-scope", "bubbled")]
    async fn current(&self) -> Result<BootResponse> {
        Ok(BootResponse::text(format!(
            "{}:{}:{}",
            self.first.id,
            self.second.id,
            Arc::ptr_eq(&self.first, &self.second),
        )))
    }
}

#[derive(Debug)]
struct MacroRequestState {
    id: usize,
}

#[injectable]
#[derive(Debug)]
struct MacroRequestController {
    state: Arc<MacroRequestState>,
}

#[controller("/macro-request-controller")]
impl MacroRequestController {
    #[a3s_boot::get("/", raw)]
    async fn current(&self) -> Result<BootResponse> {
        Ok(BootResponse::text(self.state.id.to_string()))
    }
}

#[derive(Debug)]
struct MacroRetryRequestController {
    id: usize,
    attempts: AtomicUsize,
}

impl FromModuleRef for MacroRetryRequestController {
    fn from_module_ref(_module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            id: MACRO_RETRY_CONTROLLER_CALLS.fetch_add(1, Ordering::SeqCst) + 1,
            attempts: AtomicUsize::new(0),
        })
    }

    fn provider_dependencies() -> Option<Vec<a3s_boot::ProviderDependency>> {
        Some(Vec::new())
    }
}

struct MacroRetryOnceInterceptor;

impl Interceptor for MacroRetryOnceInterceptor {
    fn intercept<'a>(
        &'a self,
        _context: ExecutionContext,
        next: CallHandler<'a>,
    ) -> BoxFuture<'a, Result<BootResponse>> {
        Box::pin(async move {
            match next.handle().await {
                Ok(response) => Ok(response),
                Err(_) => next.handle().await,
            }
        })
    }
}

#[controller("/macro-retry-request-controller")]
#[a3s_boot::use_interceptor(MacroRetryOnceInterceptor)]
impl MacroRetryRequestController {
    #[a3s_boot::get("/", raw)]
    async fn current(&self) -> Result<BootResponse> {
        MACRO_RETRY_CONTROLLER_ATTEMPTS
            .lock()
            .unwrap()
            .push(self.id);
        if self.attempts.fetch_add(1, Ordering::SeqCst) == 0 {
            return Err(BootError::Internal("retry controller".to_string()));
        }

        Ok(BootResponse::text(self.id.to_string()))
    }
}

#[a3s_boot::module(
    name = "macro-provider-controllers",
    providers = [
        ProviderDefinition::request_scoped::<MacroBubbledState, _>(|_| {
            Ok(MacroBubbledState {
                id: MACRO_BUBBLED_STATE_CALLS.fetch_add(1, Ordering::SeqCst) + 1,
            })
        }),
        ProviderDefinition::request_scoped::<MacroRequestState, _>(|_| {
            Ok(MacroRequestState {
                id: MACRO_REQUEST_STATE_CALLS.fetch_add(1, Ordering::SeqCst) + 1,
            })
        }),
        MacroBubbledController,
        MacroRequestController::request_scoped_provider(),
        ProviderDefinition::request_scoped_injectable::<MacroRetryRequestController>(),
    ],
    controllers = [
        MacroBubbledController,
        MacroRequestController,
        MacroRetryRequestController,
    ],
)]
#[derive(Debug)]
struct MacroProviderControllerModule;

#[test]
fn injectable_macros_describe_provider_dependencies() {
    let unit = MacroCatsService::provider();
    assert_eq!(unit.dependencies(), Some(&[][..]));

    let reader = MacroAutoCatsReader::provider();
    let dependencies = reader.dependencies().unwrap();
    let actual = dependencies
        .iter()
        .map(|dependency| {
            (
                dependency.token().clone(),
                dependency.is_optional(),
                dependency.is_lazy(),
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        actual,
        vec![
            (ProviderToken::of::<MacroCatsService>(), false, false),
            (ProviderToken::named("readonly-cats"), false, false),
            (ProviderToken::of::<MacroMissingCatsService>(), true, false,),
            (ProviderToken::named("missing-cats"), true, false),
            (ProviderToken::of::<MacroCatsService>(), false, true),
            (ProviderToken::named("readonly-cats"), false, true),
            (ProviderToken::of::<MacroMissingCatsService>(), true, true,),
        ]
    );

    let named = MacroAutoCatsReader::named_provider("reader");
    assert_eq!(named.dependencies(), reader.dependencies());
}

#[tokio::test]
async fn module_macros_use_provider_backed_contextual_controllers() {
    MACRO_BUBBLED_STATE_CALLS.store(0, Ordering::SeqCst);
    MACRO_REQUEST_STATE_CALLS.store(0, Ordering::SeqCst);
    let app = BootApplication::builder()
        .import(MacroProviderControllerModule)
        .build()
        .unwrap();

    assert!(app
        .module_ref()
        .provider_is_contextual::<MacroBubbledController>()
        .unwrap());
    assert_eq!(
        app.module_ref()
            .provider_scope::<MacroBubbledController>()
            .unwrap(),
        ProviderScope::Singleton,
    );
    assert_eq!(
        app.module_ref()
            .provider_scope::<MacroRequestController>()
            .unwrap(),
        ProviderScope::Request,
    );
    let first_bubbled = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-bubbled-controller",
        ))
        .await
        .unwrap();
    let second_bubbled = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-bubbled-controller",
        ))
        .await
        .unwrap();
    let first_request = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-request-controller",
        ))
        .await
        .unwrap();
    let second_request = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-request-controller",
        ))
        .await
        .unwrap();

    assert_eq!(first_bubbled.body_text().unwrap(), "1:1:true");
    assert_eq!(first_bubbled.header("x-macro-controller"), Some("yes"));
    assert_eq!(second_bubbled.body_text().unwrap(), "2:2:true");
    assert_eq!(first_request.body_text().unwrap(), "1");
    assert_eq!(second_request.body_text().unwrap(), "2");
    assert_eq!(MACRO_BUBBLED_STATE_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(MACRO_REQUEST_STATE_CALLS.load(Ordering::SeqCst), 2);

    let first_scope = app.module_ref().request_scope();
    let first_bubbled_controller = first_scope.get::<MacroBubbledController>().unwrap();
    let repeated_bubbled_controller = first_scope.get::<MacroBubbledController>().unwrap();
    let first_request_controller = first_scope.get::<MacroRequestController>().unwrap();
    let repeated_request_controller = first_scope.get::<MacroRequestController>().unwrap();
    assert!(Arc::ptr_eq(
        &first_bubbled_controller,
        &repeated_bubbled_controller,
    ));
    assert!(Arc::ptr_eq(
        &first_request_controller,
        &repeated_request_controller,
    ));

    let second_scope = app.module_ref().request_scope();
    let second_bubbled_controller = second_scope.get::<MacroBubbledController>().unwrap();
    let second_request_controller = second_scope.get::<MacroRequestController>().unwrap();
    assert!(!Arc::ptr_eq(
        &first_bubbled_controller,
        &second_bubbled_controller,
    ));
    assert!(!Arc::ptr_eq(
        &first_request_controller,
        &second_request_controller,
    ));

    assert_eq!(
        app.reflector().unwrap().metadata_value(
            a3s_boot::HttpMethod::Get,
            "/macro-bubbled-controller",
            "controller-scope",
        ),
        Some(&json!("bubbled")),
    );
    assert_eq!(
        app.reflector().unwrap().metadata_value(
            a3s_boot::HttpMethod::Get,
            "/macro-bubbled-controller",
            "route-scope",
        ),
        Some(&json!("bubbled")),
    );
}

#[tokio::test]
async fn interceptor_retry_reuses_the_request_scoped_controller_instance() {
    MACRO_RETRY_CONTROLLER_CALLS.store(0, Ordering::SeqCst);
    MACRO_RETRY_CONTROLLER_ATTEMPTS.lock().unwrap().clear();
    let app = BootApplication::builder()
        .import(MacroProviderControllerModule)
        .build()
        .unwrap();

    assert_eq!(
        app.module_ref()
            .provider_scope::<MacroRetryRequestController>()
            .unwrap(),
        ProviderScope::Request,
    );

    let first = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-retry-request-controller",
        ))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-retry-request-controller",
        ))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1");
    assert_eq!(second.body_text().unwrap(), "2");
    assert_eq!(MACRO_RETRY_CONTROLLER_CALLS.load(Ordering::SeqCst), 2);
    assert_eq!(
        *MACRO_RETRY_CONTROLLER_ATTEMPTS.lock().unwrap(),
        [1, 1, 2, 2],
    );
}

#[tokio::test]
async fn provider_backed_controllers_support_all_http_handler_flavors() {
    let app = BootApplication::builder()
        .import(MacroProviderFlavorModule)
        .build()
        .unwrap();

    let raw = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/raw-id",
        ))
        .await
        .unwrap();
    assert_eq!(raw.body_text().unwrap(), "raw-id:Milo");

    let json_response = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/json-id/json")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    assert_eq!(
        json_response.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "json-id".to_string(),
            name: "Milo".to_string(),
        },
    );

    let json_body = BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats")
        .with_json(&MacroCreateCatDto {
            name: "Nori".to_string(),
        })
        .unwrap();
    let created = app.call(json_body).await.unwrap();
    assert_eq!(created.status(), 201);
    assert_eq!(
        created.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "generated".to_string(),
            name: "Nori".to_string(),
        },
    );

    let extracted = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Get,
                "/macro-cats/pipe/cat_extracted?page=3",
            )
            .with_header("x-cat-kind", "  TABBY  "),
        )
        .await
        .unwrap();
    assert_eq!(
        extracted.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "cat_extracted".to_string(),
            name: "TABBY:3".to_string(),
        },
    );

    let extracted_raw = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Get,
                "/macro-cats/extracted-raw/raw-details",
            )
            .with_header("x-request-id", "provider-request")
            .with_header("user-agent", "provider-controller-test"),
        )
        .await
        .unwrap();
    assert_eq!(
        extracted_raw.body_text().unwrap(),
        "extracted-raw:provider-request:provider-controller-test:/macro-cats/extracted-raw/raw-details",
    );

    let extracted_body =
        BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats/adopted/adoptions")
            .with_json(&MacroCreateCatDto {
                name: "Nori".to_string(),
            })
            .unwrap();
    let adopted = app.call(extracted_body).await.unwrap();
    assert_eq!(adopted.status(), 201);
    assert_eq!(
        adopted.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "adopted".to_string(),
            name: "Nori".to_string(),
        },
    );

    let catch_all = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Patch,
            "/macro-cats/catch",
        ))
        .await
        .unwrap();
    assert_eq!(
        catch_all.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "PATCH".to_string(),
            name: "Catch".to_string(),
        },
    );

    let rendered = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/rendered/card",
        ))
        .await
        .unwrap();
    assert_eq!(
        rendered.body_text().unwrap(),
        "<article>rendered:Milo</article>",
    );

    let cached = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/cache",
        ))
        .await
        .unwrap();
    assert_eq!(cached.header("cache-control"), Some("max-age=60"));

    let redirected = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/legacy",
        ))
        .await
        .unwrap();
    assert_eq!(redirected.status(), 301);
    assert_eq!(redirected.location(), Some("/macro-cats/42"));

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
        "event: cat.found\ndata: Milo\n\n",
    );
    assert!(stream.next().await.is_none());

    let extracted_events = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/provider-sse/events")
                .with_header("accept", "text/event-stream"),
        )
        .await
        .unwrap();
    let mut extracted_stream = extracted_events.into_sse_stream().unwrap();
    assert_eq!(
        String::from_utf8(extracted_stream.next().await.unwrap().unwrap().encode(),).unwrap(),
        "event: cat.selected\ndata: provider-sse\n\n",
    );
    assert!(extracted_stream.next().await.is_none());

    assert_eq!(
        app.reflector().unwrap().metadata_value(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/{id}/details",
            "roles",
        ),
        Some(&json!(["admin"])),
    );
    let document =
        serde_json::to_value(app.openapi(OpenApiInfo::new("Provider", "1.0.0"))).unwrap();
    assert_eq!(
        document["paths"]["/macro-cats/{id}/details"]["get"]["operationId"],
        json!("findMacroCatDetails"),
    );
}

#[tokio::test]
async fn macros_register_injectable_services_and_controller_routes() {
    let app = BootApplication::builder()
        .import(MacroCatsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 20);
    assert_eq!(app.gateways().len(), 1);
    assert_eq!(app.gateways()[0].namespace(), Some("/macro-cats"));
    assert_eq!(app.message_patterns().len(), 5);
    let exports = MacroCatsModule.exports().unwrap();
    assert!(exports.contains(&ProviderToken::of::<MacroCatsService>()));
    assert!(exports.contains(&ProviderToken::named("readonly-cats")));
    let reader = app.get::<MacroAutoCatsReader>().unwrap();
    let controller = app.get::<MacroCatsController>().unwrap();
    let instance_controller = Arc::clone(&controller).controller().unwrap();
    let provider_controller = MacroCatsController::provider_controller().unwrap();
    assert_eq!(
        instance_controller.routes().len(),
        provider_controller.routes().len()
    );
    for (instance_route, provider_route) in instance_controller
        .routes()
        .iter()
        .zip(provider_controller.routes())
    {
        assert_eq!(instance_route.method(), provider_route.method());
        assert_eq!(instance_route.path(), provider_route.path());
        assert_eq!(instance_route.host(), provider_route.host());
        assert_eq!(instance_route.openapi(), provider_route.openapi());
        assert_eq!(instance_route.versioning(), provider_route.versioning());
        assert_eq!(
            instance_route.serialization(),
            provider_route.serialization()
        );
        assert_eq!(instance_route.metadata(), provider_route.metadata());
        assert_eq!(
            instance_route.validation_enabled(),
            provider_route.validation_enabled(),
        );
    }
    assert_eq!(
        reader.summary(),
        "auto:readonly:true:true:lazy:lazy-readonly:true"
    );
    assert!(Arc::ptr_eq(&reader, &controller.reader));
    assert_eq!(
        app.reflector().unwrap().metadata_value(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/{id}/details",
            "roles"
        ),
        Some(&json!(["admin"]))
    );
    assert_eq!(
        app.reflector().unwrap().metadata_value(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/{id}/details",
            "resource"
        ),
        Some(&json!("cats"))
    );
    assert_eq!(
        app.reflector().unwrap().gateway_event_metadata_value(
            "/macro-cats/ws",
            "cat.find",
            "resource"
        ),
        Some(&json!("macro-gateway"))
    );
    assert_eq!(
        app.reflector().unwrap().gateway_event_metadata_value(
            "/macro-cats/ws",
            "cat.find",
            "action"
        ),
        Some(&json!("find"))
    );
    assert_eq!(
        app.reflector()
            .unwrap()
            .message_pattern_metadata_value("macro.cat.find", "resource"),
        Some(&json!("macro-messages"))
    );
    assert_eq!(
        app.reflector()
            .unwrap()
            .message_pattern_metadata_value("macro.cat.find", "action"),
        Some(&json!("find"))
    );

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

    let card = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/42/card",
        ))
        .await
        .unwrap();
    assert_eq!(card.content_type(), Some("text/html; charset=utf-8"));
    assert_eq!(card.body_text().unwrap(), "<article>42:Milo</article>");

    let details = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Get,
                "/macro-cats/42/details?include_toys=true&page=3&tag=quiet",
            )
            .with_header("x-request-id", "req-1")
            .with_header("x-tenant", "acme"),
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
            tenant: "acme".to_string(),
        }
    );

    let piped = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/pipe/cat_42?page=7")
                .with_header("x-cat-kind", "tabby"),
        )
        .await
        .unwrap();
    assert_eq!(
        piped.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "cat_42".to_string(),
            name: "TABBY:7".to_string(),
        }
    );

    let piped_without_page = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/pipe/cat_99")
                .with_header("x-cat-kind", "calico"),
        )
        .await
        .unwrap();
    assert_eq!(
        piped_without_page.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "cat_99".to_string(),
            name: "CALICO:none".to_string(),
        }
    );

    let invalid_piped_id = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/pipe/42?page=1")
                .with_header("x-cat-kind", "tabby"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_piped_id, BootError::BadRequest(message) if message == "cat id must start with cat_")
    );

    let invalid_piped_page = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/pipe/cat_42?page=0")
                .with_header("x-cat-kind", "tabby"),
        )
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_piped_page, BootError::BadRequest(message) if message == "page must be greater than zero")
    );

    let builtin_pipes = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/builtin-pipes/42?active=true",
        ))
        .await
        .unwrap();
    assert_eq!(
        builtin_pipes.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "true:none:1:3".to_string(),
        }
    );

    let builtin_pipes_with_values = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Get,
                "/macro-cats/builtin-pipes/99?active=0&ratio=1.5&page=7",
            )
            .with_header("x-retry", "2"),
        )
        .await
        .unwrap();
    assert_eq!(
        builtin_pipes_with_values
            .body_json::<MacroCatDto>()
            .unwrap(),
        MacroCatDto {
            id: "99".to_string(),
            name: "false:1.5:7:2".to_string(),
        }
    );

    let invalid_builtin_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/builtin-pipes/not-int?active=true",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_builtin_pipe, BootError::BadRequest(message) if message.contains("numeric string is expected"))
    );

    let uuid_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/uuid/550e8400-e29b-41d4-a716-446655440000?request=550e8400-e29b-41d4-a716-446655440000",
        ))
        .await
        .unwrap();
    assert_eq!(
        uuid_pipe.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "550e8400-e29b-41d4-a716-446655440000".to_string(),
            name: "550e8400-e29b-41d4-a716-446655440000".to_string(),
        }
    );

    let invalid_uuid_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/uuid/not-a-uuid?request=550e8400-e29b-41d4-a716-446655440000",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_uuid_pipe, BootError::BadRequest(message) if message.contains("UUID string is expected"))
    );

    let invalid_uuid_version_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/uuid/550e8400-e29b-41d4-a716-446655440000?request=6ba7b810-9dad-11d1-80b4-00c04fd430c8",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_uuid_version_pipe, BootError::BadRequest(message) if message.contains("UUID v4 string is expected"))
    );

    let array_enum_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/array-enum?ids=1,2,3&kind=tabby",
        ))
        .await
        .unwrap();
    assert_eq!(
        array_enum_pipe.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "1|2|3".to_string(),
            name: "tabby".to_string(),
        }
    );

    let invalid_array_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/array-enum?ids=1,cat&kind=tabby",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_array_pipe, BootError::BadRequest(message) if message.contains("array item is invalid"))
    );

    let invalid_enum_pipe = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/array-enum?ids=1,2&kind=calico",
        ))
        .await
        .unwrap_err();
    assert!(
        matches!(invalid_enum_pipe, BootError::BadRequest(message) if message.contains("enum value is expected"))
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

    let all_get = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/catch",
        ))
        .await
        .unwrap();
    let all_post = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Post,
            "/macro-cats/catch",
        ))
        .await
        .unwrap();
    let raw_all = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Patch,
            "/macro-cats/raw-catch",
        ))
        .await
        .unwrap();

    assert_eq!(
        all_get.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "GET".to_string(),
            name: "Catch".to_string(),
        }
    );
    assert_eq!(
        all_post.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "POST".to_string(),
            name: "Catch".to_string(),
        }
    );
    assert_eq!(raw_all.body_text().unwrap(), "raw:PATCH");

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

    let touched = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-cats/42/touch")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    assert_eq!(touched.status(), 202);
    assert_eq!(
        touched.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Touched".to_string(),
        }
    );

    let cached = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/cache")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    assert_eq!(cached.header("cache-control"), Some("max-age=60"));
    assert_eq!(
        cached.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "cache".to_string(),
            name: "Cached".to_string(),
        }
    );

    let legacy = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-cats/legacy")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap();
    assert_eq!(legacy.status(), 301);
    assert_eq!(legacy.location(), Some("/macro-cats/42"));
    assert!(legacy.body().is_empty());

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
        details_operation["servers"],
        json!([{ "url": "https://edge.example.com", "description": "Edge" }])
    );
    assert_eq!(
        details_operation["externalDocs"],
        json!({
            "description": "Macro cat details guide",
            "url": "https://docs.example.com/macro-cats/details"
        })
    );
    assert_eq!(
        details_operation["x-controller-default"],
        json!({ "source": "controller" })
    );
    assert_eq!(
        details_operation["x-codeSamples"][0],
        json!({ "lang": "bash", "source": "curl /macro-cats/42/details" })
    );
    assert_eq!(
        details_operation["responses"]["200"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCatDetailsDto" })
    );
    assert_eq!(
        details_operation["responses"]["200"]["headers"]["x-rate-limit-remaining"],
        json!({
            "schema": { "type": "integer" },
            "description": "Remaining requests"
        })
    );
    assert_eq!(details_operation["security"][0]["bearerAuth"], json!([]));
    assert!(has_openapi_parameter(
        details_operation,
        "id",
        "path",
        true,
        json!({ "type": "string" }),
    ));
    assert_eq!(
        find_openapi_parameter(details_operation, "id", "path")
            .and_then(|parameter| parameter["description"].as_str()),
        Some("Cat identifier")
    );
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
    assert_eq!(
        find_openapi_parameter(details_operation, "include_toys", "query")
            .and_then(|parameter| parameter["description"].as_str()),
        Some("Include toy data")
    );
    let include_toys = find_openapi_parameter(details_operation, "include_toys", "query").unwrap();
    assert_eq!(include_toys["deprecated"], true);
    assert_eq!(include_toys["allowReserved"], true);
    assert_eq!(include_toys["style"], "form");
    assert_eq!(include_toys["explode"], false);
    assert_eq!(include_toys["examples"]["with_toys"]["value"], true);
    assert_eq!(
        find_openapi_parameter(details_operation, "x-request-id", "header")
            .and_then(|parameter| parameter["description"].as_str()),
        Some("Request correlation id")
    );

    let piped_operation = &document["paths"]["/macro-cats/pipe/{id}"]["get"];
    assert!(has_openapi_parameter(
        piped_operation,
        "id",
        "path",
        true,
        json!({ "type": "string" }),
    ));
    assert!(has_openapi_parameter(
        piped_operation,
        "page",
        "query",
        false,
        json!({ "type": "string" }),
    ));
    assert!(has_openapi_parameter(
        piped_operation,
        "x-cat-kind",
        "header",
        true,
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
        create_operation["requestBody"]["content"]["application/json"]["examples"]["milo"]["value"],
        json!({ "name": "Milo" })
    );
    assert_eq!(
        create_operation["responses"]["201"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCatDto" })
    );
    assert_eq!(
        create_operation["responses"]["201"]["content"]["application/json"]["examples"]["created"]
            ["value"],
        json!({ "id": "generated", "name": "Milo" })
    );

    let adopt_operation = &document["paths"]["/macro-cats/{id}/adoptions"]["post"];
    assert_eq!(
        adopt_operation["requestBody"]["content"]["application/json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCreateCatDto" })
    );

    let import_operation = &document["paths"]["/macro-cats/imports"]["post"];
    assert_eq!(
        import_operation["requestBody"]["description"],
        json!("Cat import form")
    );
    assert_eq!(
        import_operation["requestBody"]["content"]["multipart/form-data"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCreateCatDto" })
    );
    assert!(import_operation["requestBody"]["content"]["application/json"].is_null());
    assert_eq!(
        import_operation["responses"]["202"]["content"]["application/vnd.a3s.cat+json"]["schema"],
        json!({ "$ref": "#/components/schemas/MacroCatDto" })
    );
    assert_eq!(
        import_operation["responses"]["202"]["content"]["application/vnd.a3s.cat+json"]["example"],
        json!({ "id": "imported", "name": "Milo" })
    );
    assert_eq!(
        document["components"]["schemas"]["MacroCatPageDto"]["allOf"],
        json!([
            {
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": { "$ref": "#/components/schemas/MacroCatDto" }
                    }
                },
                "required": ["items"]
            },
            {
                "type": "object",
                "properties": {
                    "next_cursor": { "type": "string", "nullable": true },
                    "status": { "type": "string", "enum": ["fresh", "stale"] }
                }
            }
        ])
    );
    assert!(!document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/macro-cats/events"));

    app.bootstrap().await.unwrap();
    let ws_connection = app.gateways()[0]
        .connect_async(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-cats/ws",
        ))
        .await
        .unwrap();
    assert_eq!(ws_connection.namespace(), Some("/macro-cats"));
    let ws_reply = ws_connection
        .dispatch(WebSocketMessage::new("cat.find", json!({ "id": "42" })))
        .await
        .unwrap()
        .unwrap();
    let ws_typed_reply = ws_connection
        .dispatch(WebSocketMessage::new("cat.typed", json!({ "id": "43" })))
        .await
        .unwrap()
        .unwrap();
    let ws_field_reply = ws_connection
        .dispatch(WebSocketMessage::new("cat.field", json!({ "id": "44" })))
        .await
        .unwrap()
        .unwrap();
    let ws_server_reply = ws_connection
        .dispatch(WebSocketMessage::new("cat.server", json!({})))
        .await
        .unwrap()
        .unwrap();
    let ws_connection_reply = ws_connection
        .dispatch(WebSocketMessage::new(
            "cat.connection",
            json!({ "id": "45" }),
        ))
        .await
        .unwrap()
        .unwrap();
    let ws_create_reply = ws_connection
        .dispatch(WebSocketMessage::new(
            "cat.create",
            json!({ "name": "Luna" }),
        ))
        .await
        .unwrap()
        .unwrap();
    let ws_validation_error = ws_connection
        .dispatch(WebSocketMessage::new(
            "cat.validate",
            json!({ "name": " " }),
        ))
        .await
        .unwrap_err();
    let ws_error = ws_connection
        .dispatch(WebSocketMessage::new("cat.fail", json!({})))
        .await
        .unwrap()
        .unwrap();
    ws_connection.close().await.unwrap();
    assert_eq!(ws_reply.event(), "cat.found");
    assert_eq!(ws_reply.data()["id"], json!("42"));
    assert_eq!(ws_reply.data()["name"], json!("Milo"));
    assert_eq!(ws_typed_reply.event(), "cat.typed");
    assert_eq!(ws_typed_reply.data()["id"], json!("43"));
    assert_eq!(ws_typed_reply.data()["name"], json!("Milo"));
    assert_eq!(ws_field_reply.event(), "cat.field");
    assert_eq!(
        ws_field_reply.data(),
        &json!({
            "id": "44",
            "page": 1,
            "tag": "none",
        })
    );
    assert_eq!(ws_server_reply.event(), "cat.server");
    assert_eq!(
        ws_server_reply.data(),
        &json!({
            "path": "/macro-cats/ws",
            "namespace": "/macro-cats",
            "connections": 1,
        })
    );
    assert_eq!(ws_connection_reply.event(), "cat.connection");
    assert_eq!(
        ws_connection_reply.data()["connectionId"],
        json!(ws_connection.id())
    );
    assert_eq!(ws_connection_reply.data()["path"], json!("/macro-cats/ws"));
    assert_eq!(ws_connection_reply.data()["id"], json!("45"));
    assert_eq!(ws_create_reply.event(), "cat.created");
    assert_eq!(ws_create_reply.data()["name"], json!("Luna"));
    assert_eq!(ws_create_reply.data()["kind"], json!("cat"));
    assert!(
        matches!(&ws_validation_error, BootError::BadRequest(message) if message.contains("name is required")),
        "{ws_validation_error:?}"
    );
    assert_eq!(
        ws_error,
        WebSocketMessage::new(
            "macro.error",
            json!({
                "event": "cat.fail",
                "message": "bad request: macro websocket filter",
            }),
        )
    );
    assert_eq!(
        app.get::<MacroGatewayLifecycleLog>().unwrap().entries(),
        [
            "init:/macro-cats/ws:cat.connection,cat.create,cat.fail,cat.field,cat.find,cat.server,cat.typed,cat.validate:0",
            "connect:/macro-cats/ws",
            "disconnect:/macro-cats/ws"
        ]
    );

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

    let message_field_reply = app
        .dispatch_message(TransportMessage::new(
            "macro.cat.field",
            json!({ "id": "field" }),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message_field_reply.data_as::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "field".to_string(),
            name: "1:none".to_string(),
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

    let message_error = app
        .dispatch_message(TransportMessage::new("macro.cat.fail", json!({})))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        message_error.data(),
        &json!({
            "pattern": "macro.cat.fail",
            "message": "bad request: macro message filter",
        })
    );

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
    find_openapi_parameter(operation, name, location)
        .is_some_and(|parameter| parameter["required"] == required && parameter["schema"] == schema)
}

fn find_openapi_parameter<'a>(
    operation: &'a serde_json::Value,
    name: &str,
    location: &str,
) -> Option<&'a serde_json::Value> {
    operation["parameters"]
        .as_array()
        .unwrap()
        .iter()
        .find(|parameter| parameter["name"] == name && parameter["in"] == location)
}

#[tokio::test]
async fn macro_pipeline_decorators_register_controller_and_route_hooks() {
    let app = BootApplication::builder()
        .import(MacroPipelineModule)
        .build()
        .unwrap();
    assert_eq!(
        app.module_ref()
            .provider_scope::<MacroPipelineController>()
            .unwrap(),
        ProviderScope::Request,
    );

    let guarded = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/guarded",
        ))
        .await
        .unwrap();
    assert_eq!(guarded.body_json::<String>().unwrap(), "guarded");
    assert_eq!(guarded.header("x-macro-controller"), Some("yes"));

    let piped = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/piped",
        ))
        .await
        .unwrap();
    assert_eq!(piped.body_text().unwrap(), "yes");
    assert_eq!(piped.header("x-macro-controller"), Some("yes"));

    let filtered = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/filtered",
        ))
        .await
        .unwrap();
    assert_eq!(filtered.status(), 499);
    assert_eq!(
        filtered.body_text().unwrap(),
        "/macro-pipeline/filtered: bad request: macro filter"
    );
    assert_eq!(
        MacroBadRequestFilter::caught_kinds(),
        [BootErrorKind::BadRequest]
    );

    let filtered_conflict = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/filtered-conflict",
        ))
        .await
        .unwrap();
    assert_eq!(filtered_conflict.status(), 409);
    assert_eq!(
        filtered_conflict.body_text().unwrap(),
        "/macro-pipeline/filtered-conflict: resource conflict: macro conflict"
    );
    assert_eq!(
        MacroConflictFilter::caught_kinds(),
        [BootErrorKind::Conflict]
    );

    let unfiltered = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/unfiltered",
        ))
        .await
        .unwrap_err();
    assert!(matches!(unfiltered, BootError::Unauthorized(message) if message == "macro private"));
}

#[tokio::test]
async fn controller_level_hide_from_openapi_macro_hides_routes() {
    let app = BootApplication::builder()
        .import(MacroHiddenOpenApiModule)
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-hidden-openapi",
        ))
        .await
        .unwrap();
    assert_eq!(
        response.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "hidden".to_string(),
            name: "Hidden".to_string(),
        }
    );

    let document = app.openapi(OpenApiInfo::new("Hidden", "1.0.0"));
    let document = serde_json::to_value(document).unwrap();
    assert!(!document["paths"]
        .as_object()
        .unwrap()
        .contains_key("/macro-hidden-openapi"));
}

#[test]
fn module_macro_registers_forward_imports() {
    let app = BootApplication::builder()
        .import(MacroForwardRootModule)
        .build()
        .unwrap();
    let service = app.get::<MacroForwardFeatureService>().unwrap();

    assert!(service.root.get().is_ok());
    assert_eq!(
        app.discovery()
            .unwrap()
            .graph()
            .module("macro-forward-root")
            .unwrap()
            .imports
            .as_slice(),
        ["macro-forward-feature"]
    );
}

#[tokio::test]
async fn module_macro_registers_route_prefixes() {
    let app = BootApplication::builder()
        .import(MacroPrefixedModule)
        .build()
        .unwrap();

    assert_eq!(MacroPrefixedModule.route_prefix(), Some("/api"));
    assert_eq!(app.routes()[0].path(), "/api/dogs/{id}");

    let response = app
        .call(BootRequest::new(a3s_boot::HttpMethod::Get, "/api/dogs/42"))
        .await
        .unwrap();
    assert_eq!(
        response.body_json::<MacroCatDto>().unwrap(),
        MacroCatDto {
            id: "42".to_string(),
            name: "Rex".to_string(),
        }
    );
}

#[tokio::test]
async fn macro_host_and_ip_extractors_register_host_scoped_routes() {
    let app = BootApplication::builder()
        .import(MacroHostModule)
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-host/who")
                .with_header("host", "acme.example.com:3000")
                .with_header("x-forwarded-for", "203.0.113.7, 203.0.113.8"),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_json::<MacroHostDto>().unwrap(),
        MacroHostDto {
            tenant: "acme".to_string(),
            ip: Some("203.0.113.7".to_string()),
        }
    );

    let api = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-host/api")
                .with_header("host", "api.example.com")
                .with_header("x-real-ip", "192.0.2.24"),
        )
        .await
        .unwrap();

    assert_eq!(api.body_json::<String>().unwrap(), "192.0.2.24");

    let pipe_ip = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-host/pipe-ip")
                .with_header("host", "api.example.com")
                .with_header("x-real-ip", "192.0.2.25"),
        )
        .await
        .unwrap();

    assert_eq!(pipe_ip.body_json::<String>().unwrap(), "ip:192.0.2.25");

    let missing = app
        .handle(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-host/who")
                .with_header("host", "www.other.com"),
        )
        .await;

    assert_eq!(missing.status(), 404);
}

#[tokio::test]
async fn macro_version_decorators_register_controller_and_route_versions() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version"))
        .import(MacroVersionModule)
        .build()
        .unwrap();

    let v1 = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-version/cats")
                .with_header("x-api-version", "1"),
        )
        .await
        .unwrap();
    let v2 = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-version/cats")
                .with_header("x-api-version", "2"),
        )
        .await
        .unwrap();
    let neutral = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-version/health",
        ))
        .await
        .unwrap();
    let multi = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Get, "/macro-version/multi")
                .with_header("x-api-version", "3"),
        )
        .await
        .unwrap();

    assert_eq!(v1.body_json::<String>().unwrap(), "v1");
    assert_eq!(v2.body_json::<String>().unwrap(), "v2");
    assert_eq!(neutral.body_json::<String>().unwrap(), "ok");
    assert_eq!(multi.body_json::<String>().unwrap(), "multi");
}

#[tokio::test]
async fn macro_serialize_decorators_register_controller_and_route_options() {
    let app = BootApplication::builder()
        .use_global_serialization()
        .import(MacroSerializationModule)
        .build()
        .unwrap();

    let user = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-serialization/user",
        ))
        .await
        .unwrap();
    let public = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-serialization/public",
        ))
        .await
        .unwrap();

    assert_eq!(
        user.body_json::<serde_json::Value>().unwrap(),
        json!({
            "id": "u1",
            "email": "milo@example.com"
        })
    );
    assert_eq!(
        public.body_json::<serde_json::Value>().unwrap(),
        json!({
            "id": "u1",
            "email": "milo@example.com"
        })
    );
}

#[tokio::test]
async fn validate_macro_enables_body_and_query_dto_validation() {
    let app = BootApplication::builder()
        .import(MacroValidationModule)
        .build()
        .unwrap();
    assert_eq!(
        app.module_ref()
            .provider_scope::<MacroValidationController>()
            .unwrap(),
        ProviderScope::Request,
    );

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
    let whitelisted = app
        .call(
            BootRequest::new(a3s_boot::HttpMethod::Post, "/macro-whitelist-validation")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Milo","displayName":"Mr. Milo","role":"admin"}"#),
        )
        .await
        .unwrap();
    let forbidden = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Post,
                "/macro-whitelist-validation/strict",
            )
            .with_content_type("application/json")
            .with_body(r#"{"name":"Milo","role":"admin"}"#),
        )
        .await
        .unwrap_err();
    let transformed = app
        .call(
            BootRequest::new(
                a3s_boot::HttpMethod::Post,
                "/macro-whitelist-validation/transform",
            )
            .with_content_type("application/json")
            .with_body(r#"{"name":"Milo"}"#),
        )
        .await
        .unwrap();

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
    assert_eq!(
        whitelisted.body_json::<serde_json::Value>().unwrap(),
        json!({
            "displayName": "Mr. Milo",
            "name": "Milo"
        })
    );
    assert!(
        matches!(forbidden, BootError::BadRequest(message) if message == "non-whitelisted body properties: role")
    );
    assert_eq!(
        transformed.body_json::<serde_json::Value>().unwrap(),
        json!({
            "kind": "cat",
            "name": "Milo"
        })
    );
}
