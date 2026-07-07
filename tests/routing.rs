use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition, HttpMethod,
    RouteDefinition,
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
    assert_eq!(not_found.body_text().unwrap(), "GET /tools/hammer");
    assert_eq!(method_not_allowed.status, 405);
    assert_eq!(
        method_not_allowed.body_text().unwrap(),
        "POST /items/hammer"
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
    assert_eq!(not_found.body_text().unwrap(), "GET /tools/hammer");
    assert_eq!(method_not_allowed.status, 405);
    assert_eq!(
        method_not_allowed.body_text().unwrap(),
        "POST /items/hammer"
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
