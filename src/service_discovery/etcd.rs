//! Default etcd-backed service discovery for Boot microservice clients.
//!
//! ## Contract
//!
//! Keys: `{prefix}/{service}/{instance_id}`  
//! Default prefix: `/a3s/services`
//!
//! Value: JSON [`ServiceInstance`].
//!
//! Registration uses a TTL lease + keep-alive. Discovery does an initial prefix
//! get, then watches the prefix into a process-local cache. Synchronous
//! [`ServiceDiscovery::instances`] reads that cache only.

use super::{remove_instance_map, upsert_instance_map, ServiceDiscovery, ServiceInstance};
use crate::{BootError, Result};
use etcd_client::{Client, EventType, GetOptions, PutOptions, WatchOptions};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::sync::{oneshot, Mutex};
use tokio::task::JoinHandle;

const DEFAULT_PREFIX: &str = "/a3s/services";
const DEFAULT_LEASE_TTL_SECS: i64 = 30;

/// Connection and key-layout options for [`EtcdServiceDiscovery`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EtcdDiscoveryOptions {
    pub endpoints: Vec<String>,
    pub key_prefix: String,
    pub lease_ttl_secs: i64,
}

impl Default for EtcdDiscoveryOptions {
    fn default() -> Self {
        Self {
            endpoints: vec!["127.0.0.1:2379".to_string()],
            key_prefix: DEFAULT_PREFIX.to_string(),
            lease_ttl_secs: DEFAULT_LEASE_TTL_SECS,
        }
    }
}

impl EtcdDiscoveryOptions {
    pub fn new(endpoints: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            endpoints: endpoints.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }

    pub fn with_key_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.key_prefix = prefix.into();
        self
    }

    pub fn with_lease_ttl_secs(mut self, lease_ttl_secs: i64) -> Self {
        self.lease_ttl_secs = lease_ttl_secs;
        self
    }
}

/// Handle for a live etcd registration. Dropping it cancels keep-alive and
/// best-effort deletes the instance key.
pub struct EtcdRegistration {
    cancel: Option<oneshot::Sender<()>>,
    join: Option<JoinHandle<()>>,
}

impl EtcdRegistration {
    pub async fn cancel(mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(join) = self.join.take() {
            let _ = join.await;
        }
    }
}

impl Drop for EtcdRegistration {
    fn drop(&mut self) {
        if let Some(cancel) = self.cancel.take() {
            let _ = cancel.send(());
        }
        if let Some(join) = self.join.take() {
            join.abort();
        }
    }
}

/// Etcd-backed discovery with local cache + background watch.
pub struct EtcdServiceDiscovery {
    options: EtcdDiscoveryOptions,
    client: Mutex<Client>,
    cache: RwLock<BTreeMap<String, Vec<ServiceInstance>>>,
    watch_started: AtomicBool,
    watch_cancel: Mutex<Option<oneshot::Sender<()>>>,
    watch_join: Mutex<Option<JoinHandle<()>>>,
}

impl EtcdServiceDiscovery {
    /// Connect to etcd. Call [`Self::bootstrap`] before relying on discovery.
    pub async fn connect(options: EtcdDiscoveryOptions) -> Result<Arc<Self>> {
        if options.endpoints.is_empty() {
            return Err(BootError::BadRequest(
                "etcd discovery requires at least one endpoint".to_string(),
            ));
        }
        if options.lease_ttl_secs <= 0 {
            return Err(BootError::BadRequest(
                "etcd discovery lease_ttl_secs must be greater than zero".to_string(),
            ));
        }
        let client = Client::connect(options.endpoints.clone(), None)
            .await
            .map_err(|error| BootError::Internal(format!("etcd connect failed: {error}")))?;
        Ok(Arc::new(Self {
            options,
            client: Mutex::new(client),
            cache: RwLock::new(BTreeMap::new()),
            watch_started: AtomicBool::new(false),
            watch_cancel: Mutex::new(None),
            watch_join: Mutex::new(None),
        }))
    }

    pub fn options(&self) -> &EtcdDiscoveryOptions {
        &self.options
    }

    /// Load the current prefix snapshot and start the watch loop.
    pub async fn bootstrap(self: &Arc<Self>) -> Result<()> {
        self.refresh().await?;
        self.start_watch().await
    }

