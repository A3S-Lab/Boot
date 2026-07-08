use super::{MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use futures_util::StreamExt;
use lapin::{
    options::{BasicAckOptions, BasicConsumeOptions, BasicPublishOptions, QueueDeclareOptions},
    types::FieldTable,
    BasicProperties, Channel, Connection, ConnectionProperties,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_RABBITMQ_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_RABBITMQ_CONSUMER_ID: AtomicU64 = AtomicU64::new(1);

/// Options for the RabbitMQ message transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqTransportOptions {
    request_queue: String,
    event_queue: String,
    reply_queue_prefix: String,
    consumer_tag_prefix: String,
    request_timeout: Duration,
    durable: bool,
    auto_delete: bool,
}

impl RabbitMqTransportOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_queue(&self) -> &str {
        &self.request_queue
    }

    pub fn event_queue(&self) -> &str {
        &self.event_queue
    }

    pub fn reply_queue_prefix(&self) -> &str {
        &self.reply_queue_prefix
    }

    pub fn consumer_tag_prefix(&self) -> &str {
        &self.consumer_tag_prefix
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn durable(&self) -> bool {
        self.durable
    }

    pub fn auto_delete(&self) -> bool {
        self.auto_delete
    }

    pub fn with_queue_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.request_queue = format!("{prefix}.requests");
        self.event_queue = format!("{prefix}.events");
        self.reply_queue_prefix = format!("{prefix}.replies");
        self.consumer_tag_prefix = format!("{prefix}.consumer");
        self
    }

    pub fn with_request_queue(mut self, queue: impl Into<String>) -> Self {
        self.request_queue = queue.into();
        self
    }

    pub fn with_event_queue(mut self, queue: impl Into<String>) -> Self {
        self.event_queue = queue.into();
        self
    }

    pub fn with_reply_queue_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.reply_queue_prefix = prefix.into();
        self
    }

    pub fn with_consumer_tag_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.consumer_tag_prefix = prefix.into();
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    pub fn with_durable(mut self, durable: bool) -> Self {
        self.durable = durable;
        self
    }

    pub fn with_auto_delete(mut self, auto_delete: bool) -> Self {
        self.auto_delete = auto_delete;
        self
    }
}

impl Default for RabbitMqTransportOptions {
    fn default() -> Self {
        Self {
            request_queue: "a3s.boot.requests".to_string(),
            event_queue: "a3s.boot.events".to_string(),
            reply_queue_prefix: "a3s.boot.replies".to_string(),
            consumer_tag_prefix: "a3s.boot.consumer".to_string(),
            request_timeout: Duration::from_secs(5),
            durable: false,
            auto_delete: false,
        }
    }
}

/// RabbitMQ transport for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqTransport {
    uri: String,
    options: RabbitMqTransportOptions,
}

impl RabbitMqTransport {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            options: RabbitMqTransportOptions::default(),
        }
    }

    pub fn with_options(uri: impl Into<String>, options: RabbitMqTransportOptions) -> Self {
        Self {
            uri: uri.into(),
            options,
        }
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn options(&self) -> &RabbitMqTransportOptions {
        &self.options
    }
}

