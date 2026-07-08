use super::{MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fmt;
use std::time::Duration;

/// Options for the NATS message transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsTransportOptions {
    request_subject: String,
    event_subject: String,
    queue_group: Option<String>,
    request_timeout: Duration,
}

impl NatsTransportOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_subject(&self) -> &str {
        &self.request_subject
    }

    pub fn event_subject(&self) -> &str {
        &self.event_subject
    }

    pub fn queue_group(&self) -> Option<&str> {
        self.queue_group.as_deref()
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn with_subject_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.request_subject = format!("{prefix}.requests");
        self.event_subject = format!("{prefix}.events");
        self
    }

    pub fn with_request_subject(mut self, subject: impl Into<String>) -> Self {
        self.request_subject = subject.into();
        self
    }

    pub fn with_event_subject(mut self, subject: impl Into<String>) -> Self {
        self.event_subject = subject.into();
        self
    }

    pub fn with_queue_group(mut self, queue_group: impl Into<String>) -> Self {
        self.queue_group = Some(queue_group.into());
        self
    }

    pub fn without_queue_group(mut self) -> Self {
        self.queue_group = None;
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout.max(Duration::from_millis(1));
        self
    }
}

impl Default for NatsTransportOptions {
    fn default() -> Self {
        Self {
            request_subject: "a3s.boot.requests".to_string(),
            event_subject: "a3s.boot.events".to_string(),
            queue_group: None,
            request_timeout: Duration::from_secs(5),
        }
    }
}

/// NATS transport for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsTransport {
    url: String,
    options: NatsTransportOptions,
}

impl NatsTransport {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            options: NatsTransportOptions::default(),
        }
    }

    pub fn with_options(url: impl Into<String>, options: NatsTransportOptions) -> Self {
        Self {
            url: url.into(),
            options,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn options(&self) -> &NatsTransportOptions {
        &self.options
    }
}

impl MessageTransport for NatsTransport {
    type Output = NatsTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(NatsTransportClient {
            url: self.url.clone(),
            options: self.options.clone(),
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let url = self.url.clone();
        let options = self.options.clone();
        Box::pin(async move {
            let client = async_nats::connect(url).await.map_err(nats_error)?;
            let request_subscriber = subscribe(&client, options.request_subject.as_str(), &options)
                .await
                .map_err(nats_error)?;
            let event_subscriber = subscribe(&client, options.event_subject.as_str(), &options)
                .await
                .map_err(nats_error)?;

            let request_loop = serve_request_messages(
                app.clone(),
                client.clone(),
                request_subscriber,
                options.request_subject.clone(),
            );
            let event_loop =
                serve_event_messages(app, event_subscriber, options.event_subject.clone());

            futures_util::future::try_join(request_loop, event_loop).await?;
            Ok(())
        })
    }
}

/// NATS client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NatsTransportClient {
    url: String,
    options: NatsTransportOptions,
}

impl NatsTransportClient {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            options: NatsTransportOptions::default(),
        }
    }

    pub fn with_options(url: impl Into<String>, options: NatsTransportOptions) -> Self {
        Self {
            url: url.into(),
            options,
        }
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn options(&self) -> &NatsTransportOptions {
        &self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let client = async_nats::connect(self.url.as_str())
            .await
            .map_err(nats_error)?;
        let request_subject = self.options.request_subject.clone();
        let payload = encode(&message)?;
        let response = tokio::time::timeout(
            self.options.request_timeout,
            client.request(request_subject.clone(), payload.into()),
        )
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "nats transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })?
        .map_err(|error| request_error(error, request_subject.as_str()))?;

        decode_response(&response.payload)?.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let client = async_nats::connect(self.url.as_str())
            .await
            .map_err(nats_error)?;
        let event_subject = self.options.event_subject.clone();
        let payload = encode(&message)?;
        client
            .publish(event_subject, payload.into())
            .await
            .map_err(nats_error)?;
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum NatsResponseEnvelope {
    Reply { data: Value },
    NoReply,
    Error { status: u16, message: String },
}

impl NatsResponseEnvelope {
    fn from_result(result: Result<Option<TransportReply>>) -> Self {
        match result {
            Ok(Some(reply)) => Self::Reply { data: reply.data },
            Ok(None) => Self::NoReply,
            Err(error) => Self::from_error(error),
        }
    }

    fn from_error(error: BootError) -> Self {
        Self::Error {
            status: error.http_status_code(),
            message: error.http_response_message(),
        }
    }

    fn into_result(self) -> Result<Option<TransportReply>> {
        match self {
            Self::Reply { data } => Ok(Some(TransportReply::new(data))),
            Self::NoReply => Ok(None),
            Self::Error { status, message } => Err(error_from_status(status, message)),
        }
    }
}

async fn subscribe(
    client: &async_nats::Client,
    subject: &str,
    options: &NatsTransportOptions,
) -> std::result::Result<async_nats::Subscriber, async_nats::SubscribeError> {
    match options.queue_group() {
        Some(queue_group) => {
            client
                .queue_subscribe(subject.to_string(), queue_group.to_string())
                .await
        }
        None => client.subscribe(subject.to_string()).await,
    }
}

async fn serve_request_messages(
    app: BootApplication,
    client: async_nats::Client,
    mut subscriber: async_nats::Subscriber,
    subject: String,
) -> Result<()> {
    while let Some(message) = subscriber.next().await {
        let Some(reply_subject) = message.reply else {
            continue;
        };
        let app = app.clone();
        let client = client.clone();
        tokio::spawn(async move {
            let _ =
                handle_request_message(app, client, message.payload.to_vec(), reply_subject).await;
        });
    }

    Err(BootError::Adapter(format!(
        "nats transport request subscription closed: {subject}"
    )))
}

async fn handle_request_message(
    app: BootApplication,
    client: async_nats::Client,
    payload: Vec<u8>,
    reply_subject: async_nats::Subject,
) -> Result<()> {
    let response = match decode_message(&payload) {
        Ok(message) => NatsResponseEnvelope::from_result(app.dispatch_message(message).await),
        Err(error) => NatsResponseEnvelope::from_error(error),
    };
    let payload = encode(&response)?;
    client
        .publish(reply_subject, payload.into())
        .await
        .map_err(nats_error)?;
    Ok(())
}

async fn serve_event_messages(
    app: BootApplication,
    mut subscriber: async_nats::Subscriber,
    subject: String,
) -> Result<()> {
    while let Some(message) = subscriber.next().await {
        let payload = message.payload.to_vec();
        let Ok(message) = decode_message(&payload) else {
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _ = app.emit_message(message).await;
        });
    }

    Err(BootError::Adapter(format!(
        "nats transport event subscription closed: {subject}"
    )))
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_message(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(payload: &[u8]) -> Result<NatsResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::Adapter(err.to_string()))
}

fn request_error(error: async_nats::RequestError, subject: &str) -> BootError {
    match error.kind() {
        async_nats::RequestErrorKind::NoResponders => BootError::Adapter(format!(
            "nats transport request subject has no responders: {subject}"
        )),
        async_nats::RequestErrorKind::TimedOut => {
            BootError::Adapter(format!("nats transport request timed out: {subject}"))
        }
        _ => nats_error(error),
    }
}

fn nats_error(error: impl fmt::Display) -> BootError {
    BootError::Adapter(error.to_string())
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
