use super::{transport_error_from_status, MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use ::redis::AsyncCommands;
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_REDIS_REQUEST_ID: AtomicU64 = AtomicU64::new(1);

/// Options for the Redis Pub/Sub message transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisTransportOptions {
    request_channel: String,
    event_channel: String,
    reply_channel_prefix: String,
    request_timeout: Duration,
}

impl RedisTransportOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_channel(&self) -> &str {
        &self.request_channel
    }

    pub fn event_channel(&self) -> &str {
        &self.event_channel
    }

    pub fn reply_channel_prefix(&self) -> &str {
        &self.reply_channel_prefix
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn with_channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.request_channel = format!("{prefix}.requests");
        self.event_channel = format!("{prefix}.events");
        self.reply_channel_prefix = format!("{prefix}.replies");
        self
    }

    pub fn with_request_channel(mut self, channel: impl Into<String>) -> Self {
        self.request_channel = channel.into();
        self
    }

    pub fn with_event_channel(mut self, channel: impl Into<String>) -> Self {
        self.event_channel = channel.into();
        self
    }

    pub fn with_reply_channel_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.reply_channel_prefix = prefix.into();
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout.max(Duration::from_millis(1));
        self
    }
}

impl Default for RedisTransportOptions {
    fn default() -> Self {
        Self {
            request_channel: "a3s.boot.requests".to_string(),
            event_channel: "a3s.boot.events".to_string(),
            reply_channel_prefix: "a3s.boot.replies".to_string(),
            request_timeout: Duration::from_secs(5),
        }
    }
}

/// Redis Pub/Sub transport for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisTransport {
    url: String,
    options: RedisTransportOptions,
}

impl RedisTransport {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            options: RedisTransportOptions::default(),
        }
    }

    pub fn with_options(url: impl Into<String>, options: RedisTransportOptions) -> Self {
        Self {
            url: url.into(),
            options,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn options(&self) -> &RedisTransportOptions {
        &self.options
    }
}

impl MessageTransport for RedisTransport {
    type Output = RedisTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(RedisTransportClient {
            url: self.url.clone(),
            options: self.options.clone(),
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let url = self.url.clone();
        let options = self.options.clone();
        Box::pin(async move {
            let client = redis_client(&url)?;
            let mut pubsub = client.get_async_pubsub().await.map_err(redis_error)?;
            pubsub
                .subscribe(options.request_channel.as_str())
                .await
                .map_err(redis_error)?;
            pubsub
                .subscribe(options.event_channel.as_str())
                .await
                .map_err(redis_error)?;
            let publisher = client
                .get_multiplexed_async_connection()
                .await
                .map_err(redis_error)?;
            let mut messages = pubsub.on_message();

            while let Some(message) = messages.next().await {
                let channel = message.get_channel_name().to_string();
                let payload = message.get_payload_bytes().to_vec();
                if channel == options.request_channel {
                    let envelope = match decode_request(&payload) {
                        Ok(envelope) => envelope,
                        Err(_) => continue,
                    };
                    let app = app.clone();
                    let mut publisher = publisher.clone();
                    tokio::spawn(async move {
                        let response = RedisResponseEnvelope::from_result(
                            &envelope.id,
                            app.dispatch_message(envelope.message).await,
                        );
                        let _ =
                            publish_response(&mut publisher, &envelope.reply_to, &response).await;
                    });
                } else if channel == options.event_channel {
                    let Ok(message) = decode_event(&payload) else {
                        continue;
                    };
                    let app = app.clone();
                    tokio::spawn(async move {
                        let _ = app.emit_message(message).await;
                    });
                }
            }

            Ok(())
        })
    }
}

/// Redis Pub/Sub client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedisTransportClient {
    url: String,
    options: RedisTransportOptions,
}

impl RedisTransportClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            options: RedisTransportOptions::default(),
        }
    }

    pub fn with_options(url: impl Into<String>, options: RedisTransportOptions) -> Self {
        Self {
            url: url.into(),
            options,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn options(&self) -> &RedisTransportOptions {
        &self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let client = redis_client(&self.url)?;
        let mut pubsub = client.get_async_pubsub().await.map_err(redis_error)?;
        let request_id = next_request_id();
        let reply_to = self.reply_channel(&request_id);
        pubsub
            .subscribe(reply_to.as_str())
            .await
            .map_err(redis_error)?;
        let envelope = RedisRequestEnvelope {
            id: request_id.clone(),
            reply_to: reply_to.clone(),
            message,
        };
        let payload = encode(&envelope)?;

        let mut publisher = client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        let subscriber_count: usize = publisher
            .publish(self.options.request_channel.as_str(), payload)
            .await
            .map_err(redis_error)?;
        if subscriber_count == 0 {
            return Err(BootError::Adapter(format!(
                "redis transport request channel has no subscribers: {}",
                self.options.request_channel
            )));
        }

        let mut messages = pubsub.on_message();
        let response = tokio::time::timeout(self.options.request_timeout, async {
            while let Some(message) = messages.next().await {
                let response = decode_response(message.get_payload_bytes())?;
                if response.id() == request_id {
                    return Ok(response);
                }
            }
            Err(BootError::Adapter(
                "redis transport reply channel closed".to_string(),
            ))
        })
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "redis transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })??;

        response.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let client = redis_client(&self.url)?;
        let mut publisher = client
            .get_multiplexed_async_connection()
            .await
            .map_err(redis_error)?;
        let payload = encode(&message)?;
        let _: usize = publisher
            .publish(self.options.event_channel.as_str(), payload)
            .await
            .map_err(redis_error)?;
        Ok(())
    }

    fn reply_channel(&self, request_id: &str) -> String {
        format!("{}.{}", self.options.reply_channel_prefix, request_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct RedisRequestEnvelope {
    id: String,
    reply_to: String,
    message: TransportMessage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum RedisResponseEnvelope {
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

impl RedisResponseEnvelope {
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

async fn publish_response(
    publisher: &mut ::redis::aio::MultiplexedConnection,
    channel: &str,
    response: &RedisResponseEnvelope,
) -> Result<()> {
    let payload = encode(response)?;
    let _: usize = publisher
        .publish(channel, payload)
        .await
        .map_err(redis_error)?;
    Ok(())
}

fn redis_client(url: &str) -> Result<::redis::Client> {
    ::redis::Client::open(url).map_err(redis_error)
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_request(payload: &[u8]) -> Result<RedisRequestEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_event(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(payload: &[u8]) -> Result<RedisResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::Adapter(err.to_string()))
}

fn redis_error(error: ::redis::RedisError) -> BootError {
    BootError::Adapter(error.to_string())
}

fn next_request_id() -> String {
    let counter = NEXT_REDIS_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}-{counter}", std::process::id())
}
