use super::id::{generate_session_id, validate_session_id};
use super::options::SessionOptions;
use super::store::{InMemorySessionStore, SessionStore};
use crate::{BootError, BootRequest, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

/// Provider-backed session manager.
#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    options: SessionOptions,
}

impl fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionManager")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl SessionManager {
    pub fn new<S>(store: S) -> Self
    where
        S: SessionStore,
    {
        Self::from_store_arc(Arc::new(store))
    }

    pub fn from_store_arc(store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            options: SessionOptions::default(),
        }
    }

    pub fn in_memory(options: SessionOptions) -> Self {
        Self::new(InMemorySessionStore::new()).with_options(options)
    }

    pub fn with_options(mut self, options: SessionOptions) -> Self {
        self.options = options;
        self
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    pub fn session_id(&self, request: &BootRequest) -> Result<Option<String>> {
        if let Some(session_id) = request.header(self.options.request_header_name()) {
            return Ok(Some(validate_session_id(session_id.to_string())?));
        }

        self.cookie_session_id(request)
    }

    pub fn require_session_id(&self, request: &BootRequest) -> Result<String> {
        self.session_id(request)?
            .ok_or_else(|| BootError::Unauthorized("missing session id".to_string()))
    }

    pub fn cookie_session_id(&self, request: &BootRequest) -> Result<Option<String>> {
        request
            .cookie(self.options.cookie_name())?
            .map(validate_session_id)
            .transpose()
    }

    pub fn create_session_id(&self) -> Result<String> {
        generate_session_id()
    }

    pub fn get<T>(&self, session_id: &str, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.get_value(session_id, key)? else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| BootError::Internal(format!("invalid session value `{key}`: {error}")))
    }

    pub fn get_value(&self, session_id: &str, key: &str) -> Result<Option<Value>> {
        Ok(self
            .load_data(session_id)?
            .and_then(|data| data.get(key).cloned()))
    }

    pub fn set<T>(&self, session_id: &str, key: impl Into<String>, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!("failed to serialize session value: {error}"))
        })?;
        self.set_value(session_id, key, value)
    }

    pub fn set_value(&self, session_id: &str, key: impl Into<String>, value: Value) -> Result<()> {
        let session_id = validate_session_id(session_id.to_string())?;
        let mut data = self.load_data(&session_id)?.unwrap_or_default();
        data.insert(key.into(), value);
        self.save_data(session_id, data)
    }

    pub fn remove_key(&self, session_id: &str, key: &str) -> Result<bool> {
        let session_id = validate_session_id(session_id.to_string())?;
        let Some(mut data) = self.load_data(&session_id)? else {
            return Ok(false);
        };
        let removed = data.remove(key).is_some();
        if data.is_empty() {
            self.store.remove(&session_id)?;
        } else {
            self.save_data(session_id, data)?;
        }
        Ok(removed)
    }

    pub fn destroy(&self, session_id: &str) -> Result<bool> {
        self.store
            .remove(&validate_session_id(session_id.to_string())?)
    }

    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }

    pub fn has_data(&self, session_id: &str) -> Result<bool> {
        Ok(self
            .load_data(&validate_session_id(session_id.to_string())?)?
            .is_some_and(|data| !data.is_empty()))
    }

    fn load_data(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>> {
        self.store.load(session_id)
    }

    fn save_data(&self, session_id: String, data: BTreeMap<String, Value>) -> Result<()> {
        if data.is_empty() {
            self.store.remove(&session_id)?;
            return Ok(());
        }
        self.store.save(session_id, data, self.options.ttl())
    }
}