    /// Replace the local cache from a prefix get.
    pub async fn refresh(&self) -> Result<()> {
        let prefix = service_prefix(&self.options.key_prefix);
        let mut client = self.client.lock().await;
        let response = client
            .get(prefix, Some(GetOptions::new().with_prefix()))
            .await
            .map_err(|error| BootError::Internal(format!("etcd prefix get failed: {error}")))?;
        drop(client);

        let mut map = BTreeMap::new();
        for kv in response.kvs() {
            if let Some(instance) = decode_instance(kv.value()) {
                upsert_instance_map(&mut map, instance);
            }
        }
        let mut cache = self
            .cache
            .write()
            .map_err(|_| BootError::Internal("etcd discovery cache lock poisoned".to_string()))?;
        *cache = map;
        Ok(())
    }

    async fn start_watch(self: &Arc<Self>) -> Result<()> {
        if self
            .watch_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Ok(());
        }

        let prefix = service_prefix(&self.options.key_prefix);
        let mut client = self.client.lock().await;
        let mut stream = client
            .watch(prefix, Some(WatchOptions::new().with_prefix()))
            .await
            .map_err(|error| BootError::Internal(format!("etcd watch failed: {error}")))?;
        drop(client);

        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let this = Arc::clone(self);
        let join = tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    item = stream.message() => {
                        match item {
                            Ok(Some(response)) => {
                                for event in response.events() {
                                    this.apply_watch_event(event.event_type(), event.kv());
                                }
                            }
                            Ok(None) | Err(_) => {
                                // Stream ended or failed — attempt a blocking refresh once,
                                // then exit so the host can re-bootstrap.
                                let _ = this.refresh().await;
                                break;
                            }
                        }
                    }
                }
            }
            this.watch_started.store(false, Ordering::Release);
        });

        *self.watch_cancel.lock().await = Some(cancel_tx);
        *self.watch_join.lock().await = Some(join);
        Ok(())
    }

    fn apply_watch_event(&self, event_type: EventType, kv: Option<&etcd_client::KeyValue>) {
        let Some(kv) = kv else {
            return;
        };
        let Ok(mut cache) = self.cache.write() else {
            return;
        };
        match event_type {
            EventType::Put => {
                if let Some(instance) = decode_instance(kv.value()) {
                    upsert_instance_map(&mut cache, instance);
                }
            }
            EventType::Delete => {
                if let Some((service, id)) = parse_instance_key(&self.options.key_prefix, kv.key())
                {
                    remove_instance_map(&mut cache, &service, &id);
                }
            }
        }
    }

    /// Register an instance with a TTL lease and background keep-alive.
    pub async fn register(self: &Arc<Self>, instance: ServiceInstance) -> Result<EtcdRegistration> {
        if instance.service.is_empty() || instance.id.is_empty() {
            return Err(BootError::BadRequest(
                "etcd registration requires non-empty service and id".to_string(),
            ));
        }
        let key = instance_key(&self.options.key_prefix, &instance.service, &instance.id);
        let value = serde_json::to_vec(&instance).map_err(|error| {
            BootError::Internal(format!("failed to encode service instance: {error}"))
        })?;

        let mut client = self.client.lock().await;
        let lease = client
            .lease_grant(self.options.lease_ttl_secs, None)
            .await
            .map_err(|error| BootError::Internal(format!("etcd lease grant failed: {error}")))?;
        let lease_id = lease.id();
        client
            .put(
                key.clone(),
                value,
                Some(PutOptions::new().with_lease(lease_id)),
            )
            .await
            .map_err(|error| BootError::Internal(format!("etcd put failed: {error}")))?;
        let (mut keeper, mut keep_alive_stream) = client
            .lease_keep_alive(lease_id)
            .await
            .map_err(|error| BootError::Internal(format!("etcd keep-alive failed: {error}")))?;
        drop(client);

        {
            let mut cache = self.cache.write().map_err(|_| {
                BootError::Internal("etcd discovery cache lock poisoned".to_string())
            })?;
            upsert_instance_map(&mut cache, instance.clone());
        }

        let (cancel_tx, mut cancel_rx) = oneshot::channel();
        let this = Arc::clone(self);
        let service = instance.service.clone();
        let id = instance.id.clone();
        let join = tokio::spawn(async move {
            let interval =
                Duration::from_secs((this.options.lease_ttl_secs.max(2) as u64 / 3).max(1));
            let mut ticker = tokio::time::interval(interval);
            loop {
                tokio::select! {
                    _ = &mut cancel_rx => break,
                    _ = ticker.tick() => {
                        if keeper.keep_alive().await.is_err() {
                            break;
                        }
                        // Drain keep-alive responses so the stream does not stall.
                        while let Ok(Some(_)) = tokio::time::timeout(
                            Duration::from_millis(10),
                            keep_alive_stream.message(),
                        ).await.unwrap_or(Ok(None)) {}
                    }
                }
            }
            let mut client = this.client.lock().await;
            let _ = client.delete(key, None).await;
            let _ = client.lease_revoke(lease_id).await;
            drop(client);
            if let Ok(mut cache) = this.cache.write() {
                remove_instance_map(&mut cache, &service, &id);
            }
        });

        Ok(EtcdRegistration {
            cancel: Some(cancel_tx),
            join: Some(join),
        })
    }

    /// Delete an instance key immediately.
    pub async fn deregister(&self, service: &str, id: &str) -> Result<()> {
        let key = instance_key(&self.options.key_prefix, service, id);
        let mut client = self.client.lock().await;
        client
            .delete(key, None)
            .await
            .map_err(|error| BootError::Internal(format!("etcd delete failed: {error}")))?;
        drop(client);
        let mut cache = self
            .cache
            .write()
            .map_err(|_| BootError::Internal("etcd discovery cache lock poisoned".to_string()))?;
        remove_instance_map(&mut cache, service, id);
        Ok(())
    }

    /// Stop the background watch loop.
    pub async fn shutdown(&self) {
        if let Some(cancel) = self.watch_cancel.lock().await.take() {
            let _ = cancel.send(());
        }
        if let Some(join) = self.watch_join.lock().await.take() {
            let _ = join.await;
        }
        self.watch_started.store(false, Ordering::Release);
    }
}

