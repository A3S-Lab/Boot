use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition, HttpMethod,
    MiddlewareRoute, RouteDefinition,
};
use serde::Deserialize;

#[test]
fn rejects_relative_route_paths() {
    let result = RouteDefinition::get("health", |_| async { Ok(BootResponse::text("ok")) });

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

#[test]
fn rejects_route_paths_with_query_or_fragment_markers() {
    for path in ["/items?active=true", "/items#active"] {
        let result = RouteDefinition::get(path, |_| async { Ok(BootResponse::text("ok")) });

        assert!(
            matches!(result, Err(BootError::InvalidRoutePath(_))),
            "{path} should be rejected"
        );
    }
}

#[test]
fn rejects_controller_prefixes_with_query_or_fragment_markers() {
    for prefix in ["/items?active=true", "/items#active"] {
        let result = ControllerDefinition::new(prefix);

        assert!(
            matches!(result, Err(BootError::InvalidRoutePath(_))),
            "{prefix} should be rejected"
        );
    }
}

#[test]
fn rejects_global_prefixes_with_query_or_fragment_markers() {
    for prefix in ["/api?version=1", "/api#v1"] {
        let result = BootApplication::builder()
            .global_prefix(prefix)
            .route(
                RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) })
                    .unwrap(),
            )
            .build();

        assert!(
            matches!(result, Err(BootError::InvalidRoutePath(_))),
            "{prefix} should be rejected"
        );
    }
}

#[test]
fn rejects_malformed_route_param_segments() {
    for path in [
        "/items/{id",
        "/items/id}",
        "/items/{}",
        "/items/{id}-{slug}",
        "/files/{*}",
        "/files/{*path}/tail",
        "/files/{*path}-{id}",
    ] {
        let result = RouteDefinition::get(path, |_| async { Ok(BootResponse::text("ok")) });

        assert!(
            matches!(result, Err(BootError::InvalidRoutePath(_))),
            "{path} should be rejected"
        );
    }
}

#[test]
fn rejects_duplicate_route_param_names() {
    let result = RouteDefinition::get("/orgs/{id}/items/{id}", |_| async {
        Ok(BootResponse::text("ok"))
    });

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));

    let result = RouteDefinition::get("/files/{path}/{*path}", |_| async {
        Ok(BootResponse::text("ok"))
    });

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

#[test]
fn route_definitions_expose_path_shape_and_param_names() {
    let route = RouteDefinition::get("/orgs/{org_id}/items/{item_id}", |_| async {
        Ok(BootResponse::text("ok"))
    })
    .unwrap();

    assert_eq!(route.path_shape(), "/orgs/{}/items/{}");
    assert_eq!(route.path_param_names(), vec!["org_id", "item_id"]);
}

#[test]
fn route_definitions_expose_catch_all_path_shape_and_params() {
    let route =
        RouteDefinition::get("/files/{*path}", |_| async { Ok(BootResponse::text("ok")) }).unwrap();

    let nested = route
        .path_params("/files/readme%2Emd/versions/v1")
        .unwrap()
        .unwrap();
    let empty = route.path_params("/files").unwrap().unwrap();
    let slash = route.path_params("/files/a%2Fb/c").unwrap().unwrap();

    assert_eq!(route.path_shape(), "/files/{*}");
    assert_eq!(route.path_param_names(), vec!["path"]);
    assert!(route.matches_path("/files"));
    assert!(route.matches_path("/files/readme%2Emd/versions/v1"));
    assert_eq!(
        nested.get("path").map(String::as_str),
        Some("readme.md/versions/v1")
    );
    assert_eq!(empty.get("path").map(String::as_str), Some(""));
    assert_eq!(slash.get("path").map(String::as_str), Some("a/b/c"));
}

#[test]
fn route_definitions_expose_host_shape_and_param_names() {
    let route = RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_host(":tenant.example.com")
        .unwrap();

    let params = route
        .host_params(Some("Acme.example.com:3000"))
        .unwrap()
        .unwrap();

    assert_eq!(route.host(), Some(":tenant.example.com"));
    assert_eq!(route.host_shape().as_deref(), Some("{}.example.com"));
    assert_eq!(route.host_param_names(), vec!["tenant"]);
    assert!(route.matches_host(Some("ACME.example.com")));
    assert!(!route.matches_host(Some("example.com")));
    assert_eq!(params.get("tenant").map(String::as_str), Some("Acme"));
}

