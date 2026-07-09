use a3s_boot::{
    BootApplication, BootError, BootErrorKind, BootRequest, BoxFuture, ExecutionContext,
    ExecutionProtocol, Guard, HttpMethod, Module, ModuleRef, ProviderDefinition, Result, Validate,
    ValidationOptions, ValidationSchema, WebSocketContext, WebSocketExceptionResponse,
    WebSocketGatewayConnection, WebSocketGatewayDefinition, WebSocketGatewayInitContext,
    WebSocketGatewayServer, WebSocketInterceptor, WebSocketMessage,
    WebSocketSubscriptionDefinition,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug)]
struct WsService;

impl WsService {
    fn greeting(&self) -> &'static str {
        "hello"
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct WsCreateCat {
    name: String,
    #[serde(default = "default_ws_cat_kind")]
    kind: String,
}

impl Validate for WsCreateCat {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

impl ValidationSchema for WsCreateCat {
    fn allowed_fields() -> &'static [&'static str] {
        &["kind", "name"]
    }
}

fn default_ws_cat_kind() -> String {
    "cat".to_string()
}

#[test]
fn websocket_message_extracts_data_fields() {
    let message = WebSocketMessage::new(
        "cat.find",
        json!({
            "id": "42",
            "page": 3,
            "tag": null,
        }),
    );

    assert_eq!(message.data_field_as::<String>("id").unwrap(), "42");
    assert_eq!(message.data_field_as::<u16>("page").unwrap(), 3);
    assert_eq!(
        message.optional_data_field_as::<String>("tag").unwrap(),
        None
    );
    assert_eq!(
        message.optional_data_field_as::<String>("missing").unwrap(),
        None
    );
    assert_eq!(message.data_field_string("page").unwrap(), "3");

    let missing = message.data_field_as::<String>("missing").unwrap_err();
    assert!(
        matches!(missing, BootError::BadRequest(message) if message == "missing websocket data field: missing")
    );

    let non_object = WebSocketMessage::new("cat.find", json!("42"));
    let error = non_object.data_field_as::<String>("id").unwrap_err();
    assert!(
        matches!(error, BootError::BadRequest(message) if message == "expected JSON object websocket data")
    );
}

#[derive(Debug)]
struct WsModule;

impl Module for WsModule {
    fn name(&self) -> &'static str {
        "ws"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(WsService)])
    }

    fn gateways(&self, module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        let service = module_ref.get::<WsService>()?;
        Ok(vec![WebSocketGatewayDefinition::new("/ws")?.subscribe(
            "hello",
            move |_| {
                let service = Arc::clone(&service);
                async move { Ok(WebSocketMessage::text("hello.reply", service.greeting())) }
            },
        )?])
    }
}

#[derive(Clone)]
struct SharedExecutionGuard {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Guard for SharedExecutionGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            let websocket = context.websocket_context().expect("websocket context");
            log.lock().unwrap().push(format!(
                "{}:{}:{}",
                context.protocol().as_str(),
                websocket.gateway_path.as_str(),
                websocket.event.as_str()
            ));
            Ok(context.protocol() == ExecutionProtocol::WebSocket)
        })
    }
}

#[tokio::test]
async fn websocket_gateway_dispatches_messages_by_event() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })
        .unwrap();
    let connection = gateway
        .connect(BootRequest::new(HttpMethod::Get, "/events"))
        .unwrap();

    let reply = connection
        .dispatch(WebSocketMessage::new("ping", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::new("pong", json!({ "id": 1 })));
}

#[tokio::test]
async fn websocket_gateway_handlers_can_access_the_connection() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .subscribe_with_connection(
            "whoami",
            |connection: WebSocketGatewayConnection, message: WebSocketMessage| async move {
                Ok(WebSocketMessage::new(
                    "whoami.reply",
                    json!({
                        "connectionId": connection.id(),
                        "path": connection.request().path(),
                        "payload": message.data,
                    }),
                ))
            },
        )
        .unwrap();
    let connection = gateway
        .connect(BootRequest::new(HttpMethod::Get, "/events"))
        .unwrap();

    let reply = connection
        .dispatch(WebSocketMessage::new("whoami", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new(
            "whoami.reply",
            json!({
                "connectionId": connection.id(),
                "path": "/events",
                "payload": { "id": 1 },
            }),
        )
    );
}

