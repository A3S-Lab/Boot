use super::{MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use chrono::Utc;
use futures_util::StreamExt;
use rskafka::{
    client::{
        error::{Error as KafkaClientError, ProtocolError},
        partition::{Compression, OffsetAt, PartitionClient, UnknownTopicHandling},
        Client, ClientBuilder,
    },
    record::Record,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

static NEXT_KAFKA_REQUEST_ID: AtomicU64 = AtomicU64::new(1);
static NEXT_KAFKA_CLIENT_ID: AtomicU64 = AtomicU64::new(1);

/// Options for the Kafka message transport.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaTransportOptions {
    request_topic: String,
    event_topic: String,
    reply_topic_prefix: String,
    client_id_prefix: String,
    request_timeout: Duration,
    partition: i32,
    fetch_min_batch_size: i32,
    fetch_max_batch_size: i32,
    fetch_max_wait_ms: i32,
    max_message_size: usize,
    auto_create_topics: bool,
    topic_replication_factor: i16,
}

impl KafkaTransportOptions {
    pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 100 * 1024 * 1024;
    pub const DEFAULT_FETCH_MAX_BATCH_SIZE: i32 = 50 * 1024 * 1024;

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

    pub fn partition(&self) -> i32 {
        self.partition
    }

    pub fn fetch_min_batch_size(&self) -> i32 {
        self.fetch_min_batch_size
    }

    pub fn fetch_max_batch_size(&self) -> i32 {
        self.fetch_max_batch_size
    }

    pub fn fetch_max_wait_ms(&self) -> i32 {
        self.fetch_max_wait_ms
    }

    pub fn max_message_size(&self) -> usize {
        self.max_message_size
    }

    pub fn auto_create_topics(&self) -> bool {
        self.auto_create_topics
    }

    pub fn topic_replication_factor(&self) -> i16 {
        self.topic_replication_factor
    }

    pub fn with_topic_prefix(mut self, prefix: impl Into<String>) -> Self {
        let prefix = prefix.into();
        self.request_topic = format!("{prefix}.requests");
        self.event_topic = format!("{prefix}.events");
        self.reply_topic_prefix = format!("{prefix}.replies");
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
        self.reply_topic_prefix = prefix.into();
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

    pub fn with_partition(mut self, partition: i32) -> Self {
        self.partition = partition.max(0);
        self
    }

    pub fn with_fetch_batch_size(mut self, min_batch_size: i32, max_batch_size: i32) -> Self {
        self.fetch_min_batch_size = min_batch_size.max(1);
        self.fetch_max_batch_size = max_batch_size.max(self.fetch_min_batch_size);
        self
    }

    pub fn with_fetch_max_wait_ms(mut self, max_wait_ms: i32) -> Self {
        self.fetch_max_wait_ms = max_wait_ms.max(1);
        self
    }

    pub fn with_max_message_size(mut self, max_message_size: usize) -> Self {
        self.max_message_size = max_message_size.max(1);
        self
    }

    pub fn with_auto_create_topics(mut self, auto_create_topics: bool) -> Self {
        self.auto_create_topics = auto_create_topics;
        self
    }

    pub fn with_topic_replication_factor(mut self, replication_factor: i16) -> Self {
        self.topic_replication_factor = replication_factor.max(1);
        self
    }
}

impl Default for KafkaTransportOptions {
    fn default() -> Self {
        Self {
            request_topic: "a3s.boot.requests".to_string(),
            event_topic: "a3s.boot.events".to_string(),
            reply_topic_prefix: "a3s.boot.replies".to_string(),
            client_id_prefix: "a3s-boot".to_string(),
            request_timeout: Duration::from_secs(5),
            partition: 0,
            fetch_min_batch_size: 1,
            fetch_max_batch_size: Self::DEFAULT_FETCH_MAX_BATCH_SIZE,
            fetch_max_wait_ms: 500,
            max_message_size: Self::DEFAULT_MAX_MESSAGE_SIZE,
            auto_create_topics: false,
            topic_replication_factor: 1,
        }
    }
}

/// Kafka transport for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaTransport {
    brokers: Vec<String>,
    options: KafkaTransportOptions,
}

impl KafkaTransport {
    pub fn new<I, S>(brokers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            brokers: collect_brokers(brokers),
            options: KafkaTransportOptions::default(),
        }
    }

    pub fn with_options<I, S>(brokers: I, options: KafkaTransportOptions) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            brokers: collect_brokers(brokers),
            options,
        }
    }

    pub fn brokers(&self) -> &[String] {
        &self.brokers
    }

    pub fn options(&self) -> &KafkaTransportOptions {
        &self.options
    }
}

