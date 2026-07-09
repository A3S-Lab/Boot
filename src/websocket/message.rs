use crate::{validate_value, BootError, BoxFuture, Result, Validate};
use serde::de::DeserializeOwned;
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

    pub fn data_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.data.clone())
            .map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn data_field(&self, name: &str) -> Result<Option<Value>> {
        let Value::Object(fields) = &self.data else {
            return Err(BootError::BadRequest(
                "expected JSON object websocket data".to_string(),
            ));
        };

        Ok(fields.get(name).filter(|value| !value.is_null()).cloned())
    }

    pub fn data_field_as<T>(&self, name: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.data_field(name)? else {
            return Err(BootError::BadRequest(format!(
                "missing websocket data field: {name}"
            )));
        };
        deserialize_data_field("websocket data field", name, value)
    }

    pub fn optional_data_field_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.data_field(name)?
            .map(|value| deserialize_data_field("websocket data field", name, value))
            .transpose()
    }

    pub fn data_field_string(&self, name: &str) -> Result<String> {
        let Some(value) = self.data_field(name)? else {
            return Err(BootError::BadRequest(format!(
                "missing websocket data field: {name}"
            )));
        };
        data_field_value_to_string(value)
    }

    pub fn optional_data_field_string(&self, name: &str) -> Result<Option<String>> {
        self.data_field(name)?
            .map(data_field_value_to_string)
            .transpose()
    }

    pub fn validated_data<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        validate_value(self.data_as::<T>()?)
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

fn deserialize_data_field<T>(label: &str, name: &str, value: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value)
        .map_err(|error| BootError::BadRequest(format!("invalid {label} {name}: {error}")))
}

fn data_field_value_to_string(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(&value).map_err(|error| BootError::BadRequest(error.to_string()))
        }
        Value::Null => Ok("null".to_string()),
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
