#![cfg(feature = "auth")]

use a3s_boot::{
    AuthGuard, AuthModule, AuthPrincipal, AuthService, AUTH_PUBLIC_METADATA, AUTH_ROLES_METADATA,
    AUTH_SCOPES_METADATA, AUTH_STRATEGY_METADATA,
};
use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ExecutionContext, HttpMethod, RouteDefinition,
};
use serde_json::json;

#[tokio::test]
async fn auth_guard_authenticates_bearer_tokens_and_exposes_principal() {
    let app = BootApplication::builder()
        .import(
            AuthModule::new("auth")
                .bearer(|token: String, _context: ExecutionContext| async move {
                    match token.as_str() {
                        "admin-token" => Ok(Some(
                            AuthPrincipal::new("cat-1")
                                .with_role("admin")
                                .with_scope("cats:read")
                                .with_claim("name", "Milo")?,
                        )),
                        "reader-token" => Ok(Some(
                            AuthPrincipal::new("cat-2")
                                .with_role("reader")
                                .with_scope("cats:read"),
                        )),
                        _ => Ok(None),
                    }
                })
                .global(),
        )
        .use_global_auth()
        .route(
            RouteDefinition::get("/me", |request: BootRequest| async move {
                let principal = request.require_auth_principal()?;
                BootResponse::json(&json!({
                    "subject": principal.subject(),
                    "strategy": principal.strategy(),
                    "name": principal.claim("name"),
                }))
            })
            .unwrap()
            .with_metadata_value(AUTH_ROLES_METADATA, json!(["admin"]))
            .with_metadata_value(AUTH_SCOPES_METADATA, json!(["cats:read"])),
        )
        .build()
        .unwrap();

    let allowed = app
        .call(
            BootRequest::new(HttpMethod::Get, "/me")
                .with_header("authorization", "Bearer admin-token"),
        )
        .await
        .unwrap();
    let missing = app.handle(BootRequest::new(HttpMethod::Get, "/me")).await;
    let wrong_role = app
        .handle(
            BootRequest::new(HttpMethod::Get, "/me")
                .with_header("authorization", "Bearer reader-token"),
        )
        .await;

    assert_eq!(allowed.status(), 200);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(allowed.body()).unwrap(),
        json!({
            "subject": "cat-1",
            "strategy": "bearer",
            "name": "Milo",
        })
    );
    assert_eq!(missing.status(), 401);
    assert_eq!(
        missing.body_text().unwrap(),
        "missing authentication credentials"
    );
    assert_eq!(wrong_role.status(), 403);
    assert_eq!(
        wrong_role.body_text().unwrap(),
        "missing required role: admin"
    );
}

#[tokio::test]
async fn auth_guard_allows_public_routes_without_credentials() {
    let app = BootApplication::builder()
        .import(
            AuthModule::new("auth")
                .bearer(|_token: String, _context: ExecutionContext| async {
                    Ok(Some(AuthPrincipal::new("cat-1")))
                })
                .global(),
        )
        .use_global_auth()
        .route(
            RouteDefinition::get("/public", |request: BootRequest| async move {
                assert!(request.auth_principal()?.is_none());
                Ok(BootResponse::text("public"))
            })
            .unwrap()
            .with_metadata_value(AUTH_PUBLIC_METADATA, json!(true)),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/public"))
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.body_text().unwrap(), "public");
}

#[tokio::test]
async fn auth_guard_can_select_a_strategy_from_route_metadata() {
    let app = BootApplication::builder()
        .import(
            AuthModule::new("auth")
                .strategy("api-key", |context: ExecutionContext| async move {
                    Ok((context.request.header("x-api-key") == Some("secret-key"))
                        .then(|| AuthPrincipal::new("key-user").with_strategy("api-key")))
                })
                .global(),
        )
        .use_global_auth()
        .route(
            RouteDefinition::get("/keys", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.require_auth_principal()?.subject().to_string(),
                ))
            })
            .unwrap()
            .with_metadata_value(AUTH_STRATEGY_METADATA, json!("api-key")),
        )
        .build()
        .unwrap();

    let allowed = app
        .call(BootRequest::new(HttpMethod::Get, "/keys").with_header("x-api-key", "secret-key"))
        .await
        .unwrap();
    let rejected = app.handle(BootRequest::new(HttpMethod::Get, "/keys")).await;

    assert_eq!(allowed.status(), 200);
    assert_eq!(allowed.body_text().unwrap(), "key-user");
    assert_eq!(rejected.status(), 401);
}

#[tokio::test]
async fn named_auth_guards_can_require_scopes() {
    let app = BootApplication::builder()
        .import(
            AuthModule::new("auth")
                .bearer_named(
                    "tokens",
                    |_token: String, _context: ExecutionContext| async {
                        Ok(Some(AuthPrincipal::new("cat-1").with_scope("cats:write")))
                    },
                )
                .global(),
        )
        .route(
            RouteDefinition::post("/cats", |_| async { Ok(BootResponse::text("created")) })
                .unwrap()
                .with_guard(
                    AuthGuard::new()
                        .strategy("tokens")
                        .require_scope("cats:write"),
                ),
        )
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Post, "/cats")
                .with_header("authorization", "Bearer any-token"),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert_eq!(response.body_text().unwrap(), "created");
}

#[test]
fn auth_service_reports_duplicate_strategy_registration() {
    let auth = AuthService::default();
    auth.register_bearer_strategy(
        "bearer",
        |_token: String, _context: ExecutionContext| async {
            Ok(Some(AuthPrincipal::new("cat-1")))
        },
    )
    .unwrap();

    let error = auth
        .register_bearer_strategy(
            "bearer",
            |_token: String, _context: ExecutionContext| async {
                Ok(Some(AuthPrincipal::new("cat-2")))
            },
        )
        .unwrap_err();

    assert!(error
        .to_string()
        .contains("auth strategy is already registered: bearer"));
}
