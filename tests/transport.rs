use a3s_boot::{
    BootApplication, BootError, BootErrorKind, BoxFuture, CallHandler, ExecutionContext,
    ExecutionInterceptor, ExecutionProtocol, ExecutionTransportKind, Guard, InProcessTransport,
    MessagePatternDefinition, MessageTransport, Module, ModuleRef, ProviderDefinition, Result,
    TransportContext, TransportExceptionResponse, TransportInterceptor, TransportMessage,
    TransportReply, Validate, ValidationOptions, ValidationSchema,
};
#[cfg(feature = "grpc-transport")]
use a3s_boot::{GrpcTransport, GrpcTransportClient, GrpcTransportOptions};
#[cfg(feature = "kafka-transport")]
use a3s_boot::{KafkaTransport, KafkaTransportClient, KafkaTransportOptions};
#[cfg(feature = "mqtt-transport")]
use a3s_boot::{MqttTransport, MqttTransportClient, MqttTransportOptions, MqttTransportQoS};
#[cfg(feature = "nats-transport")]
use a3s_boot::{NatsTransport, NatsTransportClient, NatsTransportOptions};
#[cfg(feature = "rabbitmq-transport")]
use a3s_boot::{RabbitMqTransport, RabbitMqTransportClient, RabbitMqTransportOptions};
#[cfg(feature = "redis-transport")]
use a3s_boot::{RedisTransport, RedisTransportClient, RedisTransportOptions};
#[cfg(feature = "tcp-transport")]
use a3s_boot::{TcpTransport, TcpTransportClient};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
#[cfg(any(
    feature = "grpc-transport",
    feature = "kafka-transport",
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "rabbitmq-transport",
    feature = "redis-transport",
    feature = "tcp-transport"
))]
use std::time::Duration;
#[cfg(any(
    feature = "kafka-transport",
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "rabbitmq-transport",
    feature = "redis-transport"
))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(any(feature = "grpc-transport", feature = "tcp-transport"))]
use tokio::net::TcpListener;
#[cfg(any(
    feature = "grpc-transport",
    feature = "kafka-transport",
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "rabbitmq-transport",
    feature = "redis-transport",
    feature = "tcp-transport"
))]
use tokio::time::sleep;

#[test]
fn transport_message_extracts_data_fields() {
    let message = TransportMessage::new(
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
        matches!(missing, BootError::BadRequest(message) if message == "missing transport data field: missing")
    );

    let non_object = TransportMessage::new("cat.find", json!("42"));
    let error = non_object.data_field_as::<String>("id").unwrap_err();
    assert!(
        matches!(error, BootError::BadRequest(message) if message == "expected JSON object transport data")
    );
}

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
                    kind: "cat".to_string(),
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
async fn message_patterns_whitelist_payload_properties() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request(
                "cat.create",
                |message: TransportMessage| async move { Ok(message.data) },
            )
            .unwrap()
            .with_payload_validation_options::<CreateTransportCat>(
                ValidationOptions::new().whitelist(true),
            ),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new(
            "cat.create",
            json!({ "name": "Milo", "role": "admin" }),
        ))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data, json!({ "name": "Milo" }));
}

#[tokio::test]
async fn message_patterns_transform_payload_properties() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request(
                "cat.create",
                |message: TransportMessage| async move { Ok(message.data) },
            )
            .unwrap()
            .with_payload_validation_options::<CreateTransportCat>(
                ValidationOptions::new().transform(true),
            ),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new(
            "cat.create",
            json!({ "name": "Milo" }),
        ))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data, json!({ "kind": "cat", "name": "Milo" }));
}

#[tokio::test]
async fn global_validation_options_merge_into_transport_payload_validators() {
    let app = BootApplication::builder()
        .use_global_validation_options(ValidationOptions::new().transform(true).whitelist(true))
        .message_pattern(
            MessagePatternDefinition::request(
                "cat.create",
                |message: TransportMessage| async move { Ok(message.data) },
            )
            .unwrap()
            .with_payload_validation_options::<CreateTransportCat>(ValidationOptions::default()),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new(
            "cat.create",
            json!({ "name": "Milo", "role": "admin" }),
        ))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data, json!({ "kind": "cat", "name": "Milo" }));
}

#[tokio::test]
async fn transport_payload_validation_can_opt_out_of_global_validation() {
    let app = BootApplication::builder()
        .use_global_validation()
        .message_pattern(
            MessagePatternDefinition::request(
                "cat.create",
                |message: TransportMessage| async move { Ok(message.data) },
            )
            .unwrap()
            .with_payload_validation::<CreateTransportCat>()
            .without_validation(),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new(
            "cat.create",
            json!({ "name": "   ", "kind": "cat" }),
        ))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data, json!({ "name": "   ", "kind": "cat" }));
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

struct RecoveringTransportInterceptor;

impl TransportInterceptor for RecoveringTransportInterceptor {
    fn intercept<'a>(
        &'a self,
        _context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            match next.handle().await {
                Ok(reply) => Ok(reply),
                Err(error) => Ok(Some(TransportReply::text(format!("recovered: {error}")))),
            }
        })
    }
}

