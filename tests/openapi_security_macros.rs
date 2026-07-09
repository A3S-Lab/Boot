#![cfg(feature = "macros")]

use std::sync::Arc;

use a3s_boot::{
    controller, BootApplication, BootResponse, ControllerDefinition, Module, ModuleRef,
    OpenApiInfo, OpenApiOAuthFlows, Result, RouteDefinition,
};
use serde_json::json;

#[test]
fn route_builder_bearer_auth_registers_requirement_and_scheme() {
    let route = RouteDefinition::get("/secure", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_bearer_auth();

    let document = BootApplication::builder()
        .route(route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Security API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();
    let operation = &value["paths"]["/secure"]["get"];

    assert_eq!(operation["security"][0]["bearerAuth"], json!([]));
    assert_eq!(
        value["components"]["securitySchemes"]["bearerAuth"],
        json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        })
    );
}

#[test]
fn route_builder_oauth2_and_openid_connect_register_schemes() {
    let oauth_route = RouteDefinition::get("/oauth", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_oauth2_auth(
            "oauth2",
            OpenApiOAuthFlows::authorization_code(
                "https://auth.example.com/oauth/authorize",
                "https://auth.example.com/oauth/token",
                [("cats:read", "Read cats")],
            ),
            ["cats:read"],
        );
    let open_id_route = RouteDefinition::get("/openid", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_open_id_connect_auth(
            "openId",
            "https://auth.example.com/.well-known/openid-configuration",
            ["openid", "profile"],
        );

    let document = BootApplication::builder()
        .route(oauth_route)
        .route(open_id_route)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Security API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();

    assert_eq!(
        value["paths"]["/oauth"]["get"]["security"][0]["oauth2"],
        json!(["cats:read"])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["oauth2"],
        json!({
            "type": "oauth2",
            "flows": {
                "authorizationCode": {
                    "authorizationUrl": "https://auth.example.com/oauth/authorize",
                    "tokenUrl": "https://auth.example.com/oauth/token",
                    "scopes": {
                        "cats:read": "Read cats"
                    }
                }
            }
        })
    );
    assert_eq!(
        value["paths"]["/openid"]["get"]["security"][0]["openId"],
        json!(["openid", "profile"])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["openId"],
        json!({
            "type": "openIdConnect",
            "openIdConnectUrl": "https://auth.example.com/.well-known/openid-configuration"
        })
    );
}

#[derive(Debug)]
struct OpenApiSecurityController;

#[controller("/secure")]
impl OpenApiSecurityController {
    #[a3s_boot::get("/custom")]
    #[a3s_boot::api_security("customAuth", scopes = ["read", "write"])]
    async fn custom(&self) -> Result<&'static str> {
        Ok("custom")
    }

    #[a3s_boot::get("/cookie")]
    #[a3s_boot::api_cookie_auth(name = "sid")]
    async fn cookie(&self) -> Result<&'static str> {
        Ok("cookie")
    }

    #[a3s_boot::get("/header-key")]
    #[a3s_boot::api_key_auth(name = "x-api-key")]
    async fn header_key(&self) -> Result<&'static str> {
        Ok("header")
    }

    #[a3s_boot::get("/query-key")]
    #[a3s_boot::api_key_auth(
        name = "api_key",
        location = "query",
        scheme = "queryKeyAuth",
        description = "API key query parameter"
    )]
    async fn query_key(&self) -> Result<&'static str> {
        Ok("query")
    }

    #[a3s_boot::get("/bearer")]
    #[a3s_boot::bearer_auth("accessToken")]
    async fn bearer(&self) -> Result<&'static str> {
        Ok("bearer")
    }

    #[a3s_boot::get("/oauth2")]
    #[a3s_boot::oauth2_auth(
        name = "oauth2",
        flow = "authorization_code",
        authorization_url = "https://auth.example.com/oauth/authorize",
        token_url = "https://auth.example.com/oauth/token",
        refresh_url = "https://auth.example.com/oauth/refresh",
        scopes = ["cats:read", "cats:write"],
        description = "OAuth2 authorization code"
    )]
    async fn oauth2(&self) -> Result<&'static str> {
        Ok("oauth2")
    }

    #[a3s_boot::get("/openid")]
    #[a3s_boot::open_id_connect_auth(
        name = "openId",
        url = "https://auth.example.com/.well-known/openid-configuration",
        scopes = ["openid", "profile"],
        description = "OpenID Connect discovery"
    )]
    async fn openid(&self) -> Result<&'static str> {
        Ok("openid")
    }
}

#[derive(Debug)]
struct OpenApiSecurityModule;

impl Module for OpenApiSecurityModule {
    fn name(&self) -> &'static str {
        "openapi-security"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::new(OpenApiSecurityController).controller()?])
    }
}

#[test]
fn openapi_security_macros_register_requirements_and_schemes() {
    let document = BootApplication::builder()
        .import(OpenApiSecurityModule)
        .build()
        .unwrap()
        .openapi(OpenApiInfo::new("Security API", "1.0.0"));
    let value = serde_json::to_value(document).unwrap();

    assert_eq!(
        value["paths"]["/secure/custom"]["get"]["security"][0]["customAuth"],
        json!(["read", "write"])
    );
    assert!(value["components"]["securitySchemes"]
        .as_object()
        .unwrap()
        .get("customAuth")
        .is_none());

    assert_eq!(
        value["paths"]["/secure/cookie"]["get"]["security"][0]["cookieAuth"],
        json!([])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["cookieAuth"],
        json!({
            "type": "apiKey",
            "in": "cookie",
            "name": "sid"
        })
    );

    assert_eq!(
        value["paths"]["/secure/header-key"]["get"]["security"][0]["apiKeyAuth"],
        json!([])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["apiKeyAuth"],
        json!({
            "type": "apiKey",
            "in": "header",
            "name": "x-api-key"
        })
    );

    assert_eq!(
        value["paths"]["/secure/query-key"]["get"]["security"][0]["queryKeyAuth"],
        json!([])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["queryKeyAuth"],
        json!({
            "type": "apiKey",
            "in": "query",
            "name": "api_key",
            "description": "API key query parameter"
        })
    );

    assert_eq!(
        value["paths"]["/secure/bearer"]["get"]["security"][0]["accessToken"],
        json!([])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["accessToken"],
        json!({
            "type": "http",
            "scheme": "bearer",
            "bearerFormat": "JWT"
        })
    );

    assert_eq!(
        value["paths"]["/secure/oauth2"]["get"]["security"][0]["oauth2"],
        json!(["cats:read", "cats:write"])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["oauth2"],
        json!({
            "type": "oauth2",
            "description": "OAuth2 authorization code",
            "flows": {
                "authorizationCode": {
                    "authorizationUrl": "https://auth.example.com/oauth/authorize",
                    "tokenUrl": "https://auth.example.com/oauth/token",
                    "refreshUrl": "https://auth.example.com/oauth/refresh",
                    "scopes": {
                        "cats:read": "",
                        "cats:write": ""
                    }
                }
            }
        })
    );

    assert_eq!(
        value["paths"]["/secure/openid"]["get"]["security"][0]["openId"],
        json!(["openid", "profile"])
    );
    assert_eq!(
        value["components"]["securitySchemes"]["openId"],
        json!({
            "type": "openIdConnect",
            "description": "OpenID Connect discovery",
            "openIdConnectUrl": "https://auth.example.com/.well-known/openid-configuration"
        })
    );
}