#[test]
fn rejects_malformed_host_patterns() {
    for host in [
        "",
        "https://example.com",
        "{tenant.example.com",
        "{id}.{id}.com",
    ] {
        let result = RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("ok")) })
            .unwrap()
            .with_host(host);

        assert!(
            matches!(result, Err(BootError::InvalidHostPattern(_))),
            "{host} should be rejected"
        );
    }
}

#[test]
fn route_definitions_match_paths_and_decode_path_params() {
    let route = RouteDefinition::get("/files/{path}/versions/{version}", |_| async {
        Ok(BootResponse::text("ok"))
    })
    .unwrap();

    let params = route
        .path_params("/files/readme%2Emd/versions/v1")
        .unwrap()
        .unwrap();
    let error = route.path_params("/files/%ZZ/versions/v1").unwrap_err();

    assert!(route.matches_path("/files/readme%2Emd/versions/v1"));
    assert!(!route.matches_path("/files/readme%2Emd"));
    assert_eq!(params.get("path").map(String::as_str), Some("readme.md"));
    assert_eq!(params.get("version").map(String::as_str), Some("v1"));
    assert!(route
        .path_params("/tools/readme%2Emd/versions/v1")
        .unwrap()
        .is_none());
    assert!(matches!(error, BootError::BadRequest(_)));
}

#[test]
fn rejects_duplicate_route_param_names_after_controller_prefixing() {
    let result = ControllerDefinition::new("/orgs/{id}")
        .unwrap()
        .get("/items/{id}", |_| async { Ok(BootResponse::text("ok")) });

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

#[test]
fn controller_routes_expose_prefixed_path_shape_and_param_names() {
    let controller = ControllerDefinition::new("/orgs/{org_id}")
        .unwrap()
        .get("/items/{item_id}", |_| async {
            Ok(BootResponse::text("ok"))
        })
        .unwrap();
    let route = &controller.routes()[0];

    assert_eq!(route.path_shape(), "/orgs/{}/items/{}");
    assert_eq!(route.path_param_names(), vec!["org_id", "item_id"]);
}

#[test]
fn rejects_duplicate_route_param_names_after_global_prefixing() {
    let result = BootApplication::builder()
        .global_prefix("/orgs/{id}")
        .route(
            RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("ok")) })
                .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

#[test]
fn globally_prefixed_routes_expose_final_path_shape_and_param_names() {
    let app = BootApplication::builder()
        .global_prefix("/api/{version}")
        .route(
            RouteDefinition::get("/items/{item_id}", |_| async {
                Ok(BootResponse::text("ok"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let route = &app.routes()[0];

    assert_eq!(route.path_shape(), "/api/{}/items/{}");
    assert_eq!(route.path_param_names(), vec!["version", "item_id"]);
}

#[test]
fn rejects_duplicate_routes_with_the_same_method_and_path() {
    let result = BootApplication::builder()
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .route(
            RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("still ok")) })
                .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::DuplicateRoute(_))));
}

#[test]
fn rejects_duplicate_all_routes_with_the_same_path() {
    let result = BootApplication::builder()
        .route(RouteDefinition::all("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .route(
            RouteDefinition::all("/health", |_| async { Ok(BootResponse::text("still ok")) })
                .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::DuplicateRoute(_))));
}

#[test]
fn allows_all_routes_to_share_paths_with_exact_methods() {
    let app = BootApplication::builder()
        .route(RouteDefinition::all("/items", |_| async { Ok(BootResponse::text("all")) }).unwrap())
        .route(RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("get")) }).unwrap())
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 2);
}

#[test]
fn rejects_routes_with_the_same_method_and_ambiguous_path_shape() {
    let result = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("by id")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::get("/items/{slug}", |_| async {
                Ok(BootResponse::text("by slug"))
            })
            .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::DuplicateRoute(_))));
}

#[test]
fn rejects_routes_with_the_same_method_and_catch_all_path_shape() {
    let result = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{*path}", |_| async {
                Ok(BootResponse::text("path"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/files/{*rest}", |_| async {
                Ok(BootResponse::text("rest"))
            })
            .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::DuplicateRoute(_))));
}