impl MessageTransport for RabbitMqTransport {
    type Output = RabbitMqTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(RabbitMqTransportClient {
            uri: self.uri.clone(),
            options: self.options.clone(),
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let uri = self.uri.clone();
        let options = self.options.clone();
        Box::pin(async move {
            let connection = rabbitmq_connection(&uri).await?;
            let request_channel = connection.create_channel().await.map_err(rabbitmq_error)?;
            let event_channel = connection.create_channel().await.map_err(rabbitmq_error)?;
            let publish_channel = connection.create_channel().await.map_err(rabbitmq_error)?;

            declare_queue(&request_channel, options.request_queue.as_str(), &options).await?;
            declare_queue(&event_channel, options.event_queue.as_str(), &options).await?;

            let request_consumer = request_channel
                .basic_consume(
                    options.request_queue.clone().into(),
                    next_consumer_tag(options.consumer_tag_prefix.as_str(), "requests").into(),
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
                .map_err(rabbitmq_error)?;
            let event_consumer = event_channel
                .basic_consume(
                    options.event_queue.clone().into(),
                    next_consumer_tag(options.consumer_tag_prefix.as_str(), "events").into(),
                    BasicConsumeOptions::default(),
                    FieldTable::default(),
                )
                .await
                .map_err(rabbitmq_error)?;

            let request_loop =
                serve_request_deliveries(app.clone(), publish_channel, request_consumer);
            let event_loop = serve_event_deliveries(app, event_consumer);
            futures_util::future::try_join(request_loop, event_loop).await?;
            Ok(())
        })
    }
}

/// RabbitMQ client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RabbitMqTransportClient {
    uri: String,
    options: RabbitMqTransportOptions,
}

impl RabbitMqTransportClient {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            options: RabbitMqTransportOptions::default(),
        }
    }

    pub fn with_options(uri: impl Into<String>, options: RabbitMqTransportOptions) -> Self {
        Self {
            uri: uri.into(),
            options,
        }
    }

    pub fn uri(&self) -> &str {
        &self.uri
    }

    pub fn options(&self) -> &RabbitMqTransportOptions {
        &self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let connection = rabbitmq_connection(self.uri.as_str()).await?;
        let channel = connection.create_channel().await.map_err(rabbitmq_error)?;
        declare_queue(&channel, self.options.request_queue.as_str(), &self.options).await?;
        let request_id = next_request_id();
        let reply_to = self.reply_queue(&request_id);
        declare_reply_queue(&channel, reply_to.as_str()).await?;

        let mut consumer = channel
            .basic_consume(
                reply_to.clone().into(),
                next_consumer_tag(self.options.consumer_tag_prefix.as_str(), "reply").into(),
                BasicConsumeOptions::default(),
                FieldTable::default(),
            )
            .await
            .map_err(rabbitmq_error)?;
        let envelope = RabbitMqRequestEnvelope {
            id: request_id.clone(),
            reply_to: reply_to.clone(),
            message,
        };
        publish_to_queue(
            &channel,
            self.options.request_queue.as_str(),
            &encode(&envelope)?,
            BasicProperties::default(),
        )
        .await?;

        let response = tokio::time::timeout(self.options.request_timeout, async {
            while let Some(delivery) = consumer.next().await {
                let delivery = delivery.map_err(rabbitmq_error)?;
                let response = decode_response(&delivery.data)?;
                delivery
                    .ack(BasicAckOptions::default())
                    .await
                    .map_err(rabbitmq_error)?;
                if response.id() == request_id {
                    return Ok::<RabbitMqResponseEnvelope, BootError>(response);
                }
            }

            Err(BootError::Adapter(
                "rabbitmq transport reply queue closed".to_string(),
            ))
        })
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "rabbitmq transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })??;

        response.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let connection = rabbitmq_connection(self.uri.as_str()).await?;
        let channel = connection.create_channel().await.map_err(rabbitmq_error)?;
        declare_queue(&channel, self.options.event_queue.as_str(), &self.options).await?;
        publish_to_queue(
            &channel,
            self.options.event_queue.as_str(),
            &encode(&message)?,
            BasicProperties::default(),
        )
        .await
    }

    fn reply_queue(&self, request_id: &str) -> String {
        format!("{}.{}", self.options.reply_queue_prefix, request_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RabbitMqRequestEnvelope {
    id: String,
    reply_to: String,
    message: TransportMessage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RabbitMqResponseEnvelope {
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

impl RabbitMqResponseEnvelope {
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
            } => Err(error_from_status(status, message)),
        }
    }
}

async fn serve_request_deliveries(
    app: BootApplication,
    publish_channel: Channel,
    mut consumer: lapin::Consumer,
) -> Result<()> {
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.map_err(rabbitmq_error)?;
        let app = app.clone();
        let publish_channel = publish_channel.clone();
        tokio::spawn(async move {
            let _ = handle_request_delivery(app, publish_channel, delivery).await;
        });
    }

    Err(BootError::Adapter(
        "rabbitmq transport request consumer closed".to_string(),
    ))
}

