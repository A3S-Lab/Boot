use super::{BootResponse, CookieOptions};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Request-bound response metadata handle for Nest-style passthrough response decorators.
#[derive(Clone, Default)]
pub struct ResponsePassthrough {
    state: Arc<RwLock<ResponsePassthroughState>>,
}

#[derive(Debug, Clone, Default)]
struct ResponsePassthroughState {
    status: Option<u16>,
    headers: BTreeMap<String, String>,
    appended_headers: Vec<(String, String)>,
}

impl fmt::Debug for ResponsePassthrough {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.snapshot() {
            Ok(state) => f
                .debug_struct("ResponsePassthrough")
                .field("status", &state.status)
                .field("headers", &state.headers)
                .field("appended_headers", &state.appended_headers)
                .finish(),
            Err(_) => f
                .debug_struct("ResponsePassthrough")
                .field("state", &"<poisoned>")
                .finish(),
        }
    }
}

impl ResponsePassthrough {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_status(&self, status: u16) -> Result<()> {
        BootResponse::empty(status).validate_status()?;
        self.write_state()?.status = Some(status);
        Ok(())
    }

    pub fn status(&self, status: u16) -> Result<Self> {
        self.set_status(status)?;
        Ok(self.clone())
    }

    pub fn set_header(&self, name: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let (name, value) = normalized_header(name, value)?;
        self.write_state()?.headers.insert(name, value);
        Ok(())
    }

    pub fn header(&self, name: impl Into<String>, value: impl Into<String>) -> Result<Self> {
        self.set_header(name, value)?;
        Ok(self.clone())
    }

    pub fn append_header(&self, name: impl Into<String>, value: impl Into<String>) -> Result<()> {
        let (name, value) = normalized_header(name, value)?;
        self.write_state()?.appended_headers.push((name, value));
        Ok(())
    }

    pub fn set_cookie(
        &self,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
        options: CookieOptions,
    ) -> Result<()> {
        let header = options.set_cookie_header(name.as_ref(), value.as_ref())?;
        self.append_header("set-cookie", header)
    }

    pub fn cookie(
        &self,
        name: impl AsRef<str>,
        value: impl AsRef<str>,
        options: CookieOptions,
    ) -> Result<Self> {
        self.set_cookie(name, value, options)?;
        Ok(self.clone())
    }

    pub fn delete_cookie(&self, name: impl AsRef<str>, options: CookieOptions) -> Result<()> {
        let header = options.delete_cookie_header(name.as_ref())?;
        self.append_header("set-cookie", header)
    }

    pub fn has_changes(&self) -> Result<bool> {
        let state = self.snapshot()?;
        Ok(state.status.is_some()
            || !state.headers.is_empty()
            || !state.appended_headers.is_empty())
    }

    pub fn apply(&self, mut response: BootResponse) -> Result<BootResponse> {
        let state = self.snapshot()?;
        if let Some(status) = state.status {
            response = response.with_status(status);
        }
        for (name, value) in state.headers {
            response = response.with_header(name, value);
        }
        for (name, value) in state.appended_headers {
            response = response.append_header(name, value);
        }
        Ok(response)
    }

    fn snapshot(&self) -> Result<ResponsePassthroughState> {
        self.state
            .read()
            .map_err(|_| BootError::Internal("response passthrough lock is poisoned".to_string()))
            .map(|state| state.clone())
    }

    fn write_state(&self) -> Result<std::sync::RwLockWriteGuard<'_, ResponsePassthroughState>> {
        self.state
            .write()
            .map_err(|_| BootError::Internal("response passthrough lock is poisoned".to_string()))
    }
}

fn normalized_header(
    name: impl Into<String>,
    value: impl Into<String>,
) -> Result<(String, String)> {
    let response = BootResponse::empty(200).with_header(name, value);
    response.validate_headers()?;
    response
        .headers
        .into_iter()
        .next()
        .ok_or_else(|| BootError::Internal("missing normalized response header".to_string()))
}
