use a3s_boot::{
    BootError, BootRequest, BootResponse, ControllerDefinition, HttpMethod, RouteDefinition,
    SseEvent,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateItemDto {
    name: String,
}

#[derive(Debug, PartialEq, Eq, Serialize, Deserialize)]
struct ItemDto {
    name: String,
    active: bool,
}

#[tokio::test]
async fn json_controller_routes_decode_dtos_and_encode_responses() {
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .post_json("/", |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: false,
            })
        })
        .unwrap();
    let route = controller.routes()[0].clone();

    let response = route
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "application/json")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(
        response.headers.get("content-type").map(String::as_str),
        Some("application/json")
    );
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&response.body).unwrap(),
        ItemDto {
            name: "Hammer".to_string(),
            active: false,
        }
    );
}

#[tokio::test]
async fn sse_controller_routes_stream_events_and_require_event_stream_accept() {
    let controller = ControllerDefinition::new("/cats")
        .unwrap()
        .sse("/events", |_| async {
            Ok(futures_util::stream::iter([
                Ok::<_, BootError>(SseEvent::new("Milo").with_event("cat.created")),
                Ok::<_, BootError>(SseEvent::new("Luna").with_id("2")),
            ]))
        })
        .unwrap();
    let route = controller.routes()[0].clone();

    let rejected = route
        .call(
            BootRequest::new(HttpMethod::Get, "/cats/events")
                .with_header("accept", "application/json"),
        )
        .await
        .unwrap_err();

    assert!(matches!(
        rejected,
        BootError::NotAcceptable(message)
            if message == "expected client to accept text/event-stream response"
    ));

    let response = route
        .call(
            BootRequest::new(HttpMethod::Get, "/cats/events")
                .with_header("accept", "text/event-stream"),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), 200);
    assert!(response.is_streaming());
    assert_eq!(
        response.header("content-type"),
        Some("text/event-stream; charset=utf-8")
    );

    let mut stream = response.into_sse_stream().unwrap();
    assert_eq!(
        String::from_utf8(stream.next().await.unwrap().unwrap().encode()).unwrap(),
        "event: cat.created\ndata: Milo\n\n"
    );
    assert_eq!(
        String::from_utf8(stream.next().await.unwrap().unwrap().encode()).unwrap(),
        "id: 2\ndata: Luna\n\n"
    );
    assert!(stream.next().await.is_none());
}

#[tokio::test]
async fn json_controller_routes_can_set_response_statuses() {
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .post_json_with_status("/", 201, |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: true,
            })
        })
        .unwrap()
        .get_json_with_status("/{id}", 203, |request: BootRequest| async move {
            Ok(ItemDto {
                name: request.param("id").unwrap_or("missing").to_string(),
                active: false,
            })
        })
        .unwrap()
        .delete_json_with_status("/{id}", 202, |request: BootRequest| async move {
            Ok(ItemDto {
                name: request.param("id").unwrap_or("missing").to_string(),
                active: false,
            })
        })
        .unwrap();
    let post_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Post)
        .unwrap();
    let get_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Get)
        .unwrap();
    let delete_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Delete)
        .unwrap();

    let post_response = post_route
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap();
    let get_response = get_route
        .call(BootRequest::new(HttpMethod::Get, "/items/hammer"))
        .await
        .unwrap();
    let delete_response = delete_route
        .call(BootRequest::new(HttpMethod::Delete, "/items/hammer"))
        .await
        .unwrap();

    assert_eq!(post_response.status, 201);
    assert_eq!(get_response.status, 203);
    assert_eq!(delete_response.status, 202);
    assert_eq!(
        post_response.header("content-type"),
        Some("application/json")
    );
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&post_response.body).unwrap(),
        ItemDto {
            name: "Hammer".to_string(),
            active: true,
        }
    );
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&get_response.body).unwrap(),
        ItemDto {
            name: "hammer".to_string(),
            active: false,
        }
    );
}