#[tokio::test]
async fn transport_interceptors_can_recover_errors_before_filters() {
    let filter_calls = Arc::new(AtomicUsize::new(0));
    let filter_log = Arc::clone(&filter_calls);
    let pattern = MessagePatternDefinition::request("cats.recover", |_message| async {
        Err::<TransportReply, BootError>(BootError::BadRequest("invalid cat payload".to_string()))
    })
    .unwrap()
    .with_interceptor(RecoveringTransportInterceptor)
    .with_filter(move |_context: TransportContext, _error: BootError| {
        let filter_log = Arc::clone(&filter_log);
        async move {
            filter_log.fetch_add(1, Ordering::SeqCst);
            Ok(Some(TransportExceptionResponse::reply(
                TransportReply::text("filtered"),
            )))
        }
    });

    let reply = pattern
        .dispatch(TransportMessage::new("cats.recover", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply.data(),
        &json!("recovered: bad request: invalid cat payload")
    );
    assert_eq!(filter_calls.load(Ordering::SeqCst), 0);
}

struct RetryUnavailableTransportInterceptor;

impl TransportInterceptor for RetryUnavailableTransportInterceptor {
    fn intercept<'a>(
        &'a self,
        _context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            match next.handle().await {
                Err(BootError::ServiceUnavailable(_)) => next.handle().await,
                result => result,
            }
        })
    }
}

#[tokio::test]
async fn transport_call_handlers_can_replay_the_downstream_pipeline() {
    let pipe_calls = Arc::new(AtomicUsize::new(0));
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let pipe_log = Arc::clone(&pipe_calls);
    let handler_log = Arc::clone(&handler_calls);
    let pattern = MessagePatternDefinition::request("cats.retry", move |message| {
        let handler_log = Arc::clone(&handler_log);
        async move {
            let attempt = handler_log.fetch_add(1, Ordering::SeqCst) + 1;
            if attempt == 1 {
                return Err(BootError::ServiceUnavailable(
                    "temporary failure".to_string(),
                ));
            }
            Ok(TransportReply::new(message.data))
        }
    })
    .unwrap()
    .with_interceptor(RetryUnavailableTransportInterceptor)
    .with_pipe(move |mut message: TransportMessage| {
        let pipe_log = Arc::clone(&pipe_log);
        async move {
            let attempt = pipe_log.fetch_add(1, Ordering::SeqCst) + 1;
            message.data = json!({ "pipeAttempt": attempt });
            Ok(message)
        }
    });

    let reply = pattern
        .dispatch(TransportMessage::new("cats.retry", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "pipeAttempt": 2 }));
    assert_eq!(pipe_calls.load(Ordering::SeqCst), 2);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 2);
}

struct ShortCircuitTransportInterceptor;

impl TransportInterceptor for ShortCircuitTransportInterceptor {
    fn intercept<'a>(
        &'a self,
        _context: TransportContext,
        _next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async { Ok(Some(TransportReply::text("ignored event reply"))) })
    }
}

#[tokio::test]
async fn event_patterns_discard_short_circuit_interceptor_replies() {
    let handler_calls = Arc::new(AtomicUsize::new(0));
    let handler_log = Arc::clone(&handler_calls);
    let pattern = MessagePatternDefinition::event("cats.created", move |_message| {
        let handler_log = Arc::clone(&handler_log);
        async move {
            handler_log.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    })
    .unwrap()
    .with_interceptor(ShortCircuitTransportInterceptor);

    let reply = pattern
        .dispatch(TransportMessage::new("cats.created", json!({ "id": 1 })))
        .await
        .unwrap();

    assert_eq!(reply, None);
    assert_eq!(handler_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn global_transport_pipes_apply_before_pattern_pipes() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let global_log = Arc::clone(&log);
    let pattern_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let app = BootApplication::builder()
        .use_global_transport_pipe(move |mut message: TransportMessage| {
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
        .message_pattern(
            MessagePatternDefinition::request("ping", move |message: TransportMessage| {
                let handler_log = Arc::clone(&handler_log);
                async move {
                    handler_log.lock().unwrap().push(format!(
                        "handler:{}",
                        message.data["stage"].as_str().unwrap()
                    ));
                    Ok(TransportReply::new(message.data))
                }
            })
            .unwrap()
            .with_pipe(move |mut message: TransportMessage| {
                let pattern_log = Arc::clone(&pattern_log);
                async move {
                    pattern_log.lock().unwrap().push(format!(
                        "pipe:pattern:{}",
                        message.data["stage"].as_str().unwrap()
                    ));
                    message.data = json!({ "stage": "pattern" });
                    Ok(message)
                }
            }),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new("ping", json!({ "stage": "client" })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "stage": "pattern" }));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "pipe:global:client",
            "pipe:pattern:global",
            "handler:pattern"
        ]
    );
}

#[tokio::test]
async fn global_transport_guards_and_interceptors_wrap_pattern_hooks() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let global_guard_log = Arc::clone(&log);
    let pattern_guard_log = Arc::clone(&log);
    let handler_log = Arc::clone(&log);

    let app = BootApplication::builder()
        .use_global_transport_guard(move |context: TransportContext| {
            let global_guard_log = Arc::clone(&global_guard_log);
            async move {
                global_guard_log
                    .lock()
                    .unwrap()
                    .push(format!("guard:global:{}", context.pattern));
                Ok(true)
            }
        })
        .use_global_transport_interceptor(TraceTransportInterceptor::new(
            "global",
            Arc::clone(&log),
        ))
        .message_pattern(
            MessagePatternDefinition::request("ping", move |message: TransportMessage| {
                let handler_log = Arc::clone(&handler_log);
                async move {
                    handler_log.lock().unwrap().push("handler".to_string());
                    Ok(TransportReply::new(message.data))
                }
            })
            .unwrap()
            .with_guard(move |context: TransportContext| {
                let pattern_guard_log = Arc::clone(&pattern_guard_log);
                async move {
                    pattern_guard_log
                        .lock()
                        .unwrap()
                        .push(format!("guard:pattern:{}", context.pattern));
                    Ok(true)
                }
            })
            .with_interceptor(TraceTransportInterceptor::new("pattern", Arc::clone(&log))),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new("ping", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "id": 1 }));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "guard:global:ping",
            "guard:pattern:ping",
            "before:global",
            "before:pattern",
            "handler",
            "after:pattern",
            "after:global"
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
async fn transport_patterns_expose_metadata_to_execution_context() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let guard_log = Arc::clone(&log);
    let interceptor_log = Arc::clone(&log);
    let pattern =
        MessagePatternDefinition::request("cats.secure", |message: TransportMessage| async move {
            Ok(TransportReply::new(message.data))
        })
        .unwrap()
        .with_metadata_value("resource", json!("cats"))
        .with_metadata_value("roles", json!(["admin"]))
        .with_guard(move |context: TransportContext| {
            let guard_log = Arc::clone(&guard_log);
            async move {
                guard_log.lock().unwrap().push(format!(
                    "guard:{}:{}",
                    context.pattern,
                    context.metadata_as::<String>("resource")?.unwrap()
                ));
                Ok(context.metadata_as::<Vec<String>>("roles")? == Some(vec!["admin".to_string()]))
            }
        })
        .with_execution_interceptor(move |context: ExecutionContext| {
            let interceptor_log = Arc::clone(&interceptor_log);
            async move {
                interceptor_log.lock().unwrap().push(format!(
                    "before:{}",
                    context.metadata_as::<String>("resource")?.unwrap()
                ));
                Ok(())
            }
        });

    let reply = pattern
        .dispatch(TransportMessage::new("cats.secure", json!({ "ok": true })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!({ "ok": true }));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["guard:cats.secure:cats", "before:cats"]
    );
}

