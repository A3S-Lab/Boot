use crate::{BootError, BootRequest, HttpMethod, Result, SerializationOptions};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;

/// Request context visible to guards, interceptors, pipes, and filters.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub method: HttpMethod,
    pub request_path: String,
    pub route_path: String,
    pub module_name: Option<String>,
    pub controller_prefix: Option<String>,
    pub serialization: SerializationOptions,
    pub metadata: BTreeMap<String, Value>,
    pub request: BootRequest,
}

impl ExecutionContext {
    pub(crate) fn new(
        request: BootRequest,
        route_path: String,
        module_name: Option<String>,
        controller_prefix: Option<String>,
        serialization: SerializationOptions,
        metadata: BTreeMap<String, Value>,
    ) -> Self {
        Self {
            method: request.method,
            request_path: request.path.clone(),
            route_path,
            module_name,
            controller_prefix,
            serialization,
            metadata,
            request,
        }
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
                    "failed to deserialize execution context metadata `{key}`: {error}"
                ))
            })
    }
}
