use a3s_boot::{
    ApiVersioning, BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition,
    HttpMethod, RouteDefinition,
};

#[tokio::test]
async fn uri_versioning_dispatches_to_versioned_routes_and_decodes_params() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::uri())
        .route(
            RouteDefinition::get("/cats/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "{}:{}",
                    request.path(),
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap()
            .with_version("1"),
        )
        .build()
        .unwrap();

    let route_match = app
        .route_match(HttpMethod::Get, "/v1/cats/milo%2Egray")
        .unwrap()
        .unwrap();
    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/v1/cats/milo%2Egray"))
        .await
        .unwrap();

    assert_eq!(route_match.route().path(), "/cats/{id}");
    assert_eq!(route_match.param("id"), Some("milo.gray"));
    assert_eq!(
        app.allowed_methods("/v1/cats/milo.gray"),
        vec![HttpMethod::Get]
    );
    assert_eq!(response.body_text().unwrap(), "/cats/milo%2Egray:milo.gray");
}

#[tokio::test]
async fn header_versioning_selects_routes_with_the_same_method_and_path() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version"))
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v2")) })
                .unwrap()
                .with_version("2"),
        )
        .build()
        .unwrap();

    let v1 = app
        .call(BootRequest::new(HttpMethod::Get, "/cats").with_header("x-api-version", "1"))
        .await
        .unwrap();
    let v2 = app
        .call(BootRequest::new(HttpMethod::Get, "/cats").with_header("x-api-version", "2"))
        .await
        .unwrap();
    let missing = app
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap_err();

    assert_eq!(v1.body_text().unwrap(), "v1");
    assert_eq!(v2.body_text().unwrap(), "v2");
    assert!(matches!(missing, BootError::NotFound(_)));
}

#[tokio::test]
async fn media_type_versioning_reads_accept_parameters() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::media_type())
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v2")) })
                .unwrap()
                .with_version("2"),
        )
        .build()
        .unwrap();

    let response = app
        .call(
            BootRequest::new(HttpMethod::Get, "/cats")
                .with_header("accept", "application/json; v=2"),
        )
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "v2");
}

#[tokio::test]
async fn default_versions_and_neutral_routes_match_expected_requests() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version").with_default_version("1"))
        .route(
            RouteDefinition::get("/legacy", |_| async { Ok(BootResponse::text("legacy")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) })
                .unwrap()
                .version_neutral(),
        )
        .build()
        .unwrap();

    let defaulted = app
        .call(BootRequest::new(HttpMethod::Get, "/cats"))
        .await
        .unwrap();
    let legacy_default = app
        .call(BootRequest::new(HttpMethod::Get, "/legacy").with_header("x-api-version", "1"))
        .await
        .unwrap();
    let neutral = app
        .call(BootRequest::new(HttpMethod::Get, "/health").with_header("x-api-version", "99"))
        .await
        .unwrap();
    let legacy_wrong_version = app
        .call(BootRequest::new(HttpMethod::Get, "/legacy").with_header("x-api-version", "2"))
        .await
        .unwrap_err();

    assert_eq!(defaulted.body_text().unwrap(), "v1");
    assert_eq!(legacy_default.body_text().unwrap(), "legacy");
    assert_eq!(neutral.body_text().unwrap(), "ok");
    assert!(matches!(legacy_wrong_version, BootError::NotFound(_)));
}

#[tokio::test]
async fn controller_versions_are_inherited_by_routes() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .with_version("2")
        .get("/{id}", |request: BootRequest| async move {
            Ok(BootResponse::text(
                request.param("id").unwrap_or("missing").to_string(),
            ))
        })
        .unwrap();

    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::uri())
        .route(controller.routes()[0].clone())
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/v2/cats/luna"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "luna");
}

#[test]
fn versioned_route_duplicates_are_checked_by_version_overlap() {
    let app = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version"))
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v2")) })
                .unwrap()
                .with_version("2"),
        )
        .build()
        .unwrap();

    let duplicate = BootApplication::builder()
        .enable_api_versioning(ApiVersioning::header("x-api-version"))
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("v1")) })
                .unwrap()
                .with_version("1"),
        )
        .route(
            RouteDefinition::get("/cats", |_| async { Ok(BootResponse::text("also v1")) })
                .unwrap()
                .with_versions(["1", "2"]),
        )
        .build();

    assert_eq!(app.routes().len(), 2);
    assert!(matches!(duplicate, Err(BootError::DuplicateRoute(_))));
}