#[tokio::test]
async fn transport_exception_filters_can_handle_pipeline_errors() {
    let pattern = MessagePatternDefinition::request("cats.fail", |_message| async {
        Err::<TransportReply, BootError>(BootError::BadRequest("invalid cat payload".to_string()))
    })
    .unwrap()
    .with_catch_filter(
        [BootErrorKind::BadRequest],
        |context: TransportContext, error: BootError| async move {
            Ok(Some(TransportExceptionResponse::reply(
                TransportReply::new(json!({
                    "pattern": context.pattern,
                    "message": error.to_string(),
                })),
            )))
        },
    );

    let reply = pattern
        .dispatch(TransportMessage::new("cats.fail", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply.data(),
        &json!({
            "pattern": "cats.fail",
            "message": "bad request: invalid cat payload",
        })
    );
}

#[tokio::test]
async fn transport_exception_filters_only_handle_matching_error_kinds() {
    let pattern = MessagePatternDefinition::request("cats.fail", |_message| async {
        Err::<TransportReply, BootError>(BootError::Unauthorized("missing token".to_string()))
    })
    .unwrap()
    .with_catch_filter(
        [BootErrorKind::BadRequest],
        |_context: TransportContext, _error: BootError| async move {
            Ok(Some(TransportExceptionResponse::empty()))
        },
    );

    let error = pattern
        .dispatch(TransportMessage::new("cats.fail", json!({})))
        .await
        .unwrap_err();

    assert!(matches!(error, BootError::Unauthorized(message) if message == "missing token"));
}

#[tokio::test]
async fn global_transport_filters_apply_to_message_dispatch_errors() {
    let app = BootApplication::builder()
        .use_global_transport_catch_filter(
            [BootErrorKind::BadRequest],
            |context: TransportContext, error: BootError| async move {
                Ok(Some(TransportExceptionResponse::reply(
                    TransportReply::new(json!({
                        "pattern": context.pattern,
                        "message": error.to_string(),
                    })),
                )))
            },
        )
        .message_pattern(
            MessagePatternDefinition::request("cats.fail", |_message| async {
                Err::<TransportReply, BootError>(BootError::BadRequest(
                    "invalid cat payload".to_string(),
                ))
            })
            .unwrap(),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new("cats.fail", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(
        reply.data(),
        &json!({
            "pattern": "cats.fail",
            "message": "bad request: invalid cat payload",
        })
    );
}

#[tokio::test]
async fn transport_pattern_filters_take_precedence_over_global_filters() {
    let app = BootApplication::builder()
        .use_global_transport_filter(|_context: TransportContext, _error: BootError| async move {
            Ok(Some(TransportExceptionResponse::reply(
                TransportReply::text("global"),
            )))
        })
        .message_pattern(
            MessagePatternDefinition::request("cats.fail", |_message| async {
                Err::<TransportReply, BootError>(BootError::BadRequest(
                    "invalid cat payload".to_string(),
                ))
            })
            .unwrap()
            .with_filter(|_context: TransportContext, _error: BootError| async move {
                Ok(Some(TransportExceptionResponse::reply(
                    TransportReply::text("local"),
                )))
            }),
        )
        .build()
        .unwrap();

    let reply = app
        .dispatch_message(TransportMessage::new("cats.fail", json!({ "id": 1 })))
        .await
        .unwrap()
        .unwrap();

    assert_eq!(reply.data(), &json!("local"));
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

#[cfg(feature = "kafka-transport")]
#[test]
fn kafka_transport_builds_a_message_client_with_custom_options() {
    let brokers = vec!["127.0.0.1:9092".to_string()];
    let options = KafkaTransportOptions::new()
        .with_topic_prefix("a3s.boot.test")
        .with_client_id_prefix("a3s-boot-test")
        .with_request_timeout(Duration::from_secs(2))
        .with_partition(1)
        .with_fetch_batch_size(2, 2048)
        .with_fetch_max_wait_ms(250)
        .with_max_message_size(2 * 1024 * 1024)
        .with_auto_create_topics(true)
        .with_topic_replication_factor(1);
    let app = BootApplication::builder().build().unwrap();
    let client = KafkaTransport::with_options(brokers.clone(), options.clone())
        .build(app)
        .unwrap();

    assert_eq!(client.brokers(), brokers.as_slice());
    assert_eq!(client.options(), &options);
    assert_eq!(client.options().request_topic(), "a3s.boot.test.requests");
    assert_eq!(client.options().event_topic(), "a3s.boot.test.events");
    assert_eq!(
        client.options().reply_topic_prefix(),
        "a3s.boot.test.replies"
    );
    assert_eq!(client.options().client_id_prefix(), "a3s-boot-test");
    assert_eq!(client.options().partition(), 1);
    assert_eq!(client.options().fetch_min_batch_size(), 2);
    assert_eq!(client.options().fetch_max_batch_size(), 2048);
    assert_eq!(client.options().fetch_max_wait_ms(), 250);
    assert_eq!(client.options().max_message_size(), 2 * 1024 * 1024);
    assert!(client.options().auto_create_topics());
    assert_eq!(client.options().topic_replication_factor(), 1);
}

#[cfg(feature = "kafka-transport")]
#[tokio::test]
async fn kafka_transport_round_trips_when_kafka_brokers_are_set() {
    let Some(brokers) = kafka_test_brokers() else {
        return;
    };
    let options = KafkaTransportOptions::new()
        .with_topic_prefix(unique_transport_prefix("kafka-round-trip"))
        .with_client_id_prefix("a3s-boot-test")
        .with_request_timeout(Duration::from_secs(3))
        .with_fetch_max_wait_ms(100)
        .with_auto_create_topics(true);
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
    let transport = KafkaTransport::with_options(brokers.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = KafkaTransportClient::with_options(brokers, options);

    let reply = send_kafka_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 9 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 18);
    server.abort();
}

#[cfg(feature = "kafka-transport")]
#[tokio::test]
async fn kafka_transport_emits_events_when_kafka_brokers_are_set() {
    let Some(brokers) = kafka_test_brokers() else {
        return;
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let options = KafkaTransportOptions::new()
        .with_topic_prefix(unique_transport_prefix("kafka-event"))
        .with_client_id_prefix("a3s-boot-test")
        .with_request_timeout(Duration::from_secs(3))
        .with_fetch_max_wait_ms(100)
        .with_auto_create_topics(true);
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let transport = KafkaTransport::with_options(brokers.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = KafkaTransportClient::with_options(brokers, options);

    emit_kafka_until_observed(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 11 }).unwrap(),
        Arc::clone(&observed),
    )
    .await
    .unwrap();

    assert!(observed.lock().unwrap().contains(&11));
    server.abort();
}

#[cfg(feature = "mqtt-transport")]
#[test]
fn mqtt_transport_builds_a_message_client_with_custom_options() {
    let options = MqttTransportOptions::new()
        .with_topic_prefix("a3s/boot/test")
        .with_client_id_prefix("a3s-boot-test")
        .with_qos(MqttTransportQoS::AtLeastOnce)
        .with_request_timeout(Duration::from_secs(2))
        .with_keep_alive(Duration::from_secs(5))
        .with_channel_capacity(16)
        .with_max_packet_size(2 * 1024 * 1024)
        .with_credentials("user", "password");
    let app = BootApplication::builder().build().unwrap();
    let client = MqttTransport::with_options("127.0.0.1", 1883, options.clone())
        .build(app)
        .unwrap();

    assert_eq!(client.host(), "127.0.0.1");
    assert_eq!(client.port(), 1883);
    assert_eq!(client.options(), &options);
    assert_eq!(client.options().request_topic(), "a3s/boot/test/requests");
    assert_eq!(client.options().event_topic(), "a3s/boot/test/events");
    assert_eq!(
        client.options().reply_topic_prefix(),
        "a3s/boot/test/replies"
    );
    assert_eq!(client.options().client_id_prefix(), "a3s-boot-test");
    assert_eq!(client.options().qos(), MqttTransportQoS::AtLeastOnce);
    assert_eq!(client.options().credentials(), Some(("user", "password")));
}

#[cfg(feature = "mqtt-transport")]
#[tokio::test]
async fn mqtt_transport_round_trips_when_mqtt_endpoint_is_set() {
    let Some((host, port)) = mqtt_test_endpoint() else {
        return;
    };
    let options = MqttTransportOptions::new()
        .with_topic_prefix(unique_transport_prefix("mqtt-round-trip"))
        .with_client_id_prefix("a3s-boot-test")
        .with_request_timeout(Duration::from_secs(2));
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
    let transport = MqttTransport::with_options(host.clone(), port, options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = MqttTransportClient::with_options(host, port, options);

    let reply = send_mqtt_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 9 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 18);
    server.abort();
}

#[cfg(feature = "mqtt-transport")]
#[tokio::test]
async fn mqtt_transport_emits_events_when_mqtt_endpoint_is_set() {
    let Some((host, port)) = mqtt_test_endpoint() else {
        return;
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let options = MqttTransportOptions::new()
        .with_topic_prefix(unique_transport_prefix("mqtt-event"))
        .with_client_id_prefix("a3s-boot-test")
        .with_request_timeout(Duration::from_secs(2));
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let transport = MqttTransport::with_options(host.clone(), port, options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = MqttTransportClient::with_options(host, port, options);

    emit_mqtt_until_observed(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 11 }).unwrap(),
        Arc::clone(&observed),
    )
    .await
    .unwrap();

    assert!(observed.lock().unwrap().contains(&11));
    server.abort();
}

#[cfg(feature = "rabbitmq-transport")]
#[test]
fn rabbitmq_transport_builds_a_message_client_with_custom_options() {
    let options = RabbitMqTransportOptions::new()
        .with_queue_prefix("a3s.boot.test")
        .with_request_timeout(Duration::from_secs(2))
        .with_durable(true)
        .with_auto_delete(false);
    let app = BootApplication::builder().build().unwrap();
    let client = RabbitMqTransport::with_options("amqp://127.0.0.1:5672/%2f", options.clone())
        .build(app)
        .unwrap();

    assert_eq!(client.uri(), "amqp://127.0.0.1:5672/%2f");
    assert_eq!(client.options(), &options);
    assert_eq!(client.options().request_queue(), "a3s.boot.test.requests");
    assert_eq!(client.options().event_queue(), "a3s.boot.test.events");
    assert_eq!(
        client.options().reply_queue_prefix(),
        "a3s.boot.test.replies"
    );
    assert_eq!(
        client.options().consumer_tag_prefix(),
        "a3s.boot.test.consumer"
    );
    assert!(client.options().durable());
    assert!(!client.options().auto_delete());
}

#[cfg(feature = "rabbitmq-transport")]
#[tokio::test]
async fn rabbitmq_transport_round_trips_when_rabbitmq_uri_is_set() {
    let Some(uri) = rabbitmq_test_uri() else {
        return;
    };
    let options = RabbitMqTransportOptions::new()
        .with_queue_prefix(unique_transport_prefix("rabbitmq-round-trip"))
        .with_request_timeout(Duration::from_secs(2))
        .with_auto_delete(true);
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
    let transport = RabbitMqTransport::with_options(uri.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = RabbitMqTransportClient::with_options(uri, options);

    let reply = send_rabbitmq_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 9 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 18);
    server.abort();
}

#[cfg(feature = "rabbitmq-transport")]
#[tokio::test]
async fn rabbitmq_transport_emits_events_when_rabbitmq_uri_is_set() {
    let Some(uri) = rabbitmq_test_uri() else {
        return;
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let options = RabbitMqTransportOptions::new()
        .with_queue_prefix(unique_transport_prefix("rabbitmq-event"))
        .with_request_timeout(Duration::from_secs(2))
        .with_auto_delete(true);
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let transport = RabbitMqTransport::with_options(uri.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = RabbitMqTransportClient::with_options(uri, options);

    emit_rabbitmq_until_observed(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 11 }).unwrap(),
        Arc::clone(&observed),
    )
    .await
    .unwrap();

    assert!(observed.lock().unwrap().contains(&11));
    server.abort();
}

#[cfg(feature = "nats-transport")]
#[test]
fn nats_transport_builds_a_message_client_with_custom_options() {
    let options = NatsTransportOptions::new()
        .with_subject_prefix("a3s.boot.test")
        .with_queue_group("workers")
        .with_request_timeout(Duration::from_secs(2));
    let app = BootApplication::builder().build().unwrap();
    let client = NatsTransport::with_options("nats://127.0.0.1:4222", options.clone())
        .build(app)
        .unwrap();

    assert_eq!(client.url(), "nats://127.0.0.1:4222");
    assert_eq!(client.options(), &options);
    assert_eq!(client.options().request_subject(), "a3s.boot.test.requests");
    assert_eq!(client.options().event_subject(), "a3s.boot.test.events");
    assert_eq!(client.options().queue_group(), Some("workers"));
}

#[cfg(feature = "nats-transport")]
#[tokio::test]
async fn nats_transport_round_trips_when_nats_url_is_set() {
    let Some(url) = nats_test_url() else {
        return;
    };
    let options = NatsTransportOptions::new()
        .with_subject_prefix(unique_transport_prefix("nats-round-trip"))
        .with_queue_group("round-trip-workers")
        .with_request_timeout(Duration::from_secs(2));
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
    let transport = NatsTransport::with_options(url.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = NatsTransportClient::with_options(url, options);

    let reply = send_nats_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 9 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 18);
    server.abort();
}

#[cfg(feature = "nats-transport")]
#[tokio::test]
async fn nats_transport_emits_events_when_nats_url_is_set() {
    let Some(url) = nats_test_url() else {
        return;
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let options = NatsTransportOptions::new()
        .with_subject_prefix(unique_transport_prefix("nats-event"))
        .with_queue_group("event-workers")
        .with_request_timeout(Duration::from_secs(2));
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let transport = NatsTransport::with_options(url.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = NatsTransportClient::with_options(url, options);

    emit_nats_until_observed(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 11 }).unwrap(),
        Arc::clone(&observed),
    )
    .await
    .unwrap();

    assert!(observed.lock().unwrap().contains(&11));
    server.abort();
}

#[cfg(feature = "redis-transport")]
#[test]
fn redis_transport_builds_a_message_client_with_custom_options() {
    let options = RedisTransportOptions::new()
        .with_channel_prefix("a3s.boot.test")
        .with_request_timeout(Duration::from_secs(2));
    let app = BootApplication::builder().build().unwrap();
    let client = RedisTransport::with_options("redis://127.0.0.1/", options.clone())
        .build(app)
        .unwrap();

    assert_eq!(client.url(), "redis://127.0.0.1/");
    assert_eq!(client.options(), &options);
    assert_eq!(client.options().request_channel(), "a3s.boot.test.requests");
    assert_eq!(client.options().event_channel(), "a3s.boot.test.events");
    assert_eq!(
        client.options().reply_channel_prefix(),
        "a3s.boot.test.replies"
    );
}

#[cfg(feature = "redis-transport")]
#[tokio::test]
async fn redis_transport_round_trips_when_redis_url_is_set() {
    let Some(url) = redis_test_url() else {
        return;
    };
    let options = RedisTransportOptions::new()
        .with_channel_prefix(unique_transport_prefix("redis-round-trip"))
        .with_request_timeout(Duration::from_secs(2));
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
    let transport = RedisTransport::with_options(url.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = RedisTransportClient::with_options(url, options);

    let reply = send_redis_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 9 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 18);
    server.abort();
}

#[cfg(feature = "redis-transport")]
#[tokio::test]
async fn redis_transport_emits_events_when_redis_url_is_set() {
    let Some(url) = redis_test_url() else {
        return;
    };
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let options = RedisTransportOptions::new()
        .with_channel_prefix(unique_transport_prefix("redis-event"))
        .with_request_timeout(Duration::from_secs(2));
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let transport = RedisTransport::with_options(url.clone(), options.clone());
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = RedisTransportClient::with_options(url, options);

    emit_redis_until_observed(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 11 }).unwrap(),
        Arc::clone(&observed),
    )
    .await
    .unwrap();

    assert!(observed.lock().unwrap().contains(&11));
    server.abort();
}

#[cfg(feature = "tcp-transport")]
#[tokio::test]
async fn tcp_transport_round_trips_request_response_messages() {
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
    let addr = unused_tcp_addr().await;
    let transport = TcpTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = TcpTransport::new(addr).build(app).unwrap();

    let reply = send_tcp_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 12 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 24);
    server.abort();
}

#[cfg(feature = "tcp-transport")]
#[tokio::test]
async fn tcp_transport_emits_event_messages() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let addr = unused_tcp_addr().await;
    let transport = TcpTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = TcpTransport::new(addr).build(app).unwrap();

    send_tcp_with_retry(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 7 }).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(observed.lock().unwrap().as_slice(), [7]);
    server.abort();
}