#[tokio::test]
async fn websocket_gateway_handlers_can_access_the_server_handle() {
    let first_sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let second_sent = Arc::new(std::sync::Mutex::new(Vec::new()));
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .subscribe_with_server(
            "announce",
            |server: WebSocketGatewayServer, message: WebSocketMessage| async move {
                let room = message.data_field_as::<String>("room")?;
                let sent = server
                    .broadcast_to_room(&room, WebSocketMessage::text("announcement", "room"))
                    .await?;
                Ok(WebSocketMessage::new(
                    "announced",
                    json!({
                        "path": server.path(),
                        "connections": server.active_connection_count()?,
                        "sent": sent,
                    }),
                ))
            },
        )
        .unwrap();
    let first = gateway
        .connect_async_with_outbound(BootRequest::new(HttpMethod::Get, "/events"), {
            let first_sent = Arc::clone(&first_sent);
            move |message: WebSocketMessage| {
                let first_sent = Arc::clone(&first_sent);
                async move {
                    first_sent.lock().unwrap().push(message);
                    Ok(())
                }
            }
        })
        .await
        .unwrap();
    let second = gateway
        .connect_async_with_outbound(BootRequest::new(HttpMethod::Get, "/events"), {
            let second_sent = Arc::clone(&second_sent);
            move |message: WebSocketMessage| {
                let second_sent = Arc::clone(&second_sent);
                async move {
                    second_sent.lock().unwrap().push(message);
                    Ok(())
                }
            }
        })
        .await
        .unwrap();
    first.join("room:cats").unwrap();
    second.join("room:cats").unwrap();

    let reply = first
        .dispatch(WebSocketMessage::new(
            "announce",
            json!({ "room": "room:cats" }),
        ))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        reply,
        WebSocketMessage::new(
            "announced",
            json!({
                "path": "/events",
                "connections": 2,
                "sent": 2,
            }),
        )
    );
    assert_eq!(
        first_sent.lock().unwrap().as_slice(),
        &[WebSocketMessage::text("announcement", "room")]
    );
    assert_eq!(
        second_sent.lock().unwrap().as_slice(),
        &[WebSocketMessage::text("announcement", "room")]
    );

    let server = gateway.server();
    assert_eq!(
        server.active_connection_ids().unwrap(),
        vec![first.id(), second.id()]
    );
    assert_eq!(server.rooms().unwrap(), vec!["room:cats".to_string()]);
    assert_eq!(
        server.room_members("room:cats").unwrap(),
        vec![first.id(), second.id()]
    );
    assert!(server
        .emit_to_connection(second.id(), WebSocketMessage::text("direct", "ok"))
        .await
        .unwrap());
    assert_eq!(
        second_sent.lock().unwrap().as_slice(),
        &[
            WebSocketMessage::text("announcement", "room"),
            WebSocketMessage::text("direct", "ok"),
        ]
    );
    assert_eq!(first.server().active_connection_count().unwrap(), 2);

    first.close().await.unwrap();
    second.close().await.unwrap();
}