impl ServiceDiscovery for EtcdServiceDiscovery {
    fn instances(&self, service: &str) -> Result<Vec<ServiceInstance>> {
        let cache = self
            .cache
            .read()
            .map_err(|_| BootError::Internal("etcd discovery cache lock poisoned".to_string()))?;
        Ok(cache
            .get(service)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|instance| instance.healthy)
            .collect())
    }
}

fn service_prefix(prefix: &str) -> String {
    let trimmed = prefix.trim_end_matches('/');
    if trimmed.is_empty() {
        DEFAULT_PREFIX.to_string()
    } else {
        format!("{trimmed}/")
    }
}

fn instance_key(prefix: &str, service: &str, id: &str) -> String {
    format!("{}{service}/{id}", service_prefix(prefix))
}

fn parse_instance_key(prefix: &str, key: &[u8]) -> Option<(String, String)> {
    let key = std::str::from_utf8(key).ok()?;
    let expected = service_prefix(prefix);
    let rest = key.strip_prefix(&expected)?;
    let (service, id) = rest.split_once('/')?;
    if service.is_empty() || id.is_empty() || id.contains('/') {
        return None;
    }
    Some((service.to_string(), id.to_string()))
}

fn decode_instance(value: &[u8]) -> Option<ServiceInstance> {
    serde_json::from_slice::<ServiceInstance>(value).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_key_and_parse_round_trip() {
        let key = instance_key("/a3s/services", "billing", "pod-1");
        assert_eq!(key, "/a3s/services/billing/pod-1");
        assert_eq!(
            parse_instance_key("/a3s/services", key.as_bytes()),
            Some(("billing".to_string(), "pod-1".to_string()))
        );
    }

    #[test]
    fn decode_instance_json() {
        let raw = br#"{"service":"billing","id":"a","address":"10.0.0.1","port":8080}"#;
        let instance = decode_instance(raw).unwrap();
        assert_eq!(instance.service, "billing");
        assert!(instance.healthy);
        assert_eq!(instance.port, 8080);
    }

    #[test]
    fn default_etcd_prefix_is_the_a3s_service_root() {
        assert_eq!(DEFAULT_PREFIX, "/a3s/services");
        assert_eq!(EtcdDiscoveryOptions::default().key_prefix, "/a3s/services");
    }
}
