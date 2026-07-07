use a3s_boot::{
    BootApplication, BootResponse, ControllerDefinition, DiscoveryService, HttpMethod,
    MessagePatternDefinition, MessagePatternKind, Module, ModuleRef, ProviderDefinition,
    ProviderToken, Result, RouteDefinition, TransportReply, WebSocketGatewayDefinition,
    WebSocketMessage,
};

#[derive(Debug)]
struct CatsService;

#[derive(Debug)]
struct DiscoveryModuleFixture;

impl Module for DiscoveryModuleFixture {
    fn name(&self) -> &'static str {
        "discovery"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(CatsService)])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/cats")?
            .route(
                RouteDefinition::get("/{id}", |_| async { Ok(BootResponse::text("cat")) })?
                    .with_tag("cats")
                    .with_operation_id("getCat")
                    .with_version("1"),
            )?
            .with_tag("animals")])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("ok"))
        })?
        .with_tag("ops")])
    }

    fn gateways(&self, _module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        Ok(vec![WebSocketGatewayDefinition::new("/ws")?
            .subscribe("cat.created", |_| async {
                Ok(WebSocketMessage::text("ack", "ok"))
            })?
            .subscribe("cat.deleted", |_| async { Ok(()) })?])
    }

    fn message_patterns(&self, _module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        Ok(vec![
            MessagePatternDefinition::request("cat.find", |_| async {
                Ok(TransportReply::text("found"))
            })?,
            MessagePatternDefinition::event("cat.created", |_| async { Ok(()) })?,
        ])
    }
}

#[test]
fn discovery_service_snapshots_modules_routes_gateways_and_message_patterns() {
    let app = BootApplication::builder()
        .global_prefix("/api")
        .import(DiscoveryModuleFixture)
        .build()
        .unwrap();

    let discovery = DiscoveryService::from_app(&app).unwrap();
    let module = discovery.module("discovery").unwrap();

    assert_eq!(module.name, "discovery");
    assert!(module
        .provider_tokens
        .contains(&ProviderToken::of::<CatsService>()));

    let module_routes = discovery.routes_for_module("discovery");
    assert_eq!(module_routes.len(), 2);
    assert!(module_routes
        .iter()
        .any(|route| route.path == "/api/cats/{id}"
            && route.path_shape == "/api/cats/{}"
            && route.path_params == ["id"]
            && route.controller_prefix.as_deref() == Some("/cats")
            && route.openapi.tags == ["cats", "animals"]
            && route.versioning.to_string() == "1"));
    assert!(module_routes
        .iter()
        .any(|route| route.path == "/api/health" && route.openapi.tags == ["ops"]));

    let gateways = discovery.gateways_for_module("discovery");
    assert_eq!(gateways.len(), 1);
    assert_eq!(gateways[0].path, "/api/ws");
    assert_eq!(gateways[0].events, ["cat.created", "cat.deleted"]);

    let patterns = discovery.message_patterns_for_module("discovery");
    assert_eq!(patterns.len(), 2);
    assert!(patterns.iter().any(|pattern| pattern.pattern == "cat.find"
        && pattern.kind == MessagePatternKind::RequestResponse));
    assert!(patterns.iter().any(
        |pattern| pattern.pattern == "cat.created" && pattern.kind == MessagePatternKind::Event
    ));
}

#[test]
fn reflector_queries_route_metadata_from_discovery_snapshot() {
    let app = BootApplication::builder()
        .import(DiscoveryModuleFixture)
        .build()
        .unwrap();
    let discovery = app.discovery().unwrap();
    let reflector = discovery.reflector();

    assert_eq!(
        reflector.operation_id(HttpMethod::Get, "/cats/{id}"),
        Some("getCat")
    );
    assert_eq!(
        reflector
            .openapi(HttpMethod::Get, "/cats/{id}")
            .unwrap()
            .tags,
        ["cats", "animals"]
    );
    assert_eq!(
        reflector
            .routes_with_tag("animals")
            .into_iter()
            .map(|route| route.path.as_str())
            .collect::<Vec<_>>(),
        ["/cats/{id}"]
    );
    assert_eq!(
        reflector.routes_for_controller("/cats")[0]
            .module_name
            .as_deref(),
        Some("discovery")
    );
    assert!(app
        .reflector()
        .unwrap()
        .routes_with_tag("ops")
        .iter()
        .any(|route| route.path == "/health"));
}