async fn serve_event_deliveries(app: BootApplication, mut consumer: lapin::Consumer) -> Result<()> {
    while let Some(delivery) = consumer.next().await {
        let delivery = delivery.map_err(rabbitmq_error)?;
        let Ok(message) = decode_event(&delivery.data) else {
            delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(rabbitmq_error)?;
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _ = app.emit_message(message).await;
            let _ = delivery.ack(BasicAckOptions::default()).await;
        });
    }

    Err(BootError::Adapter(
        "rabbitmq transport event consumer closed".to_string(),
    ))
}

async fn handle_request_delivery(
    app: BootApplication,
    channel: Channel,
    delivery: lapin::message::Delivery,
) -> Result<()> {
    let envelope = match decode_request(&delivery.data) {
        Ok(envelope) => envelope,
        Err(_) => {
            delivery
                .ack(BasicAckOptions::default())
                .await
                .map_err(rabbitmq_error)?;
            return Ok(());
        }
    };
    let response = RabbitMqResponseEnvelope::from_result(
        &envelope.id,
        app.dispatch_message(envelope.message).await,
    );
    publish_to_queue(
        &channel,
        envelope.reply_to.as_str(),
        &encode(&response)?,
        BasicProperties::default(),
    )
    .await?;
    delivery
        .ack(BasicAckOptions::default())
        .await
        .map_err(rabbitmq_error)?;
    Ok(())
}

async fn rabbitmq_connection(uri: &str) -> Result<Connection> {
    Connection::connect(uri, ConnectionProperties::default())
        .await
        .map_err(rabbitmq_error)
}

async fn declare_queue(
    channel: &Channel,
    queue: &str,
    options: &RabbitMqTransportOptions,
) -> Result<()> {
    channel
        .queue_declare(
            queue.into(),
            QueueDeclareOptions {
                durable: options.durable,
                auto_delete: options.auto_delete,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await
        .map_err(rabbitmq_error)?;
    Ok(())
}

async fn declare_reply_queue(channel: &Channel, queue: &str) -> Result<()> {
    channel
        .queue_declare(
            queue.into(),
            QueueDeclareOptions {
                exclusive: true,
                auto_delete: true,
                ..QueueDeclareOptions::default()
            },
            FieldTable::default(),
        )
        .await
        .map_err(rabbitmq_error)?;
    Ok(())
}

async fn publish_to_queue(
    channel: &Channel,
    queue: &str,
    payload: &[u8],
    properties: BasicProperties,
) -> Result<()> {
    channel
        .basic_publish(
            "".into(),
            queue.into(),
            BasicPublishOptions::default(),
            payload,
            properties,
        )
        .await
        .map_err(rabbitmq_error)?
        .await
        .map_err(rabbitmq_error)?;
    Ok(())
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_request(payload: &[u8]) -> Result<RabbitMqRequestEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_event(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(payload: &[u8]) -> Result<RabbitMqResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::Adapter(err.to_string()))
}

fn rabbitmq_error(error: impl fmt::Display) -> BootError {
    BootError::Adapter(error.to_string())
}

fn next_request_id() -> String {
    let counter = NEXT_RABBITMQ_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}-{counter}", std::process::id())
}

fn next_consumer_tag(prefix: &str, role: &str) -> String {
    let counter = NEXT_RABBITMQ_CONSUMER_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{prefix}.{role}.{}.{nanos}.{counter}", std::process::id())
}

fn error_from_status(status: u16, message: String) -> BootError {
    match status {
        400 => BootError::BadRequest(message),
        401 => BootError::Unauthorized(message),
        403 => BootError::Forbidden(message),
        404 => BootError::NotFound(message),
        406 => BootError::NotAcceptable(message),
        413 => BootError::PayloadTooLarge(message),
        415 => BootError::UnsupportedMediaType(message),
        429 => BootError::TooManyRequests(message),
        500 => BootError::Internal(message),
        _ => BootError::Adapter(message),
    }
}
