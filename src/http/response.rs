use crate::{BootError, Result};
use serde::Serialize;
use std::collections::BTreeMap;

/// Framework-neutral HTTP response returned by Boot route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl BootResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self::new(200, body.into()).with_header("content-type", "text/plain; charset=utf-8")
    }

    pub fn json<T>(body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let body = serde_json::to_vec(body).map_err(|err| BootError::Internal(err.to_string()))?;
        Ok(Self::new(200, body).with_header("content-type", "application/json"))
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}
