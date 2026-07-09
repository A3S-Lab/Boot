use super::gateway::WebSocketGatewayDefinition;
use super::message::WebSocketMessage;
use crate::Result;
use std::fmt;

/// Adapter-neutral gateway server handle for broadcasting and connection lookup.
#[derive(Clone)]
pub struct WebSocketGatewayServer {
    gateway: WebSocketGatewayDefinition,
}

impl WebSocketGatewayServer {
    pub(crate) fn new(gateway: WebSocketGatewayDefinition) -> Self {
        Self { gateway }
    }

    pub fn path(&self) -> &str {
        self.gateway.path()
    }

    pub fn namespace(&self) -> Option<&str> {
        self.gateway.namespace()
    }

    pub fn events(&self) -> Vec<&str> {
        self.gateway.events()
    }

    pub fn active_connection_count(&self) -> Result<usize> {
        self.gateway.active_connection_count()
    }

    pub fn active_connection_ids(&self) -> Result<Vec<u64>> {
        self.gateway.active_connection_ids()
    }

    pub fn rooms(&self) -> Result<Vec<String>> {
        self.gateway.rooms()
    }

    pub fn room_members(&self, room: impl Into<String>) -> Result<Vec<u64>> {
        self.gateway.room_members(room)
    }

    pub async fn emit_to_connection(
        &self,
        connection_id: u64,
        message: WebSocketMessage,
    ) -> Result<bool> {
        self.gateway
            .emit_to_connection(connection_id, message)
            .await
    }

    pub async fn broadcast(&self, message: WebSocketMessage) -> Result<usize> {
        self.gateway.broadcast(message).await
    }

    pub async fn broadcast_to_room(
        &self,
        room: impl Into<String>,
        message: WebSocketMessage,
    ) -> Result<usize> {
        self.gateway.broadcast_to_room(room, message).await
    }
}

impl fmt::Debug for WebSocketGatewayServer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("WebSocketGatewayServer")
            .field("path", &self.path())
            .field("namespace", &self.namespace())
            .finish_non_exhaustive()
    }
}
