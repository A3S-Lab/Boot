use a3s_boot::{
    BootApplication, BootError, BoxFuture, ExecutionContext, ExecutionInterceptor,
    ExecutionProtocol, ExecutionTransportKind, Guard, InProcessTransport, MessagePatternDefinition,
    MessageTransport, Module, ModuleRef, ProviderDefinition, Result, TransportContext,
    TransportInterceptor, TransportMessage, TransportReply, Validate,
};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct TransportCatsService;

impl TransportCatsService {
    fn find_one(&self, id: String) -> TransportCatDto {
        TransportCatDto {
            id,
            name: "Milo".to_string(),
        }
    }
}

#[derive(Debug)]
struct TransportCatsModule;

impl Module for TransportCatsModule {
    fn name(&self) -> &'static str {
        "transport-cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(TransportCatsService)])
    }

    fn message_patterns(&self, module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        let cats = module_ref.get::<TransportCatsService>()?;
        Ok(vec![MessagePatternDefinition::request_json(
            "cat.find",
            move |payload: TransportFindCat| {
                let cats = Arc::clone(&cats);
                async move { Ok(cats.find_one(payload.id)) }
            },
        )?])
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct TransportFindCat {
    id: String,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
struct TransportCatDto {
    id: String,
    name: String,
}

#[tokio::test]
async fn message_patterns_dispatch_request_response_messages() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request_json(
                "math.double",
                |payload: NumberPayload| async move {
                    Ok(NumberPayload {
                        value: payload.value * 2,
                    })
                },
            )
            .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(
            TransportMessage::json("math.double", &NumberPayload { value: 21 }).unwrap(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 42);
}

#[tokio::test]
async fn message_patterns_can_use_module_providers() {
    let app = BootApplication::builder()
        .import(TransportCatsModule)
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(
            TransportMessage::json(
                "cat.find",
                &TransportFindCat {
                    id: "42".to_string(),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply.data_as::<TransportCatDto>().unwrap(),
        TransportCatDto {
            id: "42".to_string(),
            name: "Milo".to_string(),
        }
    );
    assert_eq!(
        app.message_patterns()[0].module_name(),
        Some("transport-cats")
    );
}

#[tokio::test]
async fn event_patterns_are_event_only() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let observed = Arc::clone(&events);
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("cat.created", move |payload: TransportCatDto| {
                let observed = Arc::clone(&observed);
                async move {
                    observed.lock().unwrap().push(payload.name);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(
            TransportMessage::json(
                "cat.created",
                &TransportCatDto {
                    id: "1".to_string(),
                    name: "Luna".to_string(),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(reply, None);
    assert_eq!(events.lock().unwrap().as_slice(), ["Luna"]);
}

#[tokio::test]
async fn message_patterns_validate_payloads() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request_validated_json(
                "cat.create",
                |payload: CreateTransportCat| async move {
                    Ok(TransportCatDto {
                        id: "created".to_string(),
                        name: payload.name,
                    })
                },
            )
            .unwrap(),
        )
        .build()
        .unwrap();

    let error = app
        .dispatch_message(
            TransportMessage::json(
                "cat.create",
                &CreateTransportCat {
                    name: " ".to_string(),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap_err();

    assert!(
        matches!(error, BootError::BadRequest(message) if message.contains("name is required"))
    );
}

#[tokio::test]
async fn transport_pipeline_runs_in_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let pipe_log = Arc::clone(&log);
    let guard_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let pattern = MessagePatternDefinition::request("ping", move |message: TransportMessage| {
        let handler_log = Arc::clone(&handler_log);
        async move {
            handler_log.lock().unwrap().push("handler".to_string());
            Ok(TransportReply::new(message.data))
        }
    })
    .unwrap()
    .with_pipe(move |mut message: TransportMessage| {
        let pipe_log = Arc::clone(&pipe_log);
        async move {
            pipe_log.lock().unwrap().push("pipe".to_string());
            message.data = json!({ "from": "pipe" });
            Ok(message)
        }
    })
    .with_guard(move |context: TransportContext| {
        let guard_log = Arc::clone(&guard_log);
        async move {
            guard_log
                .lock()
                .unwrap()
                .push(format!("guard:{}", context.pattern));
            Ok(true)
        }
    })
    .with_interceptor(TraceTransportInterceptor::new("message", Arc::clone(&log)));

    let app = BootApplication::builder()
        .message_pattern(pattern)
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new("ping", json!({ "from": "client" })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "from": "pipe" }));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "guard:ping",
            "before:message",
            "pipe",
            "handler",
            "after:message"
        ]
    );
}

#[tokio::test]
async fn transport_patterns_can_use_shared_execution_hooks() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let pattern =
        MessagePatternDefinition::request("ping", |message: TransportMessage| async move {
            Ok(TransportReply::new(message.data))
        })
        .unwrap()
        .with_execution_guard(SharedTransportExecutionPolicy {
            log: Arc::clone(&log),
        })
        .with_execution_interceptor(SharedTransportExecutionPolicy {
            log: Arc::clone(&log),
        });

    let reply = pattern
        .dispatch(TransportMessage::new("ping", json!({ "ok": true })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "ok": true }));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "guard:transport:ping:request-response",
            "before:transport:ping",
            "after:transport:ping"
        ]
    );
}

#[tokio::test]
async fn in_process_transport_builds_a_message_client() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request_json(
                "math.double",
                |payload: NumberPayload| async move {
                    Ok(NumberPayload {
                        value: payload.value * 2,
                    })
                },
            )
            .unwrap(),
        )
        .message_pattern(
            MessagePatternDefinition::event_json(
                "math.observed",
                |_payload: NumberPayload| async { Ok(()) },
            )
            .unwrap(),
        )
        .build()
        .unwrap();
    let client = InProcessTransport::new().build(app).unwrap();

    let reply = client
        .send(TransportMessage::json("math.double", &NumberPayload { value: 7 }).unwrap())
        .await
        .unwrap()
        .unwrap();
    client
        .emit(TransportMessage::json("math.observed", &NumberPayload { value: 1 }).unwrap())
        .await
        .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 14);
}