#[test]
fn rejects_routes_with_the_same_method_path_and_host_shape() {
    let result = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("one")) })
                .unwrap()
                .with_host("{tenant}.example.com")
                .unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("two")) })
                .unwrap()
                .with_host(":account.example.com")
                .unwrap(),
        )
        .build();

    assert!(matches!(result, Err(BootError::DuplicateRoute(_))));
}

#[test]
fn allows_routes_that_share_a_path_with_different_hosts() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("acme")) })
                .unwrap()
                .with_host("acme.example.com")
                .unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("globex")) })
                .unwrap()
                .with_host("globex.example.com")
                .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 2);
}

#[test]
fn allows_routes_that_share_a_path_with_different_methods() {
    let app = BootApplication::builder()
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .route(
            RouteDefinition::post("/health", |_| async { Ok(BootResponse::text("created")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 2);
}

#[test]
fn allows_ambiguous_path_shapes_for_different_methods() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("get")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/{slug}", |_| async {
                Ok(BootResponse::text("post"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 2);
}

#[test]
fn allows_routes_that_differ_by_trailing_slash() {
    let app = BootApplication::builder()
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .route(
            RouteDefinition::get("/health/", |_| async { Ok(BootResponse::text("ok slash")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(app.routes().len(), 2);
}

#[test]
fn applies_global_prefix_to_direct_routes() {
    let app = BootApplication::builder()
        .global_prefix("/api/v1")
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build()
        .unwrap();

    assert_eq!(app.routes()[0].path(), "/api/v1/health");
}

#[tokio::test]
async fn global_prefix_exclusions_leave_matching_routes_unprefixed() {
    let app = BootApplication::builder()
        .global_prefix("/api")
        .exclude_global_prefix([MiddlewareRoute::get("/health").unwrap()])
        .route(
            RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("get")) }).unwrap(),
        )
        .route(
            RouteDefinition::post("/health", |_| async { Ok(BootResponse::text("post")) }).unwrap(),
        )
        .route(
            RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(
                    request.param("id").unwrap_or("missing").to_string(),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    assert!(app
        .routes()
        .iter()
        .any(|route| route.method() == HttpMethod::Get && route.path() == "/health"));
    assert!(app
        .routes()
        .iter()
        .any(|route| route.method() == HttpMethod::Post && route.path() == "/api/health"));
    assert!(app
        .routes()
        .iter()
        .any(|route| route.method() == HttpMethod::Get && route.path() == "/api/items/{id}"));

    let health = app
        .call(BootRequest::new(HttpMethod::Get, "/health"))
        .await
        .unwrap();
    assert_eq!(health.body_text().unwrap(), "get");

    let item = app
        .call(BootRequest::new(HttpMethod::Get, "/api/items/42"))
        .await
        .unwrap();
    assert_eq!(item.body_text().unwrap(), "42");
}

#[test]
fn rejects_relative_global_prefixes() {
    let result = BootApplication::builder()
        .global_prefix("api")
        .route(RouteDefinition::get("/health", |_| async { Ok(BootResponse::text("ok")) }).unwrap())
        .build();

    assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
}

#[tokio::test]
async fn application_call_prefers_static_routes_over_param_routes() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "dynamic:{}",
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/items/new"))
        .await
        .unwrap();

    assert_eq!(response.body, b"static");
}

#[tokio::test]
async fn application_call_uses_catch_all_after_static_and_param_routes() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{*path}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "catch:{}",
                    request.param("path").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/files/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "param:{}",
                    request.param("id").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/files/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let static_response = app
        .call(BootRequest::new(HttpMethod::Get, "/files/new"))
        .await
        .unwrap();
    let param_response = app
        .call(BootRequest::new(HttpMethod::Get, "/files/readme"))
        .await
        .unwrap();
    let catch_response = app
        .call(BootRequest::new(HttpMethod::Get, "/files/docs/readme%2Emd"))
        .await
        .unwrap();

    assert_eq!(static_response.body_text().unwrap(), "static");
    assert_eq!(param_response.body_text().unwrap(), "param:readme");
    assert_eq!(catch_response.body_text().unwrap(), "catch:docs/readme.md");
}

#[tokio::test]
async fn application_call_prefers_static_path_before_method_matching() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap()
                .with_filter(
                    |context: a3s_boot::ExecutionContext, error: BootError| async move {
                        Ok(Some(
                            BootResponse::text(format!("{}: {error}", context.route_path))
                                .with_status(405),
                        ))
                    },
                ),
        )
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/items/new"))
        .await
        .unwrap();

    assert_eq!(response.status, 405);
    assert_eq!(
        response.body,
        b"/items/new: method is not allowed: GET /items/new"
    );
}

#[tokio::test]
async fn all_routes_dispatch_standard_methods() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::all("/items", |request: BootRequest| async move {
                Ok(BootResponse::text(request.method().as_str()))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    for method in HttpMethod::standard_methods() {
        let response = app.call(BootRequest::new(*method, "/items")).await.unwrap();

        assert_eq!(response.body_text().unwrap(), method.as_str());
    }
}

#[tokio::test]
async fn exact_methods_take_precedence_over_all_routes() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::all("/items", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "all:{}",
                    request.method().as_str()
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("exact:get")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let get = app
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();
    let post = app
        .call(BootRequest::new(HttpMethod::Post, "/items"))
        .await
        .unwrap();

    assert_eq!(get.body_text().unwrap(), "exact:get");
    assert_eq!(post.body_text().unwrap(), "all:POST");
}

#[test]
fn allowed_methods_use_the_most_specific_matching_path_shape() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::delete("/items/{slug}", |_| async {
                Ok(BootResponse::text("deleted"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(app.allowed_methods("/items/new"), vec![HttpMethod::Post]);
    assert_eq!(
        app.allowed_methods("/items/hammer"),
        vec![HttpMethod::Get, HttpMethod::Delete]
    );
    assert!(app.allowed_methods("/tools/hammer").is_empty());
}

#[test]
fn allowed_methods_header_uses_the_most_specific_matching_path_shape() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::delete("/items/{slug}", |_| async {
                Ok(BootResponse::text("deleted"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(
        app.allowed_methods_header("/items/hammer").as_deref(),
        Some("GET,DELETE")
    );
    assert_eq!(
        app.allowed_methods_header("/items/new").as_deref(),
        Some("POST")
    );
    assert!(app.allowed_methods_header("/tools/hammer").is_none());
}

#[test]
fn allowed_methods_expand_all_routes_to_standard_methods() {
    let app = BootApplication::builder()
        .route(RouteDefinition::all("/items", |_| async { Ok(BootResponse::text("all")) }).unwrap())
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("exact")) }).unwrap(),
        )
        .build()
        .unwrap();

    assert_eq!(
        app.allowed_methods("/items"),
        HttpMethod::standard_methods().to_vec()
    );
    assert_eq!(
        app.allowed_methods_header("/items").as_deref(),
        Some("GET,POST,PUT,PATCH,DELETE,OPTIONS,HEAD")
    );
}

#[test]
fn route_for_uses_the_most_specific_matching_path_shape() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::delete("/items/{slug}", |_| async {
                Ok(BootResponse::text("deleted"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let static_route = app.route_for(HttpMethod::Post, "/items/new").unwrap();
    let get_dynamic_route = app.route_for(HttpMethod::Get, "/items/hammer").unwrap();
    let delete_dynamic_route = app.route_for(HttpMethod::Delete, "/items/hammer").unwrap();

    assert_eq!(static_route.path(), "/items/new");
    assert_eq!(get_dynamic_route.path(), "/items/{id}");
    assert_eq!(delete_dynamic_route.path(), "/items/{slug}");
    assert!(app.route_for(HttpMethod::Get, "/items/new").is_none());
    assert!(app.route_for(HttpMethod::Get, "/tools/hammer").is_none());
}

#[test]
fn route_for_uses_all_routes_as_method_fallbacks() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::all("/items/{id}", |_| async { Ok(BootResponse::text("all")) })
                .unwrap(),
        )
        .route(
            RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("get")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let get = app.route_for(HttpMethod::Get, "/items/hammer").unwrap();
    let post = app.route_for(HttpMethod::Post, "/items/hammer").unwrap();

    assert_eq!(get.method(), HttpMethod::Get);
    assert_eq!(post.method(), HttpMethod::All);
}

#[test]
fn route_match_returns_route_and_decoded_path_params() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{path}/versions/{version}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/files/new/versions/latest", |_| async {
                Ok(BootResponse::text("static"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let dynamic_match = app
        .route_match(HttpMethod::Get, "/files/readme%2Emd/versions/v1")
        .unwrap()
        .unwrap();
    let static_match = app
        .route_match(HttpMethod::Post, "/files/new/versions/latest")
        .unwrap()
        .unwrap();

    assert_eq!(
        dynamic_match.route().path(),
        "/files/{path}/versions/{version}"
    );
    assert_eq!(dynamic_match.param("path"), Some("readme.md"));
    assert_eq!(dynamic_match.param("version"), Some("v1"));
    assert_eq!(dynamic_match.params().len(), 2);
    assert_eq!(static_match.route().path(), "/files/new/versions/latest");
    assert!(static_match.params().is_empty());
}

#[test]
fn route_match_returns_catch_all_path_params() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{*path}", |_| async {
                Ok(BootResponse::text("catch"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let matched = app
        .route_match(HttpMethod::Get, "/files/docs/readme%2Emd")
        .unwrap()
        .unwrap();

    assert_eq!(matched.route().path(), "/files/{*path}");
    assert_eq!(matched.param("path"), Some("docs/readme.md"));
}

#[test]
fn route_match_uses_all_routes_as_method_fallbacks() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::all("/items/{id}", |_| async { Ok(BootResponse::text("all")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let matched = app
        .route_match(HttpMethod::Patch, "/items/hammer")
        .unwrap()
        .unwrap();

    assert_eq!(matched.route().method(), HttpMethod::All);
    assert_eq!(matched.param("id"), Some("hammer"));
}

#[test]
fn route_match_uses_selected_method_and_reports_param_decode_errors() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("dynamic"))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items/new", |_| async { Ok(BootResponse::text("static")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let decode_error = app.route_match(HttpMethod::Get, "/items/%ZZ").unwrap_err();

    assert!(matches!(decode_error, BootError::BadRequest(_)));
    assert!(app
        .route_match(HttpMethod::Get, "/items/new")
        .unwrap()
        .is_none());
    assert!(app
        .route_match(HttpMethod::Post, "/items/%ZZ")
        .unwrap()
        .is_none());
    assert!(app
        .route_match(HttpMethod::Get, "/tools/new")
        .unwrap()
        .is_none());
}

#[tokio::test]
async fn application_call_dispatches_matching_routes() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "{}:{}",
                    request.param("id").unwrap_or("missing"),
                    request.query_param("verbose").unwrap_or("missing")
                )))
            })
            .unwrap(),
        )
        .route(
            RouteDefinition::post("/items", |_| async { Ok(BootResponse::text("created")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let get_response = app
        .call(BootRequest::new(
            HttpMethod::Get,
            "/items/hammer?verbose=true",
        ))
        .await
        .unwrap();
    let post_response = app
        .call(BootRequest::new(HttpMethod::Post, "/items"))
        .await
        .unwrap();

    assert_eq!(get_response.body, b"hammer:true");
    assert_eq!(post_response.body, b"created");
}

#[tokio::test]
async fn application_call_prefers_matching_host_specific_routes() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("global")) }).unwrap(),
        )
        .route(
            RouteDefinition::get("/items", |request: BootRequest| async move {
                Ok(BootResponse::text(format!(
                    "tenant:{}",
                    request.host_param("tenant").unwrap_or("missing")
                )))
            })
            .unwrap()
            .with_host("{tenant}.example.com")
            .unwrap(),
        )
        .build()
        .unwrap();

    let tenant = app
        .call(
            BootRequest::new(HttpMethod::Get, "/items")
                .with_header("host", "acme.example.com:3000"),
        )
        .await
        .unwrap();
    let global = app
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();

    assert_eq!(tenant.body_text().unwrap(), "tenant:acme");
    assert_eq!(global.body_text().unwrap(), "global");
}

#[tokio::test]
async fn application_call_ignores_routes_with_non_matching_hosts() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap()
            .with_host("api.example.com")
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/items").with_header("host", "www.example.com"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::NotFound(message) if message == "GET /items"
    ));
}