impl MessageTransport for KafkaTransport {
    type Output = KafkaTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(KafkaTransportClient {
            brokers: self.brokers.clone(),
            options: self.options.clone(),
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let brokers = self.brokers.clone();
        let options = self.options.clone();
        Box::pin(async move {
            let client = Arc::new(kafka_client(&brokers, &options, "server").await?);
            ensure_topic(&client, options.request_topic.as_str(), &options).await?;
            ensure_topic(&client, options.event_topic.as_str(), &options).await?;

            let request_partition =
                partition_client(&client, options.request_topic.as_str(), &options).await?;
            let event_partition =
                partition_client(&client, options.event_topic.as_str(), &options).await?;

            let request_loop = serve_request_records(
                app.clone(),
                Arc::clone(&client),
                request_partition,
                options.clone(),
            );
            let event_loop = serve_event_records(app, event_partition, options);
            futures_util::future::try_join(request_loop, event_loop).await?;
            Ok(())
        })
    }
}

/// Kafka client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KafkaTransportClient {
    brokers: Vec<String>,
    options: KafkaTransportOptions,
}

impl KafkaTransportClient {
    pub fn new<I, S>(brokers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            brokers: collect_brokers(brokers),
            options: KafkaTransportOptions::default(),
        }
    }

    pub fn with_options<I, S>(brokers: I, options: KafkaTransportOptions) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        Self {
            brokers: collect_brokers(brokers),
            options,
        }
    }

    pub fn brokers(&self) -> &[String] {
        &self.brokers
    }

    pub fn options(&self) -> &KafkaTransportOptions {
        &self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let client = Arc::new(kafka_client(&self.brokers, &self.options, "client").await?);
        ensure_topic(&client, self.options.request_topic.as_str(), &self.options).await?;

        let request_id = next_request_id();
        let reply_topic = self.reply_topic(&request_id);
        ensure_topic(&client, reply_topic.as_str(), &self.options).await?;

        let request_partition =
            partition_client(&client, self.options.request_topic.as_str(), &self.options).await?;
        let reply_partition =
            partition_client(&client, reply_topic.as_str(), &self.options).await?;
        let reply_start_offset = reply_partition
            .get_offset(OffsetAt::Latest)
            .await
            .map_err(kafka_error)?;
        let mut reply_stream = kafka_stream(
            reply_partition,
            StartOffsetKind::At(reply_start_offset),
            &self.options,
        );

        let envelope = KafkaRequestEnvelope {
            id: request_id.clone(),
            reply_topic,
            message,
        };
        produce_payload(
            &request_partition,
            Some(request_id.as_bytes()),
            &encode(&envelope)?,
        )
        .await?;

        let response = tokio::time::timeout(self.options.request_timeout, async {
            while let Some(record) = reply_stream.next().await {
                let (record, _) = record.map_err(kafka_error)?;
                let Some(payload) = record.record.value.as_deref() else {
                    continue;
                };
                let response = decode_response(payload)?;
                if response.id() == request_id {
                    return Ok::<KafkaResponseEnvelope, BootError>(response);
                }
            }

            Err(BootError::Adapter(
                "kafka transport reply topic closed".to_string(),
            ))
        })
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "kafka transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })??;

        response.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let client = Arc::new(kafka_client(&self.brokers, &self.options, "client").await?);
        ensure_topic(&client, self.options.event_topic.as_str(), &self.options).await?;
        let event_partition =
            partition_client(&client, self.options.event_topic.as_str(), &self.options).await?;
        produce_payload(&event_partition, None, &encode(&message)?).await
    }

    fn reply_topic(&self, request_id: &str) -> String {
        format!("{}.{}", self.options.reply_topic_prefix, request_id)
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct KafkaRequestEnvelope {
    id: String,
    reply_topic: String,
    message: TransportMessage,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum KafkaResponseEnvelope {
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

impl KafkaResponseEnvelope {
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

enum StartOffsetKind {
    At(i64),
    Latest,
}

async fn serve_request_records(
    app: BootApplication,
    client: Arc<Client>,
    partition: Arc<PartitionClient>,
    options: KafkaTransportOptions,
) -> Result<()> {
    let mut stream = kafka_stream(partition, StartOffsetKind::Latest, &options);
    while let Some(record) = stream.next().await {
        let (record, _) = record.map_err(kafka_error)?;
        let Some(payload) = record.record.value else {
            continue;
        };
        let app = app.clone();
        let client = Arc::clone(&client);
        let options = options.clone();
        tokio::spawn(async move {
            let _ = handle_request_record(app, client, options, payload).await;
        });
    }

    Err(BootError::Adapter(
        "kafka transport request stream closed".to_string(),
    ))
}

async fn serve_event_records(
    app: BootApplication,
    partition: Arc<PartitionClient>,
    options: KafkaTransportOptions,
) -> Result<()> {
    let mut stream = kafka_stream(partition, StartOffsetKind::Latest, &options);
    while let Some(record) = stream.next().await {
        let (record, _) = record.map_err(kafka_error)?;
        let Some(payload) = record.record.value else {
            continue;
        };
        let Ok(message) = decode_event(&payload) else {
            continue;
        };
        let app = app.clone();
        tokio::spawn(async move {
            let _ = app.emit_message(message).await;
        });
    }

    Err(BootError::Adapter(
        "kafka transport event stream closed".to_string(),
    ))
}

async fn handle_request_record(
    app: BootApplication,
    client: Arc<Client>,
    options: KafkaTransportOptions,
    payload: Vec<u8>,
) -> Result<()> {
    let envelope = match decode_request(&payload) {
        Ok(envelope) => envelope,
        Err(_) => return Ok(()),
    };
    ensure_topic(&client, envelope.reply_topic.as_str(), &options).await?;
    let reply_partition =
        partition_client(&client, envelope.reply_topic.as_str(), &options).await?;
    let response = KafkaResponseEnvelope::from_result(
        &envelope.id,
        app.dispatch_message(envelope.message).await,
    );
    produce_payload(
        &reply_partition,
        Some(envelope.id.as_bytes()),
        &encode(&response)?,
    )
    .await
}

async fn kafka_client(
    brokers: &[String],
    options: &KafkaTransportOptions,
    role: &str,
) -> Result<Client> {
    if brokers.is_empty() {
        return Err(BootError::Adapter(
            "kafka transport requires at least one broker".to_string(),
        ));
    }

    ClientBuilder::new(brokers.to_vec())
        .client_id(next_client_id(options.client_id_prefix.as_str(), role))
        .max_message_size(options.max_message_size)
        .build()
        .await
        .map_err(kafka_error)
}

async fn ensure_topic(client: &Client, topic: &str, options: &KafkaTransportOptions) -> Result<()> {
    if !options.auto_create_topics {
        return Ok(());
    }

    let controller = client.controller_client().map_err(kafka_error)?;
    let partitions = options.partition.saturating_add(1);
    let timeout_ms = duration_millis_i32(options.request_timeout);
    match controller
        .create_topic(
            topic.to_string(),
            partitions,
            options.topic_replication_factor,
            timeout_ms,
        )
        .await
    {
        Ok(()) => Ok(()),
        Err(error) if is_topic_already_exists(&error) => Ok(()),
        Err(error) => Err(kafka_error(error)),
    }
}

async fn partition_client(
    client: &Client,
    topic: &str,
    options: &KafkaTransportOptions,
) -> Result<Arc<PartitionClient>> {
    let unknown_topic_handling = if options.auto_create_topics {
        UnknownTopicHandling::Retry
    } else {
        UnknownTopicHandling::Error
    };
    client
        .partition_client(topic.to_string(), options.partition, unknown_topic_handling)
        .await
        .map(Arc::new)
        .map_err(kafka_error)
}

fn kafka_stream(
    partition: Arc<PartitionClient>,
    start_offset: StartOffsetKind,
    options: &KafkaTransportOptions,
) -> rskafka::client::consumer::StreamConsumer {
    let start_offset = match start_offset {
        StartOffsetKind::At(offset) => rskafka::client::consumer::StartOffset::At(offset),
        StartOffsetKind::Latest => rskafka::client::consumer::StartOffset::Latest,
    };
    rskafka::client::consumer::StreamConsumerBuilder::new(partition, start_offset)
        .with_min_batch_size(options.fetch_min_batch_size)
        .with_max_batch_size(options.fetch_max_batch_size)
        .with_max_wait_ms(options.fetch_max_wait_ms)
        .build()
}

async fn produce_payload(
    partition: &PartitionClient,
    key: Option<&[u8]>,
    payload: &[u8],
) -> Result<()> {
    let record = Record {
        key: key.map(<[u8]>::to_vec),
        value: Some(payload.to_vec()),
        headers: BTreeMap::new(),
        timestamp: Utc::now(),
    };
    partition
        .produce(vec![record], Compression::NoCompression)
        .await
        .map_err(kafka_error)?;
    Ok(())
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_request(payload: &[u8]) -> Result<KafkaRequestEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_event(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(payload: &[u8]) -> Result<KafkaResponseEnvelope> {
    serde_json::from_slice(payload).map_err(|err| BootError::Adapter(err.to_string()))
}

fn kafka_error(error: impl fmt::Display) -> BootError {
    BootError::Adapter(error.to_string())
}

fn is_topic_already_exists(error: &KafkaClientError) -> bool {
    matches!(
        error,
        KafkaClientError::ServerError {
            protocol_error: ProtocolError::TopicAlreadyExists,
            ..
        }
    )
}

fn collect_brokers<I, S>(brokers: I) -> Vec<String>
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    brokers.into_iter().map(Into::into).collect()
}

fn duration_millis_i32(duration: Duration) -> i32 {
    duration
        .as_millis()
        .clamp(1, i32::MAX as u128)
        .try_into()
        .unwrap_or(i32::MAX)
}

fn next_request_id() -> String {
    let counter = NEXT_KAFKA_REQUEST_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{}-{nanos}-{counter}", std::process::id())
}

fn next_client_id(prefix: &str, role: &str) -> String {
    let counter = NEXT_KAFKA_CLIENT_ID.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    format!("{prefix}-{role}-{}-{nanos}-{counter}", std::process::id())
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