#[tokio::test]
async fn websocket_gateway_lifecycle_hooks_run_for_init_connection_and_disconnect() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let init_log = Arc::clone(&log);
    let connection_log = Arc::clone(&log);
    let disconnect_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let app = BootApplication::builder()
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .with_after_init(move |context: WebSocketGatewayInitContext| {
                    let init_log = Arc::clone(&init_log);
                    async move {
                        init_log.lock().unwrap().push(format!(
                            "init:{}:{}",
                            context.gateway_path,
                            context.events.join(",")
                        ));
                        Ok(())
                    }
                })
                .with_connection_hook(move |connection: WebSocketGatewayConnection| {
                    let connection_log = Arc::clone(&connection_log);
                    async move {
                        connection_log
                            .lock()
                            .unwrap()
                            .push(format!("connect:{}", connection.request().path()));
                        Ok(())
                    }
                })
                .with_disconnect_hook(move |connection: WebSocketGatewayConnection| {
                    let disconnect_log = Arc::clone(&disconnect_log);
                    async move {
                        disconnect_log
                            .lock()
                            .unwrap()
                            .push(format!("disconnect:{}", connection.request().path()));
                        Ok(())
                    }
                })
                .subscribe("ping", move |message: WebSocketMessage| {
                    let handler_log = Arc::clone(&handler_log);
                    async move {
                        handler_log.lock().unwrap().push("handler".to_string());
                        Ok(WebSocketMessage::new("pong", message.data))
                    }
                })
                .unwrap(),
        )
        .build()
        .unwrap();

    app.bootstrap().await.unwrap();
    let gateway = app.gateway_for("/events").unwrap();
    let connection = gateway
        .connect_async(BootRequest::new(HttpMethod::Get, "/events"))
        .await
        .unwrap();
    let reply = connection
        .dispatch(WebSocketMessage::new("ping", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();
    connection.close().await.unwrap();

    assert_eq!(reply, WebSocketMessage::new("pong", json!({ "id": 1 })));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "init:/events:ping",
            "connect:/events",
            "handler",
            "disconnect:/events"
        ]
    );
}

#[tokio::test]
async fn websocket_gateway_captures_catch_all_path_params() {
    let gateway = WebSocketGatewayDefinition::new("/events/{*topic}")
        .unwrap()
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })
        .unwrap();
    let connection = gateway
        .connect(BootRequest::new(
            HttpMethod::Get,
            "/events/cats/created%2Ev1",
        ))
        .unwrap();

    assert_eq!(connection.request().param("topic"), Some("cats/created.v1"));
}

#[tokio::test]
async fn websocket_gateways_can_use_module_providers() {
    let app = BootApplication::builder().import(WsModule).build().unwrap();
    let gateway = app.gateway_for("/ws").unwrap();

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/ws"),
            WebSocketMessage::new("hello", json!(null)),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::text("hello.reply", "hello"));
}

#[derive(Clone)]
struct TraceWsInterceptor {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl TraceWsInterceptor {
    fn new(name: &'static str, log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { name, log }
    }
}

impl WebSocketInterceptor for TraceWsInterceptor {
    fn before(&self, _context: WebSocketContext) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("before:{name}"));
            Ok(())
        })
    }

    fn after(
        &self,
        _context: WebSocketContext,
        reply: Option<WebSocketMessage>,
    ) -> BoxFuture<'static, Result<Option<WebSocketMessage>>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("after:{name}"));
            Ok(reply)
        })
    }
}

#[tokio::test]
async fn websocket_gateway_pipeline_runs_in_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let pipe_log = Arc::clone(&log);
    let guard_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_pipe(move |mut message: WebSocketMessage| {
            let pipe_log = Arc::clone(&pipe_log);
            async move {
                pipe_log.lock().unwrap().push("pipe".to_string());
                message.data = json!({ "from": "pipe" });
                Ok(message)
            }
        })
        .with_guard(move |context: WebSocketContext| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                guard_log
                    .lock()
                    .unwrap()
                    .push(format!("guard:{}", context.event));
                Ok(true)
            }
        })
        .with_interceptor(TraceWsInterceptor::new("gateway", Arc::clone(&log)))
        .subscribe("ping", move |message: WebSocketMessage| {
            let handler_log = Arc::clone(&handler_log);
            async move {
                handler_log.lock().unwrap().push("handler".to_string());
                Ok(WebSocketMessage::new("pong", message.data))
            }
        })
        .unwrap();

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({ "from": "client" })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new("pong", json!({ "from": "pipe" }))
    );
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "guard:ping",
            "before:gateway",
            "pipe",
            "handler",
            "after:gateway"
        ]
    );
}

