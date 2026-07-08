use crate::{BootError, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Storage backend for session data.
pub trait SessionStore: Send + Sync + 'static {
    fn load(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>>;

    fn save(
        &self,
        session_id: String,
        data: BTreeMap<String, Value>,
        ttl: Option<Duration>,
    ) -> Result<()>;

    fn remove(&self, session_id: &str) -> Result<bool>;

    fn clear(&self) -> Result<()>;
}

/// In-memory session store suitable for tests and single-process services.
#[derive(Debug, Clone, Default)]
pub struct InMemorySessionStore {
    sessions: Arc<RwLock<BTreeMap<String, StoredSession>>>,
}

#[derive(Debug, Clone)]
struct StoredSession {
    data: BTreeMap<String, Value>,
    expires_at: Option<Instant>,
}

impl StoredSession {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn write_sessions(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, StoredSession>>> {
        self.sessions
            .write()
            .map_err(|_| BootError::Internal("session store lock is poisoned".to_string()))
    }
}

impl SessionStore for InMemorySessionStore {
    fn load(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>> {
        let mut sessions = self.write_sessions()?;
        let Some(session) = sessions.get(session_id) else {
            return Ok(None);
        };

        if session.is_expired() {
            sessions.remove(session_id);
            return Ok(None);
        }

        Ok(Some(session.data.clone()))
    }

    fn save(
        &self,
        session_id: String,
        data: BTreeMap<String, Value>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        self.write_sessions()?
            .insert(session_id, StoredSession { data, expires_at });
        Ok(())
    }

    fn remove(&self, session_id: &str) -> Result<bool> {
        Ok(self.write_sessions()?.remove(session_id).is_some())
    }

    fn clear(&self) -> Result<()> {
        self.write_sessions()?.clear();
        Ok(())
    }
}