#[cfg(feature = "tcp-transport")]
#[tokio::test]
async fn tcp_transport_maps_handler_errors_to_client_errors() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request("math.bad", |_message: TransportMessage| async {
                Err::<TransportReply, BootError>(BootError::BadRequest("invalid math".to_string()))
            })
            .unwrap(),
        )
        .message_pattern(
            MessagePatternDefinition::request(
                "math.conflict",
                |_message: TransportMessage| async {
                    Err::<TransportReply, BootError>(BootError::conflict("duplicate math"))
                },
            )
            .unwrap(),
        )
        .message_pattern(
            MessagePatternDefinition::request(
                "math.unprocessable",
                |_message: TransportMessage| async {
                    Err::<TransportReply, BootError>(BootError::unprocessable_entity(
                        "math shape is invalid",
                    ))
                },
            )
            .unwrap(),
        )
        .build()
        .unwrap();
    let addr = unused_tcp_addr().await;
    let transport = TcpTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = TcpTransport::new(addr).build(app).unwrap();

    let error = send_tcp_with_retry(
        &client,
        TransportMessage::new("math.bad", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(message) if message == "invalid math"));

    let error = send_tcp_with_retry(
        &client,
        TransportMessage::new("math.conflict", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, BootError::Conflict(message) if message == "duplicate math"));

    let error = send_tcp_with_retry(
        &client,
        TransportMessage::new("math.unprocessable", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, BootError::UnprocessableEntity(message) if message == "math shape is invalid")
    );
    server.abort();
}

