use crate::{BootError, Result};
use futures_core::Stream;
use serde::Serialize;
use std::pin::Pin;

pub type SseStream = Pin<Box<dyn Stream<Item = Result<SseEvent>> + Send + 'static>>;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SseEvent {
    id: Option<String>,
    event: Option<String>,
    retry: Option<u64>,
    comment: Option<String>,
    data: Option<String>,
}

impl SseEvent {
    pub fn new(data: impl Into<String>) -> Self {
        Self::default().with_data(data)
    }

    pub fn json<T>(data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let data =
            serde_json::to_string(data).map_err(|err| BootError::Internal(err.to_string()))?;
        Ok(Self::new(data))
    }

    pub fn comment(comment: impl Into<String>) -> Self {
        Self::default().with_comment(comment)
    }

    pub fn stream<I>(events: I) -> SseStream
    where
        I: IntoIterator<Item = SseEvent>,
        I::IntoIter: Send + 'static,
    {
        Box::pin(futures_util::stream::iter(events.into_iter().map(Ok)))
    }

    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    pub fn with_event(mut self, event: impl Into<String>) -> Self {
        self.event = Some(event.into());
        self
    }

    pub fn with_retry(mut self, retry: u64) -> Self {
        self.retry = Some(retry);
        self
    }

    pub fn with_comment(mut self, comment: impl Into<String>) -> Self {
        self.comment = Some(comment.into());
        self
    }

    pub fn with_data(mut self, data: impl Into<String>) -> Self {
        self.data = Some(data.into());
        self
    }

    pub fn encode(&self) -> Vec<u8> {
        let mut encoded = String::new();

        if let Some(comment) = &self.comment {
            push_comment(&mut encoded, comment);
        }
        if let Some(id) = &self.id {
            push_field(&mut encoded, "id", id);
        }
        if let Some(event) = &self.event {
            push_field(&mut encoded, "event", event);
        }
        if let Some(retry) = self.retry {
            encoded.push_str("retry: ");
            encoded.push_str(&retry.to_string());
            encoded.push('\n');
        }
        if let Some(data) = &self.data {
            push_field(&mut encoded, "data", data);
        }

        encoded.push('\n');
        encoded.into_bytes()
    }
}

fn push_comment(encoded: &mut String, value: &str) {
    for line in sse_lines(value) {
        encoded.push_str(": ");
        encoded.push_str(&line);
        encoded.push('\n');
    }
}

fn push_field(encoded: &mut String, name: &str, value: &str) {
    for line in sse_lines(value) {
        encoded.push_str(name);
        encoded.push_str(": ");
        encoded.push_str(&line);
        encoded.push('\n');
    }
}

fn sse_lines(value: &str) -> Vec<String> {
    let normalized = value.replace("\r\n", "\n").replace('\r', "\n");
    if normalized.is_empty() {
        return vec![String::new()];
    }

    normalized.split('\n').map(str::to_string).collect()
}
