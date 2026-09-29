//! Service discovery abstractions for Boot microservice clients.
//!
//! Gateway owns **ingress** discovery (`providers.discovery` polling into the
//! local service registry). Boot owns **egress** discovery so application code
//! can resolve named peers without hard-coding URLs.
//!
//! The default production backend is etcd (`etcd-discovery` feature): services
//! register under a prefix with a TTL lease, and clients watch that prefix into
//! a local cache. [`ServiceDiscovery::instances`] stays synchronous so call
//! paths do not block on the network.

#[cfg(feature = "etcd-discovery")]
mod etcd;

use crate::{BootError, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, RwLock};

#[cfg(feature = "etcd-discovery")]
pub use etcd::{EtcdDiscoveryOptions, EtcdRegistration, EtcdServiceDiscovery};

/// One reachable instance of a named service.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ServiceInstance {
    pub service: String,
    pub id: String,
    pub address: String,
    pub port: u16,
    #[serde(default = "default_healthy")]
    pub healthy: bool,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

fn default_healthy() -> bool {
    true
}

impl ServiceInstance {
    pub fn new(
        service: impl Into<String>,
        id: impl Into<String>,
        address: impl Into<String>,
        port: u16,
    ) -> Self {
        Self {
            service: service.into(),
            id: id.into(),
            address: address.into(),
            port,
            healthy: true,
            metadata: BTreeMap::new(),
        }
    }

    pub fn with_healthy(mut self, healthy: bool) -> Self {
        self.healthy = healthy;
        self
    }

    pub fn with_metadata(mut self, metadata: BTreeMap<String, String>) -> Self {
        self.metadata = metadata;
        self
    }

    pub fn base_url(&self) -> String {
        if self.address.contains("://") {
            format!("{}:{}", self.address.trim_end_matches('/'), self.port)
        } else {
            format!("http://{}:{}", self.address, self.port)
        }
    }
}

/// Resolve healthy instances for a logical service name.
pub trait ServiceDiscovery: Send + Sync + 'static {
    fn instances(&self, service: &str) -> Result<Vec<ServiceInstance>>;
}

/// In-memory discovery registry suitable for tests and static topologies.
#[derive(Debug, Default, Clone)]
pub struct StaticServiceDiscovery {
    instances: Arc<RwLock<BTreeMap<String, Vec<ServiceInstance>>>>,
}

impl StaticServiceDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn upsert(&self, instance: ServiceInstance) -> Result<()> {
        if instance.service.is_empty() {
            return Err(BootError::BadRequest(
                "service discovery instance service name cannot be empty".to_string(),
            ));
        }
        let mut guard = self
            .instances
            .write()
            .map_err(|_| BootError::Internal("service discovery lock poisoned".to_string()))?;
        let entries = guard.entry(instance.service.clone()).or_default();
        if let Some(existing) = entries.iter_mut().find(|item| item.id == instance.id) {
            *existing = instance;
        } else {
            entries.push(instance);
        }
        Ok(())
    }

    pub fn remove(&self, service: &str, id: &str) -> Result<()> {
        let mut guard = self
            .instances
            .write()
            .map_err(|_| BootError::Internal("service discovery lock poisoned".to_string()))?;
        if let Some(entries) = guard.get_mut(service) {
            entries.retain(|item| item.id != id);
            if entries.is_empty() {
                guard.remove(service);
            }
        }
        Ok(())
    }
}

impl ServiceDiscovery for StaticServiceDiscovery {
    fn instances(&self, service: &str) -> Result<Vec<ServiceInstance>> {
        let guard = self
            .instances
            .read()
            .map_err(|_| BootError::Internal("service discovery lock poisoned".to_string()))?;
        Ok(guard
            .get(service)
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|instance| instance.healthy)
            .collect())
    }
}

/// Client-side load balancing over discovered instances.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServiceBalanceStrategy {
    RoundRobin,
    FirstHealthy,
}

impl Default for ServiceBalanceStrategy {
    fn default() -> Self {
        Self::RoundRobin
    }
}

/// Selects an instance from a discovery source.
#[derive(Clone)]
pub struct ServiceLoadBalancer {
    discovery: Arc<dyn ServiceDiscovery>,
    strategy: ServiceBalanceStrategy,
    cursor: Arc<AtomicUsize>,
}

impl std::fmt::Debug for ServiceLoadBalancer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServiceLoadBalancer")
            .field("strategy", &self.strategy)
            .finish_non_exhaustive()
    }
}

impl ServiceLoadBalancer {
    pub fn new(discovery: Arc<dyn ServiceDiscovery>) -> Self {
        Self {
            discovery,
            strategy: ServiceBalanceStrategy::RoundRobin,
            cursor: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn with_strategy(mut self, strategy: ServiceBalanceStrategy) -> Self {
        self.strategy = strategy;
        self
    }

    pub fn select(&self, service: &str) -> Result<ServiceInstance> {
        let instances = self.discovery.instances(service)?;
        if instances.is_empty() {
            return Err(BootError::ServiceUnavailable(format!(
                "no healthy instances for service '{service}'"
            )));
        }
        match self.strategy {
            ServiceBalanceStrategy::FirstHealthy => Ok(instances[0].clone()),
            ServiceBalanceStrategy::RoundRobin => {
                let index = self.cursor.fetch_add(1, Ordering::Relaxed) % instances.len();
                Ok(instances[index].clone())
            }
        }
    }
}

pub(crate) fn upsert_instance_map(
    map: &mut BTreeMap<String, Vec<ServiceInstance>>,
    instance: ServiceInstance,
) {
    let entries = map.entry(instance.service.clone()).or_default();
    if let Some(existing) = entries.iter_mut().find(|item| item.id == instance.id) {
        *existing = instance;
    } else {
        entries.push(instance);
    }
}

pub(crate) fn remove_instance_map(
    map: &mut BTreeMap<String, Vec<ServiceInstance>>,
    service: &str,
    id: &str,
) {
    if let Some(entries) = map.get_mut(service) {
        entries.retain(|item| item.id != id);
        if entries.is_empty() {
            map.remove(service);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_robin_rotates_healthy_instances() {
        let discovery = StaticServiceDiscovery::new();
        discovery
            .upsert(ServiceInstance::new("billing", "a", "10.0.0.1", 8080))
            .unwrap();
        discovery
            .upsert(ServiceInstance::new("billing", "b", "10.0.0.2", 8080))
            .unwrap();
        discovery
            .upsert(ServiceInstance::new("billing", "c", "10.0.0.3", 8080).with_healthy(false))
            .unwrap();

        let balancer = ServiceLoadBalancer::new(Arc::new(discovery));
        let first = balancer.select("billing").unwrap();
        let second = balancer.select("billing").unwrap();
        let third = balancer.select("billing").unwrap();

        assert_eq!(first.id, "a");
        assert_eq!(second.id, "b");
        assert_eq!(third.id, "a");
    }
}