#[cfg(feature = "grpc-transport")]
#[test]
fn grpc_transport_builds_a_message_client_with_custom_options() {
    let addr = std::net::SocketAddr::from(([127, 0, 0, 1], 4001));
    let options = GrpcTransportOptions::new()
        .with_request_timeout(Duration::from_secs(2))
        .with_connect_timeout(Duration::from_secs(2))
        .with_max_message_size(2 * 1024 * 1024);
    let app = BootApplication::builder().build().unwrap();
    let client = GrpcTransport::with_options(addr, options)
        .build(app)
        .unwrap();

    assert_eq!(client.endpoint(), "http://127.0.0.1:4001");
    assert_eq!(client.options(), options);
    assert_eq!(client.options().request_timeout(), Duration::from_secs(2));
    assert_eq!(client.options().connect_timeout(), Duration::from_secs(2));
    assert_eq!(
        client.options().max_decoding_message_size(),
        2 * 1024 * 1024
    );
    assert_eq!(
        client.options().max_encoding_message_size(),
        2 * 1024 * 1024
    );
}

#[cfg(feature = "grpc-transport")]
#[tokio::test]
async fn grpc_transport_round_trips_request_response_messages() {
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
    let addr = unused_tcp_addr().await;
    let transport = GrpcTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = GrpcTransportClient::for_addr(addr);

    let reply = send_grpc_with_retry(
        &client,
        TransportMessage::json("math.double", &NumberPayload { value: 12 }).unwrap(),
    )
    .await
    .unwrap()
    .unwrap();

    assert_eq!(reply.data_as::<NumberPayload>().unwrap().value, 24);
    server.abort();
}

