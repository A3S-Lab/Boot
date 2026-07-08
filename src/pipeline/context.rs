use crate::{BootError, BootRequest, HttpMethod, Result, SerializationOptions};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;

/// Protocol handled by an [`ExecutionContext`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionProtocol {
    Http,
    WebSocket,
    Transport,
}

impl ExecutionProtocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Http => "http",
            Self::WebSocket => "websocket",
            Self::Transport => "transport",
        }
    }
}

/// Transport handler style visible through protocol-neutral execution context.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExecutionTransportKind {
    RequestResponse,
    Event,
}

impl ExecutionTransportKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RequestResponse => "request-response",
            Self::Event => "event",
        }
    }
}

/// WebSocket-specific execution details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebSocketExecutionContext {
    pub gateway_path: String,
    pub event: String,
}

/// Transport-specific execution details.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportExecutionContext {
    pub pattern: String,
    pub kind: ExecutionTransportKind,
}

/// Context visible to guards, interceptors, pipes, and filters.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub protocol: ExecutionProtocol,
    pub method: HttpMethod,
    pub request_path: String,
    pub route_path: String,
    pub module_name: Option<String>,
    pub controller_prefix: Option<String>,
    pub serialization: SerializationOptions,
    pub metadata: BTreeMap<String, Value>,
    pub request: BootRequest,
    pub websocket: Option<WebSocketExecutionContext>,
    pub transport: Option<TransportExecutionContext>,
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
            protocol: ExecutionProtocol::Http,
            method: request.method,
            request_path: request.path.clone(),
            route_path,
            module_name,
            controller_prefix,
            serialization,
            metadata,
            request,
            websocket: None,
            transport: None,
        }
    }

    pub(crate) fn websocket(
        request: BootRequest,
        gateway_path: String,
        event: String,
        module_name: Option<String>,
    ) -> Self {
        Self {
            protocol: ExecutionProtocol::WebSocket,
            method: request.method,
            request_path: request.path.clone(),
            route_path: gateway_path.clone(),
            module_name,
            controller_prefix: None,
            serialization: SerializationOptions::default(),
            metadata: BTreeMap::new(),
            request,
            websocket: Some(WebSocketExecutionContext {
                gateway_path,
                event,
            }),
            transport: None,
        }
    }

    pub(crate) fn transport(
        pattern: String,
        kind: ExecutionTransportKind,
        module_name: Option<String>,
    ) -> Self {
        Self {
            protocol: ExecutionProtocol::Transport,
            method: HttpMethod::Post,
            request_path: pattern.clone(),
            route_path: pattern.clone(),
            module_name,
            controller_prefix: None,
            serialization: SerializationOptions::default(),
            metadata: BTreeMap::new(),
            request: BootRequest::new(HttpMethod::Post, "/__transport"),
            websocket: None,
            transport: Some(TransportExecutionContext { pattern, kind }),
        }
    }

    pub fn protocol(&self) -> ExecutionProtocol {
        self.protocol
    }

    pub fn websocket_context(&self) -> Option<&WebSocketExecutionContext> {
        self.websocket.as_ref()
    }

    pub fn transport_context(&self) -> Option<&TransportExecutionContext> {
        self.transport.as_ref()
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