#[tokio::test]
async fn route_json_helpers_can_set_response_statuses() {
    let put_route = RouteDefinition::put_json_with_status(
        "/items/{id}",
        202,
        |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: true,
            })
        },
    )
    .unwrap();
    let patch_route = RouteDefinition::patch_json_with_status(
        "/items/{id}",
        200,
        |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: false,
            })
        },
    )
    .unwrap();

    let put_response = put_route
        .call(
            BootRequest::new(HttpMethod::Put, "/items/hammer")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap();
    let patch_response = patch_route
        .call(
            BootRequest::new(HttpMethod::Patch, "/items/hammer")
                .with_content_type("application/json")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap();

    assert_eq!(put_response.status, 202);
    assert_eq!(patch_response.status, 200);
    assert_eq!(
        put_response.header("content-type"),
        Some("application/json")
    );
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&patch_response.body).unwrap(),
        ItemDto {
            name: "Hammer".to_string(),
            active: false,
        }
    );
}

#[tokio::test]
async fn json_controller_routes_reject_invalid_json_as_bad_request() {
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .post_json("/", |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: false,
            })
        })
        .unwrap();

    let error = controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "application/json")
                .with_body("{"),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(_)));
}

#[tokio::test]
async fn json_controller_routes_require_json_content_type() {
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .post_json("/", |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: false,
            })
        })
        .unwrap();
    let route = controller.routes()[0].clone();

    let vendor_response = route
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "application/vnd.api+json; charset=utf-8")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap();
    let missing_error = route
        .call(BootRequest::new(HttpMethod::Post, "/items").with_body(r#"{"name":"Hammer"}"#))
        .await
        .unwrap_err();
    let text_error = route
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "text/plain")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap_err();

    assert_eq!(
        serde_json::from_slice::<ItemDto>(&vendor_response.body).unwrap(),
        ItemDto {
            name: "Hammer".to_string(),
            active: false,
        }
    );
    assert!(matches!(
        missing_error,
        BootError::UnsupportedMediaType(message) if message == "expected JSON content type"
    ));
    assert!(matches!(
        text_error,
        BootError::UnsupportedMediaType(message) if message == "expected JSON content type, got text/plain"
    ));
}

#[tokio::test]
async fn json_controller_routes_require_json_accept_headers() {
    let get_controller = ControllerDefinition::new("/items")
        .unwrap()
        .get_json("/{id}", |request: BootRequest| async move {
            Ok(ItemDto {
                name: request.param("id").unwrap_or("missing").to_string(),
                active: true,
            })
        })
        .unwrap();
    let post_controller = ControllerDefinition::new("/items")
        .unwrap()
        .post_json("/", |dto: CreateItemDto| async move {
            Ok(ItemDto {
                name: dto.name,
                active: false,
            })
        })
        .unwrap();

    let get_error = get_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Get, "/items/hammer").with_header("accept", "text/plain"),
        )
        .await
        .unwrap_err();
    let post_error = post_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "application/json")
                .with_header("accept", "text/plain")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap_err();
    let missing_content_type_post_error = post_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("accept", "text/plain")
                .with_body(r#"{"name":"Hammer"}"#),
        )
        .await
        .unwrap_err();
    let invalid_body_post_error = post_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Post, "/items")
                .with_header("content-type", "application/json")
                .with_header("accept", "text/plain")
                .with_body("{"),
        )
        .await
        .unwrap_err();
    let ok = get_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Get, "/items/hammer")
                .with_header("accept", "application/*"),
        )
        .await
        .unwrap();
    let vendor_ok = get_controller.routes()[0]
        .call(
            BootRequest::new(HttpMethod::Get, "/items/hammer")
                .with_header("accept", "application/problem+json"),
        )
        .await
        .unwrap();

    assert!(matches!(
        get_error,
        BootError::NotAcceptable(message) if message == "expected client to accept JSON response"
    ));
    assert!(matches!(
        post_error,
        BootError::NotAcceptable(message) if message == "expected client to accept JSON response"
    ));
    assert!(matches!(
        missing_content_type_post_error,
        BootError::UnsupportedMediaType(message) if message == "expected JSON content type"
    ));
    assert!(matches!(
        invalid_body_post_error,
        BootError::NotAcceptable(message) if message == "expected client to accept JSON response"
    ));
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&ok.body).unwrap(),
        ItemDto {
            name: "hammer".to_string(),
            active: true,
        }
    );
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&vendor_ok.body).unwrap(),
        ItemDto {
            name: "hammer".to_string(),
            active: true,
        }
    );
}