#[tokio::test]
async fn global_websocket_pipes_apply_before_gateway_and_subscription_pipes() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let global_log = Arc::clone(&log);
    let gateway_log = Arc::clone(&log);
    let subscription_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let app = BootApplication::builder()
        .use_global_websocket_pipe(move |mut message: WebSocketMessage| {
            let global_log = Arc::clone(&global_log);
            async move {
                global_log.lock().unwrap().push(format!(
                    "pipe:global:{}",
                    message.data["stage"].as_str().unwrap()
                ));
                message.data = json!({ "stage": "global" });
                Ok(message)
            }
        })
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .with_pipe(move |mut message: WebSocketMessage| {
                    let gateway_log = Arc::clone(&gateway_log);
                    async move {
                        gateway_log.lock().unwrap().push(format!(
                            "pipe:gateway:{}",
                            message.data["stage"].as_str().unwrap()
                        ));
                        message.data = json!({ "stage": "gateway" });
                        Ok(message)
                    }
                })
                .subscribe_definition(
                    "ping",
                    WebSocketSubscriptionDefinition::new(move |message: WebSocketMessage| {
                        let handler_log = Arc::clone(&handler_log);
                        async move {
                            handler_log.lock().unwrap().push(format!(
                                "handler:{}",
                                message.data["stage"].as_str().unwrap()
                            ));
                            Ok(WebSocketMessage::new("pong", message.data))
                        }
                    })
                    .with_pipe(move |mut message: WebSocketMessage| {
                        let subscription_log = Arc::clone(&subscription_log);
                        async move {
                            subscription_log.lock().unwrap().push(format!(
                                "pipe:subscription:{}",
                                message.data["stage"].as_str().unwrap()
                            ));
                            message.data = json!({ "stage": "subscription" });
                            Ok(message)
                        }
                    }),
                )
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({ "stage": "client" })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new("pong", json!({ "stage": "subscription" }))
    );
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "pipe:global:client",
            "pipe:gateway:global",
            "pipe:subscription:gateway",
            "handler:subscription"
        ]
    );
}

#[tokio::test]
async fn global_websocket_guards_and_interceptors_wrap_gateway_and_subscription_hooks() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let global_guard_log = Arc::clone(&log);
    let gateway_guard_log = Arc::clone(&log);
    let subscription_guard_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let app = BootApplication::builder()
        .use_global_websocket_guard(move |context: WebSocketContext| {
            let global_guard_log = Arc::clone(&global_guard_log);
            async move {
                global_guard_log
                    .lock()
                    .unwrap()
                    .push(format!("guard:global:{}", context.event));
                Ok(true)
            }
        })
        .use_global_websocket_interceptor(TraceWsInterceptor::new("global", Arc::clone(&log)))
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .with_guard(move |context: WebSocketContext| {
                    let gateway_guard_log = Arc::clone(&gateway_guard_log);
                    async move {
                        gateway_guard_log
                            .lock()
                            .unwrap()
                            .push(format!("guard:gateway:{}", context.event));
                        Ok(true)
                    }
                })
                .with_interceptor(TraceWsInterceptor::new("gateway", Arc::clone(&log)))
                .subscribe_definition(
                    "ping",
                    WebSocketSubscriptionDefinition::new(move |message: WebSocketMessage| {
                        let handler_log = Arc::clone(&handler_log);
                        async move {
                            handler_log.lock().unwrap().push("handler".to_string());
                            Ok(WebSocketMessage::new("pong", message.data))
                        }
                    })
                    .with_guard(move |context: WebSocketContext| {
                        let subscription_guard_log = Arc::clone(&subscription_guard_log);
                        async move {
                            subscription_guard_log
                                .lock()
                                .unwrap()
                                .push(format!("guard:subscription:{}", context.event));
                            Ok(true)
                        }
                    })
                    .with_interceptor(TraceWsInterceptor::new("subscription", Arc::clone(&log))),
                )
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({ "id": 1 })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::new("pong", json!({ "id": 1 })));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "guard:global:ping",
            "guard:gateway:ping",
            "guard:subscription:ping",
            "before:global",
            "before:gateway",
            "before:subscription",
            "handler",
            "after:subscription",
            "after:gateway",
            "after:global"
        ]
    );
}