#[tokio::test]
async fn application_handle_converts_unhandled_errors_to_responses() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("found")) })
                .unwrap(),
        )
        .build()
        .unwrap();

    let ok = app
        .handle(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await;
    let not_found = app
        .handle(BootRequest::new(HttpMethod::Get, "/tools/hammer"))
        .await;
    let method_not_allowed = app
        .handle(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await;

    assert_eq!(ok.status, 200);
    assert_eq!(ok.body_text().unwrap(), "found");
    assert_eq!(not_found.status, 404);
    assert_eq!(
        not_found.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({
            "statusCode": 404,
            "message": "GET /tools/hammer",
            "error": "Not Found"
        })
    );
    assert_eq!(method_not_allowed.status, 405);
    assert_eq!(
        method_not_allowed.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({
            "statusCode": 405,
            "message": "POST /items/hammer",
            "error": "Method Not Allowed"
        })
    );
}

#[tokio::test]
async fn application_handle_preserves_filter_responses() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Err(BootError::BadRequest("invalid item".to_string()))
            })
            .unwrap()
            .with_filter(
                |context: a3s_boot::ExecutionContext, error: BootError| async move {
                    Ok(Some(
                        BootResponse::text(format!("{}: {error}", context.route_path))
                            .with_status(422),
                    ))
                },
            ),
        )
        .build()
        .unwrap();

    let response = app
        .handle(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await;

    assert_eq!(response.status, 422);
    assert_eq!(
        response.body_text().unwrap(),
        "/items/{id}: bad request: invalid item"
    );
}