#[tokio::test]
async fn controller_routes_support_additional_methods_and_json_responses() {
    let controller = ControllerDefinition::new("/tools")
        .unwrap()
        .get_json("/{id}", |request: BootRequest| async move {
            Ok(ItemDto {
                name: request.param("id").unwrap_or("missing").to_string(),
                active: true,
            })
        })
        .unwrap()
        .patch("/raw", |request: BootRequest| async move {
            Ok(BootResponse::text(request.text()?))
        })
        .unwrap()
        .delete_json("/{id}", |request: BootRequest| async move {
            Ok(ItemDto {
                name: request.param("id").unwrap_or("missing").to_string(),
                active: false,
            })
        })
        .unwrap()
        .options("/raw", |_| async { Ok(BootResponse::text("options")) })
        .unwrap()
        .head("/raw", |_| async { Ok(BootResponse::no_content()) })
        .unwrap();

    let get_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Get)
        .unwrap();
    let patch_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Patch)
        .unwrap();
    let delete_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Delete)
        .unwrap();
    let options_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Options)
        .unwrap();
    let head_route = controller
        .routes()
        .iter()
        .find(|route| route.method() == HttpMethod::Head)
        .unwrap();

    let get_response = get_route
        .call(BootRequest::new(HttpMethod::Get, "/tools/hammer"))
        .await
        .unwrap();
    let patch_response = patch_route
        .call(BootRequest::new(HttpMethod::Patch, "/tools/raw").with_body("patched"))
        .await
        .unwrap();
    let delete_response = delete_route
        .call(BootRequest::new(HttpMethod::Delete, "/tools/hammer"))
        .await
        .unwrap();
    let options_response = options_route
        .call(BootRequest::new(HttpMethod::Options, "/tools/raw"))
        .await
        .unwrap();
    let head_response = head_route
        .call(BootRequest::new(HttpMethod::Head, "/tools/raw"))
        .await
        .unwrap();

    assert_eq!(
        serde_json::from_slice::<ItemDto>(&get_response.body).unwrap(),
        ItemDto {
            name: "hammer".to_string(),
            active: true,
        }
    );
    assert_eq!(patch_response.body, b"patched");
    assert_eq!(
        serde_json::from_slice::<ItemDto>(&delete_response.body).unwrap(),
        ItemDto {
            name: "hammer".to_string(),
            active: false,
        }
    );
    assert_eq!(options_response.body, b"options");
    assert_eq!(head_response.status, 204);
    assert!(head_response.body.is_empty());
}

#[derive(Debug, Deserialize)]
struct ItemQueryDto {
    verbose: bool,
}

#[tokio::test]
async fn route_calls_extract_path_params_and_query_params() {
    let controller = ControllerDefinition::new("/items")
        .unwrap()
        .get("/{id}", |request: BootRequest| async move {
            let query: ItemQueryDto = request.query()?;
            Ok(BootResponse::text(format!(
                "{}:{}:{}",
                request.param("id").unwrap_or("missing"),
                request.query_param("verbose").unwrap_or("missing"),
                query.verbose
            )))
        })
        .unwrap();

    let response = controller.routes()[0]
        .call(BootRequest::new(
            HttpMethod::Get,
            "/items/hammer?verbose=true",
        ))
        .await
        .unwrap();

    assert_eq!(response.body, b"hammer:true:true");
}
