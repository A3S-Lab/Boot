use a3s_boot::{
    BootApplication, BootError, BoxFuture, ExecutionContext, ExecutionInterceptor,
    ExecutionProtocol, ExecutionTransportKind, Guard, InProcessTransport, MessagePatternDefinition,
    MessageTransport, Module, ModuleRef, ProviderDefinition, Result, TransportContext,
    TransportInterceptor, TransportMessage, TransportReply, Validate,
};
#[cfg(feature = "mqtt-transport")]
use a3s_boot::{MqttTransport, MqttTransportClient, MqttTransportOptions, MqttTransportQoS};
#[cfg(feature = "nats-transport")]
use a3s_boot::{NatsTransport, NatsTransportClient, NatsTransportOptions};
#[cfg(feature = "redis-transport")]
use a3s_boot::{RedisTransport, RedisTransportClient, RedisTransportOptions};
#[cfg(feature = "tcp-transport")]
use a3s_boot::{TcpTransport, TcpTransportClient};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::{Arc, Mutex};
#[cfg(any(
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "redis-transport",
    feature = "tcp-transport"
))]
use std::time::Duration;
#[cfg(any(
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "redis-transport"
))]
use std::time::{SystemTime, UNIX_EPOCH};
#[cfg(feature = "tcp-transport")]
use tokio::net::TcpListener;
#[cfg(any(
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "redis-transport",
    feature = "tcp-transport"
))]
use tokio::time::sleep;

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
            MessagePatternDefinition::request("math.fail", |_message: TransportMessage| async {
                Err::<TransportReply, BootError>(BootError::BadRequest("invalid math".to_string()))
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

    let error = send_tcp_with_retry(
        &client,
        TransportMessage::new("math.fail", json!({ "value": 1 })),
    )
    .await
    .unwrap_err();

    assert!(matches!(error, BootError::BadRequest(message) if message == "invalid math"));
    server.abort();
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

#[cfg(feature = "tcp-transport")]
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
    feature = "mqtt-transport",
    feature = "nats-transport",
    feature = "redis-transport"
))]
fn unique_transport_prefix(name: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("a3s.boot.tests.{name}.{nanos}")
}