#[derive(Debug, Deserialize, Serialize)]
struct NumberPayload {
    value: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct CreateTransportCat {
    name: String,
}

impl Validate for CreateTransportCat {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

#[derive(Clone)]
struct SharedTransportExecutionPolicy {
    log: Arc<Mutex<Vec<String>>>,
}

impl Guard for SharedTransportExecutionPolicy {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            let transport = context.transport_context().expect("transport context");
            log.lock().unwrap().push(format!(
                "guard:{}:{}:{}",
                context.protocol().as_str(),
                transport.pattern.as_str(),
                transport.kind.as_str()
            ));
            Ok(context.protocol() == ExecutionProtocol::Transport
                && transport.kind == ExecutionTransportKind::RequestResponse)
        })
    }
}

impl ExecutionInterceptor for SharedTransportExecutionPolicy {
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            let transport = context.transport_context().expect("transport context");
            log.lock().unwrap().push(format!(
                "before:{}:{}",
                context.protocol().as_str(),
                transport.pattern.as_str()
            ));
            Ok(())
        })
    }

    fn after(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            let transport = context.transport_context().expect("transport context");
            log.lock().unwrap().push(format!(
                "after:{}:{}",
                context.protocol().as_str(),
                transport.pattern.as_str()
            ));
            Ok(())
        })
    }
}

#[derive(Clone)]
struct TraceTransportInterceptor {
    name: &'static str,
    log: Arc<Mutex<Vec<String>>>,
}

impl TraceTransportInterceptor {
    fn new(name: &'static str, log: Arc<Mutex<Vec<String>>>) -> Self {
        Self { name, log }
    }
}

impl TransportInterceptor for TraceTransportInterceptor {
    fn before(&self, _context: TransportContext) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("before:{name}"));
            Ok(())
        })
    }

    fn after(
        &self,
        _context: TransportContext,
        reply: Option<TransportReply>,
    ) -> BoxFuture<'static, Result<Option<TransportReply>>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("after:{name}"));
            Ok(reply)
        })
    }
}
