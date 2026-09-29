//! HTTP Idempotency-Key middleware helpers (P1.4).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::{BootRequest, BootResponse, Result};

/// In-memory idempotency cache for a single process (demo / default).
#[derive(Debug, Default, Clone)]
pub struct IdempotencyStore {
    inner: Arc<Mutex<HashMap<String, CachedResponse>>>,
}

#[derive(Debug, Clone)]
struct CachedResponse {
    status: u16,
    body: Vec<u8>,
}

impl IdempotencyStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn get(&self, key: &str) -> Option<(u16, Vec<u8>)> {
        let Ok(guard) = self.inner.lock() else {
            return None;
        };
        guard
            .get(key)
            .map(|cached| (cached.status, cached.body.clone()))
    }

    pub fn put(&self, key: impl Into<String>, status: u16, body: Vec<u8>) {
        if let Ok(mut guard) = self.inner.lock() {
            guard.insert(key.into(), CachedResponse { status, body });
        }
    }
}

/// Read `Idempotency-Key` from an inbound request.
pub fn idempotency_key(request: &BootRequest) -> Option<String> {
    request
        .header("idempotency-key")
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
}

/// Replay a cached response when the key already completed.
pub fn replay_if_present(
    store: &IdempotencyStore,
    request: &BootRequest,
) -> Result<Option<BootResponse>> {
    let Some(key) = idempotency_key(request) else {
        return Ok(None);
    };
    let Some((status, body)) = store.get(&key) else {
        return Ok(None);
    };
    Ok(Some(
        BootResponse::new(status, body).with_header("idempotent-replay", "true"),
    ))
}