#[tokio::test]
async fn global_validation_options_merge_into_websocket_payload_validators() {
    let app = BootApplication::builder()
        .use_global_validation_options(ValidationOptions::new().transform(true).whitelist(true))
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .subscribe_definition(
                    "cat.create",
                    WebSocketSubscriptionDefinition::new(|message: WebSocketMessage| async move {
                        Ok(WebSocketMessage::new("cat.created", message.data))
                    })
                    .with_payload_validation_options::<WsCreateCat>(ValidationOptions::default()),
                )
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("cat.create", json!({ "name": "Milo", "role": "admin" })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new("cat.created", json!({ "kind": "cat", "name": "Milo" }))
    );
}

#[tokio::test]
async fn websocket_payload_validation_can_opt_out_of_global_validation() {
    let app = BootApplication::builder()
        .use_global_validation()
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .subscribe_definition(
                    "cat.create",
                    WebSocketSubscriptionDefinition::new(|message: WebSocketMessage| async move {
                        Ok(WebSocketMessage::new("cat.created", message.data))
                    })
                    .with_payload_validation::<WsCreateCat>()
                    .without_validation(),
                )
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("cat.create", json!({ "name": "   ", "kind": "cat" })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new("cat.created", json!({ "name": "   ", "kind": "cat" }))
    );
}

#[tokio::test]
async fn websocket_gateway_can_use_shared_execution_guard() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_execution_guard(SharedExecutionGuard {
            log: Arc::clone(&log),
        })
        .subscribe("ping", |_| async {
            Ok(WebSocketMessage::text("pong", "ok"))
        })
        .unwrap();

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!(null)),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::text("pong", "ok"));
    assert_eq!(log.lock().unwrap().as_slice(), ["websocket:/events:ping"]);
}

#[tokio::test]
async fn websocket_gateway_events_expose_metadata_to_execution_context() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let guard_log = Arc::clone(&log);
    let interceptor_log = Arc::clone(&log);

    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_metadata_value("resource", json!("events"))
        .with_guard(move |context: WebSocketContext| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                guard_log.lock().unwrap().push(format!(
                    "guard:{}:{}:{}",
                    context.event,
                    context.metadata_as::<String>("resource")?.unwrap(),
                    context.metadata_as::<String>("action")?.unwrap()
                ));
                Ok(true)
            }
        })
        .with_execution_interceptor(move |context: ExecutionContext| {
            let interceptor_log = Arc::clone(&interceptor_log);
            async move {
                interceptor_log.lock().unwrap().push(format!(
                    "before:{}",
                    context.metadata_as::<String>("action")?.unwrap()
                ));
                Ok(())
            }
        })
        .subscribe_definition(
            "ping",
            WebSocketSubscriptionDefinition::new(|message: WebSocketMessage| async move {
                Ok(WebSocketMessage::new("pong", message.data))
            })
            .with_metadata_value("action", json!("read")),
        )
        .unwrap();

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({ "ok": true })),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::new("pong", json!({ "ok": true })));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["guard:ping:events:read", "before:read"]
    );
}

#[tokio::test]
async fn websocket_gateway_guards_can_reject_messages() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_guard(|_| async { Ok(false) })
        .subscribe("ping", |_| async {
            Ok(WebSocketMessage::text("pong", "unreachable"))
        })
        .unwrap();

    let error = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!(null)),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::Forbidden(message) if message == "websocket event /events ping")
    );
}

#[tokio::test]
async fn websocket_exception_filters_can_handle_handler_errors() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_catch_filter(
            [BootErrorKind::BadRequest],
            |context: WebSocketContext, error: BootError| async move {
                Ok(Some(WebSocketExceptionResponse::message(
                    WebSocketMessage::new(
                        "error",
                        json!({
                            "event": context.event,
                            "message": error.to_string(),
                        }),
                    ),
                )))
            },
        )
        .subscribe("ping", |_| async {
            Err::<WebSocketMessage, BootError>(BootError::BadRequest(
                "invalid websocket payload".to_string(),
            ))
        })
        .unwrap();

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({})),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new(
            "error",
            json!({
                "event": "ping",
                "message": "bad request: invalid websocket payload",
            }),
        )
    );
}

