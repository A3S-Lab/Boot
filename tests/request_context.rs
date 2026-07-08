#![cfg(feature = "request-context")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, HttpMethod, MiddlewareOutcome, Module,
    ProviderDefinition, RequestContext, Result, RouteDefinition,
};
use serde_json::json;

#[tokio::test]
async fn request_context_is_available_through_the_route_pipeline() {
    #[derive(Debug)]
    struct CatsService;

    impl CatsService {
        fn describe_current_request(&self) -> Result<String> {
            let context = RequestContext::current()?;
            Ok(format!(
                "{} {} {} {} {} {} {} {} {}",
                context.method().as_str(),
                context.request_path(),
                context.route_path(),
                context.module_name().unwrap_or("missing-module"),
                context.request_id().unwrap_or("missing-request-id"),
                context.header("x-mode").unwrap_or("missing-mode"),
                context.param("id").unwrap_or("missing-id"),
                context.query_param("page").unwrap_or("missing-page"),
                context.value_as::<String>("tenant")?.unwrap_or_default(),
            ))
        }
    }

    #[derive(Debug)]
    struct CatsModule;

    impl Module for CatsModule {
        fn name(&self) -> &'static str {
            "cats"
        }

        fn providers(&self) -> Result<Vec<ProviderDefinition>> {
            Ok(vec![ProviderDefinition::singleton(CatsService)])
        }

        fn routes(&self) -> Result<Vec<RouteDefinition>> {
            Ok(vec![RouteDefinition::get(
                "/cats/{id}",
                move |request: BootRequest| async move {
                    let service = request.get::<CatsService>()?;
                    Ok(BootResponse::text(service.describe_current_request()?))
                },
            )?
            .with_metadata_value("resource", json!("cats"))
            .with_middleware(|request: BootRequest| async move {
                let context = RequestContext::current()?;
                assert_eq!(
                    context.metadata_as::<String>("resource")?,
                    Some("cats".into())
                );
                context.set_value("tenant", "north")?;
                Ok(MiddlewareOutcome::next(request))
            })])
        }
    }

    let app = BootApplication::builder()
        .import(CatsModule)
        .build()
        .unwrap();
    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/cats/1").with_header("x-request-id", "request-1"))
        .await
        .unwrap();

    assert_eq!(
        response.body_text().unwrap(),
        "GET /cats/1 /cats/{id} cats request-1 missing-mode 1 missing-page north"
    );

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/cats/2?page=3")
                .with_header("x-request-id", "request-2")
                .with_header("X-Mode", "debug"),
        )
        .await
        .unwrap();

    assert_eq!(
        response.body_text().unwrap(),
        "GET /cats/2 /cats/{id} cats request-2 debug 2 3 north"
    );
}

#[tokio::test]
async fn request_context_is_not_available_outside_a_route_scope() {
    assert!(RequestContext::try_current().is_none());
    assert!(RequestContext::current()
        .unwrap_err()
        .to_string()
        .contains("request context is not available"));
}

#[cfg(feature = "auth")]
#[tokio::test]
async fn request_context_tracks_auth_principal_when_auth_guard_runs() {
    use a3s_boot::{AuthModule, AuthPrincipal, ExecutionContext};

    let app = BootApplication::builder()
        .import(
            AuthModule::new("auth")
                .bearer(|_token: String, _context: ExecutionContext| async {
                    Ok(Some(AuthPrincipal::new("cat-1").with_role("admin")))
                })
                .global(),
        )
        .use_global_auth()
        .route(
            RouteDefinition::get("/me", |_| async {
                let context = RequestContext::current()?;
                let principal = context.auth_principal()?.ok_or_else(|| {
                    a3s_boot::BootError::Internal("missing principal".to_string())
                })?;
                Ok(BootResponse::text(principal.subject().to_string()))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/me").with_header("authorization", "Bearer ok"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "cat-1");
}
