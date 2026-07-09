use crate::{BootError, BoxFuture, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::future::Future;
use std::sync::Arc;

/// Adapter-neutral WebSocket message used by gateways and adapters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WebSocketMessage {
    pub event: String,
    #[serde(default)]
    pub data: Value,
}

impl WebSocketMessage {
    pub fn new(event: impl Into<String>, data: impl Into<Value>) -> Self {
        Self {
            event: event.into(),
            data: data.into(),
        }
    }

    pub fn event(&self) -> &str {
        &self.event
    }

    pub fn data(&self) -> &Value {
        &self.data
    }

    pub fn text(event: impl Into<String>, data: impl Into<String>) -> Self {
        Self::new(event, Value::String(data.into()))
    }

    pub fn json<T>(event: impl Into<String>, data: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(Self::new(
            event,
            serde_json::to_value(data).map_err(|err| BootError::Internal(err.to_string()))?,
        ))
    }
}

/// Return value accepted by WebSocket gateway handlers.
pub trait IntoWebSocketReply {
    fn into_websocket_reply(self) -> Option<WebSocketMessage>;
}

impl IntoWebSocketReply for WebSocketMessage {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        Some(self)
    }
}

impl IntoWebSocketReply for Option<WebSocketMessage> {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        self
    }
}

impl IntoWebSocketReply for () {
    fn into_websocket_reply(self) -> Option<WebSocketMessage> {
        None
    }
}

/// Outbound writer for adapter-backed WebSocket connections.
pub trait WebSocketOutbound: Send + Sync + 'static {
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> WebSocketOutbound for F
where
    F: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(message))
    }
}

impl WebSocketOutbound for Arc<dyn WebSocketOutbound> {
    fn send(&self, message: WebSocketMessage) -> BoxFuture<'static, Result<()>> {
        self.as_ref().send(message)
    }
}

pub(crate) async fn send_to_outbounds(
    outbounds: Vec<Arc<dyn WebSocketOutbound>>,
    message: WebSocketMessage,
) -> Result<usize> {
    let mut sent = 0;
    for outbound in outbounds {
        outbound.send(message.clone()).await?;
        sent += 1;
    }
    Ok(sent)
}