#[cfg(feature = "grpc-transport")]
#[tokio::test]
async fn grpc_transport_emits_event_messages() {
    let observed = Arc::new(Mutex::new(Vec::new()));
    let event_log = Arc::clone(&observed);
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::event_json("math.observed", move |payload: NumberPayload| {
                let event_log = Arc::clone(&event_log);
                async move {
                    event_log.lock().unwrap().push(payload.value);
                    Ok(())
                }
            })
            .unwrap(),
        )
        .build()
        .unwrap();
    let addr = unused_tcp_addr().await;
    let transport = GrpcTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = GrpcTransportClient::for_addr(addr);

    emit_grpc_with_retry(
        &client,
        TransportMessage::json("math.observed", &NumberPayload { value: 7 }).unwrap(),
    )
    .await
    .unwrap();

    assert_eq!(observed.lock().unwrap().as_slice(), [7]);
    server.abort();
}

#[cfg(feature = "grpc-transport")]
#[tokio::test]
async fn grpc_transport_maps_handler_errors_to_client_errors() {
    let app = BootApplication::builder()
        .message_pattern(
            MessagePatternDefinition::request("math.bad", |_message: TransportMessage| async {
                Err::<TransportReply, BootError>(BootError::BadRequest("invalid math".to_string()))
            })
            .unwrap(),
        )
        .message_pattern(
            MessagePatternDefinition::request(
                "math.conflict",
                |_message: TransportMessage| async {
                    Err::<TransportReply, BootError>(BootError::conflict("duplicate math"))
                },
            )
            .unwrap(),
        )
        .message_pattern(
            MessagePatternDefinition::request(
                "math.unprocessable",
                |_message: TransportMessage| async {
                    Err::<TransportReply, BootError>(BootError::unprocessable_entity(
                        "math shape is invalid",
                    ))
                },
            )
            .unwrap(),
        )
        .build()
        .unwrap();
    let addr = unused_tcp_addr().await;
    let transport = GrpcTransport::new(addr);
    let server = tokio::spawn({
        let app = app.clone();
        async move { transport.serve(app).await }
    });
    let client = GrpcTransportClient::for_addr(addr);

    let error = send_grpc_with_retry(
        &client,
        TransportMessage::new("math.bad", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(message) if message == "invalid math"));

    let error = send_grpc_with_retry(
        &client,
        TransportMessage::new("math.conflict", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, BootError::Conflict(message) if message == "duplicate math"));

    let error = send_grpc_with_retry(
        &client,
        TransportMessage::new("math.unprocessable", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(
        matches!(error, BootError::UnprocessableEntity(message) if message == "math shape is invalid")
    );
    server.abort();
}

#[derive(Debug, Deserialize, Serialize)]
struct NumberPayload {
    value: i32,
}

#[derive(Debug, Deserialize, Serialize)]
struct CreateTransportCat {
    name: String,
    #[serde(default = "default_transport_cat_kind")]
    kind: String,
}

impl Validate for CreateTransportCat {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

impl ValidationSchema for CreateTransportCat {
    fn allowed_fields() -> &'static [&'static str] {
        &["kind", "name"]
    }
}

fn default_transport_cat_kind() -> String {
    "cat".to_string()
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

#[cfg(any(feature = "grpc-transport", feature = "tcp-transport"))]
async fn unused_tcp_addr() -> std::net::SocketAddr {
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();
    drop(listener);
    addr
}

#[cfg(feature = "tcp-transport")]
async fn send_tcp_with_retry(
    client: &TcpTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_refused = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Io(error)) if error.kind() == std::io::ErrorKind::ConnectionRefused => {
                last_refused = Some(error);
                sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_refused.map_or_else(
        || BootError::Adapter("tcp transport did not start".to_string()),
        BootError::Io,
    ))
}

#[cfg(feature = "grpc-transport")]
async fn send_grpc_with_retry(
    client: &GrpcTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message)) if should_retry_grpc_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("grpc transport did not start".to_string())))
}

