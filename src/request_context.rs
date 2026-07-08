#[cfg(feature = "auth")]
use crate::AuthPrincipal;
use crate::{BootError, BootRequest, HttpMethod, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, RwLock};

tokio::task_local! {
    static CURRENT_REQUEST_CONTEXT: RequestContext;
}

/// Task-local request details available during one HTTP route execution.
#[derive(Debug, Clone)]
pub struct RequestContext {
    method: HttpMethod,
    request_path: String,
    route_path: String,
    module_name: Option<String>,
    controller_prefix: Option<String>,
    request_id: Option<String>,
    headers: BTreeMap<String, String>,
    params: BTreeMap<String, String>,
    query: BTreeMap<String, String>,
    metadata: BTreeMap<String, Value>,
    values: Arc<RwLock<BTreeMap<String, Value>>>,
    #[cfg(feature = "auth")]
    auth_principal: Arc<RwLock<Option<AuthPrincipal>>>,
}

impl RequestContext {
    pub(crate) fn from_route_request(
        request: &BootRequest,
        route_path: impl Into<String>,
        module_name: Option<String>,
        controller_prefix: Option<String>,
        metadata: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            method: request.method(),
            request_path: request.path().to_string(),
            route_path: route_path.into(),
            module_name,
            controller_prefix,
            request_id: request
                .header("x-request-id")
                .or_else(|| request.header("x-correlation-id"))
                .map(ToString::to_string),
            headers: request
                .header_entries()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            params: request.params.clone(),
            query: request.query.clone(),
            metadata,
            values: Arc::new(RwLock::new(BTreeMap::new())),
            #[cfg(feature = "auth")]
            auth_principal: Arc::new(RwLock::new(None)),
        }
    }

    pub async fn scope<F, T>(context: Self, future: F) -> T
    where
        F: Future<Output = T>,
    {
        CURRENT_REQUEST_CONTEXT.scope(context, future).await
    }

    pub fn current() -> Result<Self> {
        CURRENT_REQUEST_CONTEXT
            .try_with(Clone::clone)
            .map_err(|_| BootError::Internal("request context is not available".to_string()))
    }

    pub fn try_current() -> Option<Self> {
        CURRENT_REQUEST_CONTEXT.try_with(Clone::clone).ok()
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn request_path(&self) -> &str {
        &self.request_path
    }

    pub fn route_path(&self) -> &str {
        &self.route_path
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn controller_prefix(&self) -> Option<&str> {
        self.controller_prefix.as_deref()
    }

    pub fn request_id(&self) -> Option<&str> {
        self.request_id.as_deref()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn metadata_as<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.metadata.get(key) else {
            return Ok(None);
        };

        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                BootError::Internal(format!(
                    "failed to deserialize request context metadata `{key}`: {error}"
                ))
            })
    }

    pub fn set_value<V>(&self, key: impl Into<String>, value: V) -> Result<()>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize request context value `{key}`: {error}"
            ))
        })?;
        self.set_value_raw(key, value)
    }

    pub fn set_value_raw(&self, key: impl Into<String>, value: Value) -> Result<()> {
        self.write_values()?.insert(key.into(), value);
        Ok(())
    }

    pub fn value(&self, key: &str) -> Result<Option<Value>> {
        Ok(self.read_values()?.get(key).cloned())
    }

    pub fn value_as<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.value(key)? else {
            return Ok(None);
        };

        serde_json::from_value(value).map(Some).map_err(|error| {
            BootError::Internal(format!(
                "failed to deserialize request context value `{key}`: {error}"
            ))
        })
    }

    #[cfg(feature = "auth")]
    pub fn set_auth_principal(&self, principal: AuthPrincipal) -> Result<()> {
        *self.write_auth_principal()? = Some(principal);
        Ok(())
    }

    #[cfg(feature = "auth")]
    pub fn auth_principal(&self) -> Result<Option<AuthPrincipal>> {
        Ok(self.read_auth_principal()?.clone())
    }

    fn read_values(&self) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, Value>>> {
        self.values
            .read()
            .map_err(|_| BootError::Internal("request context value lock is poisoned".to_string()))
    }

    fn write_values(&self) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, Value>>> {
        self.values
            .write()
            .map_err(|_| BootError::Internal("request context value lock is poisoned".to_string()))
    }

    #[cfg(feature = "auth")]
    fn read_auth_principal(&self) -> Result<std::sync::RwLockReadGuard<'_, Option<AuthPrincipal>>> {
        self.auth_principal.read().map_err(|_| {
            BootError::Internal("request context auth principal lock is poisoned".to_string())
        })
    }

    #[cfg(feature = "auth")]
    fn write_auth_principal(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, Option<AuthPrincipal>>> {
        self.auth_principal.write().map_err(|_| {
            BootError::Internal("request context auth principal lock is poisoned".to_string())
        })
    }
}