#[tokio::test]
async fn websocket_exception_filters_only_handle_matching_error_kinds() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_catch_filter(
            [BootErrorKind::BadRequest],
            |_context: WebSocketContext, _error: BootError| async move {
                Ok(Some(WebSocketExceptionResponse::empty()))
            },
        )
        .subscribe("ping", |_| async {
            Err::<WebSocketMessage, BootError>(BootError::Unauthorized("missing token".to_string()))
        })
        .unwrap();

    let error = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({})),
        )
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::Unauthorized(message) if message == "missing token"));
}

#[tokio::test]
async fn global_websocket_filters_apply_to_gateway_dispatch_errors() {
    let app = BootApplication::builder()
        .use_global_websocket_catch_filter(
            [BootErrorKind::BadRequest],
            |context: WebSocketContext, error: BootError| async move {
                Ok(Some(WebSocketExceptionResponse::message(
                    WebSocketMessage::new(
                        "global.error",
                        json!({
                            "event": context.event,
                            "message": error.to_string(),
                        }),
                    ),
                )))
            },
        )
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .subscribe("ping", |_| async {
                    Err::<WebSocketMessage, BootError>(BootError::BadRequest(
                        "invalid websocket payload".to_string(),
                    ))
                })
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({})),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply,
        WebSocketMessage::new(
            "global.error",
            json!({
                "event": "ping",
                "message": "bad request: invalid websocket payload",
            }),
        )
    );
}

#[tokio::test]
async fn websocket_gateway_filters_take_precedence_over_global_filters() {
    let app = BootApplication::builder()
        .use_global_websocket_filter(|_context: WebSocketContext, _error: BootError| async move {
            Ok(Some(WebSocketExceptionResponse::message(
                WebSocketMessage::text("global.error", "global"),
            )))
        })
        .gateway(
            WebSocketGatewayDefinition::new("/events")
                .unwrap()
                .with_filter(|_context: WebSocketContext, _error: BootError| async move {
                    Ok(Some(WebSocketExceptionResponse::message(
                        WebSocketMessage::text("local.error", "local"),
                    )))
                })
                .subscribe("ping", |_| async {
                    Err::<WebSocketMessage, BootError>(BootError::BadRequest(
                        "invalid websocket payload".to_string(),
                    ))
                })
                .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .gateway_for("/events")
        .unwrap()
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({})),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply, WebSocketMessage::text("local.error", "local"));
}

#[tokio::test]
async fn websocket_subscription_exception_filters_are_scoped_to_events() {
    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .subscribe_definition(
            "handled",
            WebSocketSubscriptionDefinition::new(|_| async {
                Err::<WebSocketMessage, BootError>(BootError::BadRequest(
                    "handled payload".to_string(),
                ))
            })
            .with_catch_filter(
                [BootErrorKind::BadRequest],
                |context: WebSocketContext, error: BootError| async move {
                    Ok(Some(WebSocketExceptionResponse::message(
                        WebSocketMessage::new(
                            "error",
                            json!({
                                "event": context.event,
                                "message": error.to_string(),
                            }),
                        ),
                    )))
                },
            ),
        )
        .unwrap()
        .subscribe("unhandled", |_| async {
            Err::<WebSocketMessage, BootError>(BootError::BadRequest(
                "unhandled payload".to_string(),
            ))
        })
        .unwrap();

    let handled = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("handled", json!({})),
        )
        .await
        .unwrap()
        .unwrap();
    let unhandled = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("unhandled", json!({})),
        )
        .await
        .unwrap_err();

    assert_eq!(
        handled,
        WebSocketMessage::new(
            "error",
            json!({
                "event": "handled",
                "message": "bad request: handled payload",
            }),
        )
    );
    assert!(matches!(unhandled, BootError::BadRequest(message) if message == "unhandled payload"));
}