#[cfg(feature = "grpc-transport")]
async fn emit_grpc_with_retry(
    client: &GrpcTransportClient,
    message: TransportMessage,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => return Ok(()),
            Err(BootError::Adapter(message)) if should_retry_grpc_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(10)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("grpc transport did not start".to_string())))
}

#[cfg(feature = "grpc-transport")]
fn should_retry_grpc_error(message: &str) -> bool {
    message.contains("transport error")
        || message.contains("error trying to connect")
        || message.contains("Connection refused")
        || message.contains("connection refused")
        || message.contains("tcp connect error")
        || message.contains("timed out")
}

#[cfg(feature = "kafka-transport")]
fn kafka_test_brokers() -> Option<Vec<String>> {
    std::env::var("A3S_BOOT_KAFKA_BROKERS")
        .ok()
        .map(|value| {
            value
                .split(',')
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(ToOwned::to_owned)
                .collect::<Vec<_>>()
        })
        .filter(|brokers| !brokers.is_empty())
}

#[cfg(feature = "kafka-transport")]
async fn send_kafka_with_retry(
    client: &KafkaTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message)) if should_retry_kafka_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("kafka transport did not start".to_string())))
}

#[cfg(feature = "kafka-transport")]
async fn emit_kafka_until_observed(
    client: &KafkaTransportClient,
    message: TransportMessage,
    observed: Arc<Mutex<Vec<i32>>>,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => {
                sleep(Duration::from_millis(50)).await;
                if !observed.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Err(BootError::Adapter(message)) if should_retry_kafka_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(50)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        BootError::Adapter("kafka transport event was not observed".to_string())
    }))
}

#[cfg(feature = "kafka-transport")]
fn should_retry_kafka_error(message: &str) -> bool {
    message.contains("UnknownTopicOrPartition")
        || message.contains("LeaderNotAvailable")
        || message.contains("NotLeaderOrFollower")
        || message.contains("Connection")
        || message.contains("connection")
        || message.contains("Timeout")
        || message.contains("timed out")
        || message.contains("reply topic closed")
}

