use super::gateway::WebSocketGatewayDefinition;
use super::server::WebSocketGatewayServer;
use crate::{BootRequest, ExecutionContext};
use std::ops::Deref;

/// Context available to WebSocket guards and interceptors.
#[derive(Debug, Clone)]
pub struct WebSocketContext {
    pub request: BootRequest,
    pub gateway_path: String,
    pub event: String,
    pub namespace: Option<String>,
    pub module_name: Option<String>,
    server: WebSocketGatewayServer,
    execution_context: ExecutionContext,
}

impl WebSocketContext {
    pub(crate) fn new(
        gateway: &WebSocketGatewayDefinition,
        request: BootRequest,
        event: &str,
    ) -> Self {
        let gateway_path = gateway.path().to_string();
        let event = event.to_string();
        let namespace = gateway.namespace().map(str::to_string);
        let module_name = gateway.module_name().map(str::to_string);
        let metadata = gateway
            .event_metadata(&event)
            .cloned()
            .unwrap_or_else(|| gateway.metadata().clone());
        let execution_context = ExecutionContext::websocket(
            request.clone(),
            gateway_path.clone(),
            event.clone(),
            namespace.clone(),
            module_name.clone(),
            metadata,
        );
        Self {
            request,
            gateway_path,
            event,
            namespace,
            module_name,
            server: gateway.server(),
            execution_context,
        }
    }

    pub fn server(&self) -> WebSocketGatewayServer {
        self.server.clone()
    }

    pub fn execution_context(&self) -> &ExecutionContext {
        &self.execution_context
    }

    pub fn into_execution_context(self) -> ExecutionContext {
        self.execution_context
    }
}

impl Deref for WebSocketContext {
    type Target = ExecutionContext;

    fn deref(&self) -> &Self::Target {
        self.execution_context()
    }
}

/// Context passed to WebSocket gateway initialization hooks.
#[derive(Debug, Clone)]
pub struct WebSocketGatewayInitContext {
    pub gateway_path: String,
    pub namespace: Option<String>,
    pub module_name: Option<String>,
    pub events: Vec<String>,
    server: WebSocketGatewayServer,
}

impl WebSocketGatewayInitContext {
    pub(crate) fn new(gateway: &WebSocketGatewayDefinition) -> Self {
        Self {
            gateway_path: gateway.path().to_string(),
            namespace: gateway.namespace().map(str::to_string),
            module_name: gateway.module_name().map(str::to_string),
            events: gateway.events().into_iter().map(str::to_string).collect(),
            server: gateway.server(),
        }
    }

    pub fn server(&self) -> WebSocketGatewayServer {
        self.server.clone()
    }
}