#[tokio::test]
async fn application_call_reports_method_not_allowed_for_matching_paths() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::MethodNotAllowed(message) if message == "POST /items/hammer"
    ));
}

#[tokio::test]
async fn application_call_reports_not_found_when_no_route_matches() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/items/{id}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/tools/hammer"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::NotFound(message) if message == "GET /tools/hammer"
    ));
}

#[tokio::test]
async fn application_call_rejects_invalid_percent_encoded_path_params() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/files/{path}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[tokio::test]
async fn application_call_reports_method_not_allowed_without_decoding_path_params() {
    let app = BootApplication::builder()
        .route(
            RouteDefinition::post("/files/{path}", |_| async {
                Ok(BootResponse::text("unreachable"))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::MethodNotAllowed(message) if message == "GET /files/%ZZ"
    ));
}

#[tokio::test]
async fn route_call_rejects_non_matching_method_and_path() {
    let route = RouteDefinition::get("/items/{id}", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap();

    let method_error = route
        .call(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await
        .unwrap_err();
    let path_error = route
        .call(BootRequest::new(HttpMethod::Get, "/tools/hammer"))
        .await
        .unwrap_err();

    assert!(matches!(method_error, BootError::MethodNotAllowed(_)));
    assert!(matches!(path_error, BootError::NotFound(_)));
}

#[tokio::test]
async fn route_handle_converts_unhandled_errors_to_responses() {
    let route =
        RouteDefinition::get("/items/{id}", |_| async { Ok(BootResponse::text("found")) }).unwrap();

    let ok = route
        .handle(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await;
    let not_found = route
        .handle(BootRequest::new(HttpMethod::Get, "/tools/hammer"))
        .await;
    let method_not_allowed = route
        .handle(BootRequest::new(HttpMethod::Post, "/items/hammer"))
        .await;

    assert_eq!(ok.status, 200);
    assert_eq!(ok.body_text().unwrap(), "found");
    assert_eq!(not_found.status, 404);
    assert_eq!(
        not_found.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({
            "statusCode": 404,
            "message": "GET /tools/hammer",
            "error": "Not Found"
        })
    );
    assert_eq!(method_not_allowed.status, 405);
    assert_eq!(
        method_not_allowed.body_json::<serde_json::Value>().unwrap(),
        serde_json::json!({
            "statusCode": 405,
            "message": "POST /items/hammer",
            "error": "Method Not Allowed"
        })
    );
}

#[tokio::test]
async fn route_handle_preserves_filter_responses() {
    let route = RouteDefinition::get("/items/{id}", |_| async {
        Err(BootError::BadRequest("invalid item".to_string()))
    })
    .unwrap()
    .with_filter(
        |context: a3s_boot::ExecutionContext, error: BootError| async move {
            Ok(Some(
                BootResponse::text(format!("{}: {error}", context.route_path)).with_status(422),
            ))
        },
    );

    let response = route
        .handle(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await;

    assert_eq!(response.status, 422);
    assert_eq!(
        response.body_text().unwrap(),
        "/items/{id}: bad request: invalid item"
    );
}

#[tokio::test]
async fn route_handle_converts_path_param_decode_errors_to_responses() {
    let route = RouteDefinition::get("/files/{path}", |_| async {
        Ok(BootResponse::text("unreachable"))
    })
    .unwrap();

    let response = route
        .handle(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await;

    assert_eq!(response.status, 400);
}

#[tokio::test]
async fn route_response_headers_apply_to_successful_responses() {
    let route = RouteDefinition::get("/cached", |_| async { Ok(BootResponse::text("ok")) })
        .unwrap()
        .with_response_header("cache-control", "max-age=60");

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/cached"))
        .await
        .unwrap();

    assert_eq!(response.body, b"ok");
    assert_eq!(response.header("cache-control"), Some("max-age=60"));
}

#[tokio::test]
async fn route_redirect_replaces_successful_responses() {
    let route = RouteDefinition::get("/old", |_| async { Ok(BootResponse::text("unreachable")) })
        .unwrap()
        .with_redirect_status(301, "/new");

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/old"))
        .await
        .unwrap();

    assert_eq!(response.status(), 301);
    assert_eq!(response.location(), Some("/new"));
    assert!(response.body().is_empty());
}

#[tokio::test]
async fn route_call_distinguishes_trailing_slash_paths() {
    let route =
        RouteDefinition::get("/items", |_| async { Ok(BootResponse::text("items")) }).unwrap();
    let slash_route = RouteDefinition::get("/items/", |_| async {
        Ok(BootResponse::text("items slash"))
    })
    .unwrap();

    let response = route
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();
    let slash_response = slash_route
        .call(BootRequest::new(HttpMethod::Get, "/items/"))
        .await
        .unwrap();
    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/items/"))
        .await
        .unwrap_err();

    assert_eq!(response.body, b"items");
    assert_eq!(slash_response.body, b"items slash");
    assert!(matches!(error, BootError::NotFound(_)));
}

#[tokio::test]
async fn route_calls_percent_decode_path_params() {
    let controller = ControllerDefinition::new("/files")
        .unwrap()
        .get("/{path}", |request: BootRequest| async move {
            Ok(BootResponse::text(
                request.param("path").unwrap_or("missing").to_string(),
            ))
        })
        .unwrap();

    let response = controller.routes()[0]
        .call(BootRequest::new(
            HttpMethod::Get,
            "/files/reports%2F2026%20summary",
        ))
        .await
        .unwrap();

    assert_eq!(response.body, b"reports/2026 summary");
}

#[tokio::test]
async fn route_calls_decode_typed_path_params() {
    #[derive(Debug, Deserialize)]
    struct FileParams {
        org_id: u64,
        path: String,
    }

    let controller = ControllerDefinition::new("/orgs")
        .unwrap()
        .get(
            "/{org_id}/files/{path}",
            |request: BootRequest| async move {
                let params = request.params::<FileParams>()?;
                Ok(BootResponse::text(format!(
                    "{}:{}",
                    params.org_id, params.path
                )))
            },
        )
        .unwrap();

    let response = controller.routes()[0]
        .call(BootRequest::new(
            HttpMethod::Get,
            "/orgs/42/files/readme%2Emd",
        ))
        .await
        .unwrap();

    assert_eq!(response.body, b"42:readme.md");
}

#[tokio::test]
async fn route_calls_reject_invalid_typed_path_params() {
    #[derive(Debug, Deserialize)]
    struct ItemParams {
        id: u64,
    }

    let route = RouteDefinition::get("/items/{id}", |request: BootRequest| async move {
        let params = request.params::<ItemParams>()?;
        Ok(BootResponse::text(params.id.to_string()))
    })
    .unwrap();

    let error = route
        .call(BootRequest::new(HttpMethod::Get, "/items/not-a-number"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[tokio::test]
async fn route_calls_reject_invalid_utf8_path_params() {
    let controller = ControllerDefinition::new("/files")
        .unwrap()
        .get("/{path}", |_| async {
            Ok(BootResponse::text("unreachable"))
        })
        .unwrap();

    let error = controller.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/files/%FF"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[tokio::test]
async fn route_calls_reject_invalid_percent_triplets_in_path_params() {
    let controller = ControllerDefinition::new("/files")
        .unwrap()
        .get("/{path}", |_| async {
            Ok(BootResponse::text("unreachable"))
        })
        .unwrap();

    let error = controller.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/files/%ZZ"))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}