#[tokio::test]
async fn websocket_gateways_track_namespaces_rooms_and_broadcasts() {
    let sender_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let receiver_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let outside_messages = Arc::new(std::sync::Mutex::new(Vec::new()));
    let room_message = WebSocketMessage::text("cat.created", "Milo");
    let all_message = WebSocketMessage::text("system", "all");
    let direct_message = WebSocketMessage::text("direct", "sender");

    let gateway = WebSocketGatewayDefinition::new("/events")
        .unwrap()
        .with_namespace("cats")
        .unwrap()
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })
        .unwrap();

    let sender = gateway
        .connect_async_with_outbound(
            BootRequest::new(HttpMethod::Get, "/events"),
            capture_outbound(Arc::clone(&sender_messages)),
        )
        .await
        .unwrap();
    let receiver = gateway
        .connect_async_with_outbound(
            BootRequest::new(HttpMethod::Get, "/events"),
            capture_outbound(Arc::clone(&receiver_messages)),
        )
        .await
        .unwrap();
    let outside = gateway
        .connect_async_with_outbound(
            BootRequest::new(HttpMethod::Get, "/events"),
            capture_outbound(Arc::clone(&outside_messages)),
        )
        .await
        .unwrap();

    assert_eq!(gateway.namespace(), Some("/cats"));
    assert_eq!(sender.namespace(), Some("/cats"));
    assert_eq!(gateway.active_connection_count().unwrap(), 3);
    assert_eq!(
        gateway.active_connection_ids().unwrap(),
        [sender.id(), receiver.id(), outside.id()]
    );

    sender.join("room:cats").unwrap();
    receiver.join("room:cats").unwrap();
    outside.join("room:dogs").unwrap();
    assert_eq!(sender.rooms().unwrap(), ["room:cats"]);
    assert_eq!(gateway.rooms().unwrap(), ["room:cats", "room:dogs"]);
    assert_eq!(
        gateway.room_members("room:cats").unwrap(),
        [sender.id(), receiver.id()]
    );

    let delivered_to_room = sender
        .broadcast_to_room("room:cats", room_message.clone())
        .await
        .unwrap();
    assert_eq!(delivered_to_room, 1);
    assert!(sender_messages.lock().unwrap().is_empty());
    assert_eq!(
        receiver_messages.lock().unwrap().as_slice(),
        std::slice::from_ref(&room_message)
    );
    assert!(outside_messages.lock().unwrap().is_empty());

    let delivered_to_all = gateway.broadcast(all_message.clone()).await.unwrap();
    assert_eq!(delivered_to_all, 3);
    assert_eq!(
        sender_messages.lock().unwrap().as_slice(),
        std::slice::from_ref(&all_message)
    );
    assert_eq!(
        receiver_messages.lock().unwrap().as_slice(),
        [room_message.clone(), all_message.clone()]
    );
    assert_eq!(
        outside_messages.lock().unwrap().as_slice(),
        std::slice::from_ref(&all_message)
    );

    assert!(sender.emit(direct_message.clone()).await.unwrap());
    assert_eq!(
        sender_messages.lock().unwrap().as_slice(),
        [all_message, direct_message]
    );

    receiver.close().await.unwrap();
    assert_eq!(gateway.active_connection_count().unwrap(), 2);
    assert_eq!(gateway.room_members("room:cats").unwrap(), [sender.id()]);
    sender.close().await.unwrap();
    outside.close().await.unwrap();
    assert_eq!(gateway.active_connection_count().unwrap(), 0);
}

fn capture_outbound(
    messages: Arc<std::sync::Mutex<Vec<WebSocketMessage>>>,
) -> impl Fn(WebSocketMessage) -> std::future::Ready<Result<()>> + Send + Sync + 'static {
    move |message| {
        messages.lock().unwrap().push(message);
        std::future::ready(Ok(()))
    }
}
