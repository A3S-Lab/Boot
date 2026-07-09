use super::{transport_error_from_status, MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use rumqttc::{AsyncClient, Event, EventLoop, Incoming, MqttOptions, Outgoing, QoS};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_MQTT_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_MQTT_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

/// MQTT quality of service used by the Boot MQTT transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MqttTransportQoS {
    AtMostOnce,
    AtLeastOnce,
    ExactlyOnce,
}

impl MqttTransportQoS {
    fn into_rumqttc(self) -> QoS {
        match self {
            Self::AtMostOnce => QoS::AtMostOnce,
            Self::AtLeastOnce => QoS::AtLeastOnce,
            Self::ExactlyOnce => QoS::ExactlyOnce,
        }
    }
}

/// Options for the MQTT message transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttTransportOptions {
    request_topic: String,
    event_topic: String,
    reply_topic_prefix: String,
    client_id_prefix: String,
    request_timeout: Duration,
    keep_alive: Duration,
    channel_capacity: usize,
    max_packet_size: usize,
    qos: MqttTransportQoS,
    retain: bool,
    clean_session: bool,
    credentials: Option<(String, String)>,
}

impl MqttTransportOptions {
    pub const DEFAULT_MAX_PACKET_SIZE: usize = 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_topic(&self) -> &str {
        &self.request_topic
    }

    pub fn event_topic(&self) -> &str {
        &self.event_topic
    }

    pub fn reply_topic_prefix(&self) -> &str {
        &self.reply_topic_prefix
    }

    pub fn client_id_prefix(&self) -> &str {
        &self.client_id_prefix
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn keep_alive(&self) -> Duration {
        self.keep_alive
    }

    pub fn channel_capacity(&self) -> usize {
        self.channel_capacity
    }

    pub fn max_packet_size(&self) -> usize {
        self.max_packet_size
    }

    pub fn qos(&self) -> MqttTransportQoS {
        self.qos
    }

    pub fn retain(&self) -> bool {
        self.retain
    }

    pub fn clean_session(&self) -> bool {
        self.clean_session
    }

    pub fn credentials(&self) -> Option<(&str, &str)> {
        self.credentials
            .as_ref()
            .map(|(username, password)| (username.as_str(), password.as_str()))
    }

    pub fn with_topic_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = trim_mqtt_topic(prefix.into());
        self.request_topic = format!("{prefix}/requests");
        self.event_topic = format!("{prefix}/events");
        self.reply_topic_prefix = format!("{prefix}/replies");
        self
    }

    pub fn with_request_topic(mut self, topic: impl Into<String>) -> Self {
        self.request_topic = topic.into();
        self
    }

    pub fn with_event_topic(mut self, topic: impl Into<String>) -> Self {
        self.event_topic = topic.into();
        self
    }

    pub fn with_reply_topic_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.reply_topic_prefix = trim_mqtt_topic(prefix.into());
        self
    }

    pub fn with_client_id_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.client_id_prefix = prefix.into();
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    pub fn with_keep_alive(mut self, keep_alive: Duration) -> Self {
        self.keep_alive = if keep_alive.is_zero() {
            keep_alive
        } else {
            keep_alive.max(Duration::from_secs(1))
        };
        self
    }

    pub fn with_channel_capacity(mut self, capacity: usize) -> Self {
        self.channel_capacity = capacity.max(1);
        self
    }

    pub fn with_max_packet_size(mut self, max_packet_size: usize) -> Self {
        self.max_packet_size = max_packet_size.max(1);
        self
    }

    pub fn with_qos(mut self, qos: MqttTransportQoS) -> Self {
        self.qos = qos;
        self
    }

    pub fn with_retain(mut self, retain: bool) -> Self {
        self.retain = retain;
        self
    }

    pub fn with_clean_session(mut self, clean_session: bool) -> Self {
        self.clean_session = clean_session;
        self
    }

    pub fn with_credentials(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
    ) -> Self {
        self.credentials = Some((username.into(), password.into()));
        self
    }

    pub fn without_credentials(mut self) -> Self {
        self.credentials = None;
        self
    }
}

