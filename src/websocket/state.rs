use super::message::WebSocketOutbound;
use crate::{BootError, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Mutex};

#[derive(Default)]
pub(crate) struct WebSocketGatewayState {
    next_connection_id: std::sync::atomic::AtomicU64,
    connections: Mutex<BTreeMap<u64, WebSocketConnectionState>>,
}

struct WebSocketConnectionState {
    rooms: BTreeSet<String>,
    outbound: Option<Arc<dyn WebSocketOutbound>>,
}

impl WebSocketGatewayState {
    pub(crate) fn next_connection_id(&self) -> u64 {
        self.next_connection_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            + 1
    }

    pub(crate) fn register(
        &self,
        id: u64,
        outbound: Option<Arc<dyn WebSocketOutbound>>,
    ) -> Result<()> {
        self.connections()?.insert(
            id,
            WebSocketConnectionState {
                rooms: BTreeSet::new(),
                outbound,
            },
        );
        Ok(())
    }

    pub(crate) fn unregister(&self, id: u64) -> Result<()> {
        self.connections()?.remove(&id);
        Ok(())
    }

    pub(crate) fn join(&self, id: u64, room: impl Into<String>) -> Result<()> {
        let room = normalize_room(room)?;
        let mut connections = self.connections()?;
        let connection = connections.get_mut(&id).ok_or_else(|| {
            BootError::BadRequest(format!("websocket connection {id} is not open"))
        })?;
        connection.rooms.insert(room);
        Ok(())
    }

    pub(crate) fn leave(&self, id: u64, room: impl Into<String>) -> Result<()> {
        let room = normalize_room(room)?;
        let mut connections = self.connections()?;
        let Some(connection) = connections.get_mut(&id) else {
            return Ok(());
        };
        connection.rooms.remove(&room);
        Ok(())
    }

    pub(crate) fn connection_count(&self) -> Result<usize> {
        Ok(self.connections()?.len())
    }

    pub(crate) fn connection_ids(&self) -> Result<Vec<u64>> {
        Ok(self.connections()?.keys().copied().collect())
    }

    pub(crate) fn rooms(&self) -> Result<Vec<String>> {
        let mut rooms = BTreeSet::new();
        for connection in self.connections()?.values() {
            rooms.extend(connection.rooms.iter().cloned());
        }
        Ok(rooms.into_iter().collect())
    }

    pub(crate) fn rooms_for_connection(&self, id: u64) -> Result<Vec<String>> {
        Ok(self
            .connections()?
            .get(&id)
            .map(|connection| connection.rooms.iter().cloned().collect())
            .unwrap_or_default())
    }

    pub(crate) fn room_members(&self, room: impl Into<String>) -> Result<Vec<u64>> {
        let room = normalize_room(room)?;
        Ok(self
            .connections()?
            .iter()
            .filter_map(|(id, connection)| connection.rooms.contains(&room).then_some(*id))
            .collect())
    }

    pub(crate) fn outbound_for_connection(
        &self,
        id: u64,
    ) -> Result<Option<Arc<dyn WebSocketOutbound>>> {
        Ok(self
            .connections()?
            .get(&id)
            .and_then(|connection| connection.outbound.clone()))
    }

    pub(crate) fn broadcast_targets(
        &self,
        room: Option<&str>,
        exclude_connection_id: Option<u64>,
    ) -> Result<Vec<Arc<dyn WebSocketOutbound>>> {
        let connections = self.connections()?;
        Ok(connections
            .iter()
            .filter(|(id, _)| Some(**id) != exclude_connection_id)
            .filter(|(_, connection)| match room {
                Some(room) => connection.rooms.contains(room),
                None => true,
            })
            .filter_map(|(_, connection)| connection.outbound.clone())
            .collect())
    }

    fn connections(
        &self,
    ) -> Result<std::sync::MutexGuard<'_, BTreeMap<u64, WebSocketConnectionState>>> {
        self.connections.lock().map_err(|_| {
            BootError::Internal("websocket gateway state lock is poisoned".to_string())
        })
    }
}

pub(crate) fn normalize_room(room: impl Into<String>) -> Result<String> {
    let room = room.into();
    let room = room.trim();
    if room.is_empty() {
        return Err(BootError::BadRequest(
            "websocket room cannot be empty".to_string(),
        ));
    }
    Ok(room.to_string())
}

pub(crate) fn normalize_namespace(namespace: impl Into<String>) -> Result<String> {
    let namespace = namespace.into();
    let namespace = namespace.trim();
    if namespace.is_empty() {
        return Err(BootError::BadRequest(
            "websocket namespace cannot be empty".to_string(),
        ));
    }
    if namespace.contains('?') || namespace.contains('#') {
        return Err(BootError::BadRequest(format!(
            "websocket namespace cannot contain query or fragment markers: {namespace}"
        )));
    }
    if namespace.starts_with('/') {
        Ok(namespace.to_string())
    } else {
        Ok(format!("/{namespace}"))
    }
}
