use a3s_boot::{
    BootApplication, BootError, BootRequest, BoxFuture, ExecutionContext, ExecutionProtocol, Guard,
    HttpMethod, Module, ModuleRef, ProviderDefinition, Result, WebSocketContext,
    WebSocketGatewayConnection, WebSocketGatewayDefinition, WebSocketGatewayInitContext,
    WebSocketInterceptor, WebSocketMessage,
};
use serde_json::json;
use std::sync::Arc;

#[derive(Debug)]
struct WsService;

impl WsService {
    fn greeting(&self) -> &'static str {
        "hello"
    }
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