impl Default for MqttTransportOptions {
    fn default() -> Self {
        Self {
            request_topic: "a3s/boot/requests".to_string(),
            event_topic: "a3s/boot/events".to_string(),
            reply_topic_prefix: "a3s/boot/replies".to_string(),
            client_id_prefix: "a3s-boot".to_string(),
            request_timeout: Duration::from_secs(5),
            keep_alive: Duration::from_secs(30),
            channel_capacity: 10,
            max_packet_size: Self::DEFAULT_MAX_PACKET_SIZE,
            qos: MqttTransportQoS::AtLeastOnce,
            retain: false,
            clean_session: true,
            credentials: None,
        }
    }
}

/// MQTT transport for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttTransport {
    host: String,
    port: u16,
    options: MqttTransportOptions,
}

impl MqttTransport {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            options: MqttTransportOptions::default(),
        }
    }

    pub fn with_options(host: impl Into<String>, port: u16, options: MqttTransportOptions) -> Self {
        Self {
            host: host.into(),
            port,
            options,
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn options(&self) -> &MqttTransportOptions {
        &self.options
    }
}

impl MessageTransport for MqttTransport {
    type Output = MqttTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(MqttTransportClient {
            host: self.host.clone(),
            port: self.port,
            options: self.options.clone(),
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let host = self.host.clone();
        let port = self.port;
        let options = self.options.clone();
        Box::pin(async move {
            let (client, mut eventloop) = mqtt_client(&host, port, &options, "server");
            client
                .subscribe(options.request_topic.clone(), options.qos.into_rumqttc())
                .await
                .map_err(mqtt_error)?;
            client
                .subscribe(options.event_topic.clone(), options.qos.into_rumqttc())
                .await
                .map_err(mqtt_error)?;

            loop {
                match eventloop.poll().await.map_err(mqtt_error)? {
                    Event::Incoming(Incoming::Publish(publish)) => {
                        if publish.topic == options.request_topic {
                            let app = app.clone();
                            let client = client.clone();
                            let qos = options.qos;
                            tokio::spawn(async move {
                                let _ = handle_request_publish(
                                    app,
                                    client,
                                    publish.payload.to_vec(),
                                    qos,
                                )
                                .await;
                            });
                        } else if publish.topic == options.event_topic {
                            let Ok(message) = decode_event(&publish.payload) else {
                                continue;
                            };
                            let app = app.clone();
                            tokio::spawn(async move {
                                let _ = app.emit_message(message).await;
                            });
                        }
                    }
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        })
    }
}

/// MQTT client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MqttTransportClient {
    host: String,
    port: u16,
    options: MqttTransportOptions,
}

impl MqttTransportClient {
    pub fn new(host: impl Into<String>, port: u16) -> Self {
        Self {
            host: host.into(),
            port,
            options: MqttTransportOptions::default(),
        }
    }

    pub fn with_options(host: impl Into<String>, port: u16, options: MqttTransportOptions) -> Self {
        Self {
            host: host.into(),
            port,
            options,
        }
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn options(&self) -> &MqttTransportOptions {
        &self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let (client, mut eventloop) =
            mqtt_client(self.host.as_str(), self.port, &self.options, "client");
        let request_id = next_request_id();
        let reply_to = self.reply_topic(&request_id);
        let envelope = MqttRequestEnvelope {
            id: request_id.clone(),
            reply_to: reply_to.clone(),
            message,
        };

        client
            .subscribe(reply_to.clone(), self.options.qos.into_rumqttc())
            .await
            .map_err(mqtt_error)?;
        client
            .publish(
                self.options.request_topic.clone(),
                self.options.qos.into_rumqttc(),
                self.options.retain,
                encode(&envelope)?,
            )
            .await
            .map_err(mqtt_error)?;

        let response = tokio::time::timeout(self.options.request_timeout, async {
            loop {
                match eventloop.poll().await.map_err(mqtt_error)? {
                    Event::Incoming(Incoming::Publish(publish)) if publish.topic == reply_to => {
                        let response = decode_response(&publish.payload)?;
                        if response.id() == request_id {
                            return Ok::<MqttResponseEnvelope, BootError>(response);
                        }
                    }
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        })
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "mqtt transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })??;

        let _ = client.disconnect().await;
        response.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let (client, mut eventloop) =
            mqtt_client(self.host.as_str(), self.port, &self.options, "client");
        client
            .publish(
                self.options.event_topic.clone(),
                self.options.qos.into_rumqttc(),
                self.options.retain,
                encode(&message)?,
            )
            .await
            .map_err(mqtt_error)?;

        tokio::time::timeout(self.options.request_timeout, async {
            loop {
                match eventloop.poll().await.map_err(mqtt_error)? {
                    Event::Outgoing(Outgoing::Publish(_)) => return Ok::<(), BootError>(()),
                    Event::Incoming(_) | Event::Outgoing(_) => {}
                }
            }
        })
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "mqtt transport publish timed out after {:?}",
                self.options.request_timeout
            ))
        })??;

        let _ = client.disconnect().await;
        Ok(())
    }

    fn reply_topic(&self, request_id: &str) -> String {
        format!("{}/{}", self.options.reply_topic_prefix, request_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct MqttRequestEnvelope {
    id: String,
    reply_to: String,
    message: TransportMessage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum MqttResponseEnvelope {
    Reply {
        id: String,
        data: Value,
    },
    NoReply {
        id: String,
    },
    Error {
        id: String,
        status: u16,
        message: String,
    },
}

impl MqttResponseEnvelope {
    fn from_result(id: &str, result: Result<Option<TransportReply>>) -> Self {
        match result {
            Ok(Some(reply)) => Self::Reply {
                id: id.to_string(),
                data: reply.data,
            },
            Ok(None) => Self::NoReply { id: id.to_string() },
            Err(error) => Self::from_error(id, error),
        }
    }

    fn from_error(id: &str, error: BootError) -> Self {
        Self::Error {
            id: id.to_string(),
            status: error.http_status_code(),
            message: error.http_response_message(),
        }
    }

    fn id(&self) -> &str {
        match self {
            Self::Reply { id, .. } | Self::NoReply { id } | Self::Error { id, .. } => id,
        }
    }

    fn into_result(self) -> Result<Option<TransportReply>> {
        match self {
            Self::Reply { data, .. } => Ok(Some(TransportReply::new(data))),
            Self::NoReply { .. } => Ok(None),
            Self::Error {
                status, message, ..
            } => Err(transport_error_from_status(status, message)),
        }
    }
}

async fn handle_request_publish(
    app: BootApplication,
    client: AsyncClient,
    payload: Vec<u8>,
    qos: MqttTransportQoS,
) -> Result<()> {
    let envelope = match decode_request(&payload) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(()),
    };
    let response = MqttResponseEnvelope::from_result(
        &envelope.id,
        app.dispatch_message(envelope.message).await,
    );
    client
        .publish(
            envelope.reply_to,
            qos.into_rumqttc(),
            false,
            encode(&response)?,
        )
        .await
        .map_err(mqtt_error)?;
    Ok(())
}

fn mqtt_client(
    host: &str,
    port: u16,
    options: &MqttTransportOptions,
    role: &str,
) -> (AsyncClient, EventLoop) {
    let client_id = next_client_id(options.client_id_prefix.as_str(), role);
    let mut mqtt_options = MqttOptions::new(client_id, host, port);
    mqtt_options.set_keep_alive(options.keep_alive);
    mqtt_options.set_clean_session(options.clean_session);
    mqtt_options.set_request_channel_capacity(options.channel_capacity);
    mqtt_options.set_max_packet_size(options.max_packet_size, options.max_packet_size);
    if let Some((username, password)) = &options.credentials {
        mqtt_options.set_credentials(username.clone(), password.clone());
    }

    AsyncClient::new(mqtt_options, options.channel_capacity)
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_request(payload: &[u8]) -> Result<MqttRequestEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_event(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(payload: &[u8]) -> Result<MqttResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::Adapter(err.to_string()))
}

fn mqtt_error(error: impl fmt::Display) -> BootError {
    BootError::Adapter(error.to_string())
}

fn next_request_id() -> String {
    let counter = NEXT_MQTT_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}-{counter}", std::process::id())
}

fn next_client_id(prefix: &str, role: &str) -> String {
    let counter = NEXT_MQTT_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{prefix}-{role}-{}-{nanos}-{counter}", std::process::id())
}

fn trim_mqtt_topic(value: String) -> String {
    value.trim_matches('/').to_string()
}
