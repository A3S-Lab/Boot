//! Transactional outbox primitive (P3.1) — table contract + in-memory relay.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use serde_json::Value;

/// One unpublished integration fact.
#[derive(Debug, Clone)]
pub struct OutboxMessage {
    pub id: String,
    pub topic: String,
    pub payload: Value,
}

/// Port for persisting outbox rows in the same DB transaction as domain writes.
pub trait OutboxStore: Send + Sync + 'static {
    fn enqueue(&self, topic: &str, payload: Value) -> Result<OutboxMessage, String>;
    fn dequeue_batch(&self, limit: usize) -> Result<Vec<OutboxMessage>, String>;
    fn ack(&self, id: &str) -> Result<(), String>;
}

/// Process-local outbox for demos and unit tests.
#[derive(Debug, Default)]
pub struct InMemoryOutbox {
    pending: Arc<Mutex<VecDeque<OutboxMessage>>>,
    seq: AtomicU64,
}

impl InMemoryOutbox {
    pub fn new() -> Self {
        Self::default()
    }
}

impl OutboxStore for InMemoryOutbox {
    fn enqueue(&self, topic: &str, payload: Value) -> Result<OutboxMessage, String> {
        let id = format!("outbox-{}", self.seq.fetch_add(1, Ordering::SeqCst));
        let message = OutboxMessage {
            id,
            topic: topic.to_owned(),
            payload,
        };
        self.pending
            .lock()
            .map_err(|_| "outbox lock poisoned".to_owned())?
            .push_back(message.clone());
        Ok(message)
    }

    fn dequeue_batch(&self, limit: usize) -> Result<Vec<OutboxMessage>, String> {
        let mut guard = self
            .pending
            .lock()
            .map_err(|_| "outbox lock poisoned".to_owned())?;
        let mut batch = Vec::new();
        for _ in 0..limit {
            match guard.pop_front() {
                Some(message) => batch.push(message),
                None => break,
            }
        }
        Ok(batch)
    }

    fn ack(&self, _id: &str) -> Result<(), String> {
        Ok(())
    }
}