#[cfg(feature = "mqtt-transport")]
fn mqtt_test_endpoint() -> Option<(String, u16)> {
    if let Ok(value) = std::env::var("A3S_BOOT_MQTT_URL") {
        let value = value.trim();
        if !value.is_empty() {
            return parse_mqtt_endpoint(value);
        }
    }

    let host = std::env::var("A3S_BOOT_MQTT_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())?;
    let port = std::env::var("A3S_BOOT_MQTT_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(1883);
    Some((host, port))
}

#[cfg(feature = "mqtt-transport")]
fn parse_mqtt_endpoint(value: &str) -> Option<(String, u16)> {
    let without_scheme = value
        .strip_prefix("mqtt://")
        .or_else(|| value.strip_prefix("tcp://"))
        .unwrap_or(value);
    let authority = without_scheme.split('/').next().unwrap_or(without_scheme);
    if authority.trim().is_empty() {
        return None;
    }

    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => {
            (host.to_string(), port.parse::<u16>().unwrap_or(1883))
        }
        _ => (authority.to_string(), 1883),
    };
    Some((host, port))
}

#[cfg(feature = "mqtt-transport")]
async fn send_mqtt_with_retry(
    client: &MqttTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message))
                if message.contains("timed out")
                    || message.contains("Connection refused")
                    || message.contains("connection refused") =>
            {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("mqtt transport did not start".to_string())))
}

#[cfg(feature = "mqtt-transport")]
async fn emit_mqtt_until_observed(
    client: &MqttTransportClient,
    message: TransportMessage,
    observed: Arc<Mutex<Vec<i32>>>,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => {
                sleep(Duration::from_millis(20)).await;
                if !observed.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(20)).await;
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("mqtt transport event was not observed".to_string())))
}

#[cfg(feature = "nats-transport")]
fn nats_test_url() -> Option<String> {
    std::env::var("A3S_BOOT_NATS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(feature = "rabbitmq-transport")]
fn rabbitmq_test_uri() -> Option<String> {
    std::env::var("A3S_BOOT_RABBITMQ_URI")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(feature = "nats-transport")]
async fn send_nats_with_retry(
    client: &NatsTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message))
                if message.contains("no responders") || message.contains("timed out") =>
            {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("nats transport did not start".to_string())))
}

#[cfg(feature = "nats-transport")]
async fn emit_nats_until_observed(
    client: &NatsTransportClient,
    message: TransportMessage,
    observed: Arc<Mutex<Vec<i32>>>,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => {
                sleep(Duration::from_millis(20)).await;
                if !observed.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(20)).await;
            }
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("nats transport event was not observed".to_string())))
}

#[cfg(feature = "rabbitmq-transport")]
async fn send_rabbitmq_with_retry(
    client: &RabbitMqTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message)) if should_retry_rabbitmq_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("rabbitmq transport did not start".to_string())))
}

#[cfg(feature = "rabbitmq-transport")]
async fn emit_rabbitmq_until_observed(
    client: &RabbitMqTransportClient,
    message: TransportMessage,
    observed: Arc<Mutex<Vec<i32>>>,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => {
                sleep(Duration::from_millis(20)).await;
                if !observed.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Err(BootError::Adapter(message)) if should_retry_rabbitmq_error(&message) => {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error.unwrap_or_else(|| {
        BootError::Adapter("rabbitmq transport event was not observed".to_string())
    }))
}

#[cfg(feature = "rabbitmq-transport")]
fn should_retry_rabbitmq_error(message: &str) -> bool {
    message.contains("refused")
        || message.contains("Refused")
        || message.contains("timed out")
        || message.contains("reply queue closed")
        || message.contains("connection")
        || message.contains("Connection")
}

#[cfg(feature = "redis-transport")]
fn redis_test_url() -> Option<String> {
    std::env::var("A3S_BOOT_REDIS_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[cfg(feature = "redis-transport")]
async fn send_redis_with_retry(
    client: &RedisTransportClient,
    message: TransportMessage,
) -> Result<Option<TransportReply>> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.send(message.clone()).await {
            Ok(reply) => return Ok(reply),
            Err(BootError::Adapter(message))
                if message.contains("has no subscribers") || message.contains("timed out") =>
            {
                last_error = Some(BootError::Adapter(message));
                sleep(Duration::from_millis(20)).await;
            }
            Err(error) => return Err(error),
        }
    }

    Err(last_error
        .unwrap_or_else(|| BootError::Adapter("redis transport did not start".to_string())))
}

#[cfg(feature = "redis-transport")]
async fn emit_redis_until_observed(
    client: &RedisTransportClient,
    message: TransportMessage,
    observed: Arc<Mutex<Vec<i32>>>,
) -> Result<()> {
    let mut last_error = None;
    for _ in 0..50 {
        match client.emit(message.clone()).await {
            Ok(()) => {
                sleep(Duration::from_millis(20)).await;
                if !observed.lock().unwrap().is_empty() {
                    return Ok(());
                }
            }
            Err(error) => {
                last_error = Some(error);
                sleep(Duration::from_millis(20)).await;
            }
        }
    }

    Err(last_error.unwrap_or_else(|| {
        BootError::Adapter("redis transport event was not observed".to_string())
    }))
}

#[cfg(any(
    feature = "kafka-transport",
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "rabbitmq-transport",
    feature = "redis-transport"
))]
fn unique_transport_prefix(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("a3s.boot.tests.{name}.{nanos}")
}
