use super::id::validate_session_id;
use super::manager::SessionManager;
use crate::Result;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::fmt;
use std::sync::Arc;

/// Request-bound session handle exposed to route handlers.
#[derive(Clone)]
pub struct Session {
    manager: Arc<SessionManager>,
    session_id: String,
}

impl fmt::Debug for Session {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Session")
            .field("session_id", &self.session_id)
            .finish_non_exhaustive()
    }
}

impl Session {
    pub fn from_manager_arc(manager: Arc<SessionManager>, session_id: String) -> Result<Self> {
        Ok(Self {
            manager,
            session_id: validate_session_id(session_id)?,
        })
    }

    pub fn id(&self) -> &str {
        &self.session_id
    }

    pub fn manager(&self) -> &SessionManager {
        &self.manager
    }

    pub fn get<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.manager.get(&self.session_id, key)
    }

    pub fn get_value(&self, key: &str) -> Result<Option<Value>> {
        self.manager.get_value(&self.session_id, key)
    }

    pub fn set<T>(&self, key: impl Into<String>, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        self.manager.set(&self.session_id, key, value)
    }

    pub fn set_value(&self, key: impl Into<String>, value: Value) -> Result<()> {
        self.manager.set_value(&self.session_id, key, value)
    }

    pub fn remove_key(&self, key: &str) -> Result<bool> {
        self.manager.remove_key(&self.session_id, key)
    }

    pub fn destroy(&self) -> Result<bool> {
        self.manager.destroy(&self.session_id)
    }

    pub fn has_data(&self) -> Result<bool> {
        self.manager.has_data(&self.session_id)
    }
}
