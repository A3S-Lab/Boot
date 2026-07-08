#![cfg(feature = "macros")]

use std::collections::BTreeMap;
use std::str::FromStr;
use std::sync::Arc;

use a3s_boot::{
    controller, injectable, ApiVersioning, BootApplication, BootError, BootErrorKind, BootRequest,
    BootResponse, BoxFuture, ControllerDefinition, ExceptionFilter, ExecutionContext, Guard,
    Interceptor, Module, ModuleRef, OpenApiInfo, ParseArrayPipe, ParseBoolPipe, ParseEnumPipe,
    ParseFloatPipe, ParseIntPipe, ParseUuidPipe, Pipe, ProviderDefinition, ProviderRef,
    ProviderToken, Result, SseEvent, SseStream, StringTemplateViewEngine, TransportMessage,
    TransportReply, UuidVersion, Validate, ViewModule, WebSocketGatewayConnection,
    WebSocketGatewayInitContext, WebSocketMessage,
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
impl MacroCatsGateway {
    #[a3s_boot::on_gateway_init]
    async fn after_init(&self, context: WebSocketGatewayInitContext) -> Result<()> {
        self.lifecycle_log.push(format!(
            "init:{}:{}",
            context.gateway_path,
            context.events.join(",")
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
    async fn find(&self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        let id = message
            .data()
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let cat = self.cats.find_one(id);
        WebSocketMessage::json("cat.found", &cat)
    }

    #[a3s_boot::on_gateway_disconnect]
    async fn handle_disconnect(&self, connection: WebSocketGatewayConnection) -> Result<()> {
        self.lifecycle_log
            .push(format!("disconnect:{}", connection.request().path()));
        Ok(())
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

    #[get("/unfiltered")]
    #[use_filter(MacroBadRequestFilter::catch_filter())]
    async fn unfiltered(&self) -> Result<String> {
        Err(BootError::Unauthorized("macro private".to_string()))
    }
}

#[a3s_boot::module(
    name = "macro-pipeline",
    providers = [MacroPipelineController],
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

#[tokio::test]
async fn macros_register_injectable_services_and_controller_routes() {
    let app = BootApplication::builder()
        .import(MacroCatsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 19);
    assert_eq!(app.gateways().len(), 1);
    assert_eq!(app.gateways()[0].namespace(), Some("/macro-cats"));
    assert_eq!(app.message_patterns().len(), 3);
    let exports = MacroCatsModule.exports().unwrap();
    assert!(exports.contains(&ProviderToken::of::<MacroCatsService>()));
    assert!(exports.contains(&ProviderToken::named("readonly-cats")));
    let reader = app.get::<MacroAutoCatsReader>().unwrap();
    let controller = app.get::<MacroCatsController>().unwrap();
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
    ws_connection.close().await.unwrap();
    assert_eq!(ws_reply.event(), "cat.found");
    assert_eq!(ws_reply.data()["id"], json!("42"));
    assert_eq!(ws_reply.data()["name"], json!("Milo"));
    assert_eq!(
        app.get::<MacroGatewayLifecycleLog>().unwrap().entries(),
        [
            "init:/macro-cats/ws:cat.find",
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
async fn macro_pipeline_decorators_register_controller_and_route_hooks() {
    let app = BootApplication::builder()
        .import(MacroPipelineModule)
        .build()
        .unwrap();

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

    let unfiltered = app
        .call(BootRequest::new(
            a3s_boot::HttpMethod::Get,
            "/macro-pipeline/unfiltered",
        ))
        .await
        .unwrap_err();
    assert!(matches!(unfiltered, BootError::Unauthorized(message) if message == "macro private"));
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
