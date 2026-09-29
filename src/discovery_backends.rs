//! Additional discovery backends (P4.1) — trait-shaped placeholders.

use std::sync::Arc;

use crate::service_discovery::{ServiceDiscovery, ServiceInstance};
use crate::{BootError, Result};

/// Marker for Kubernetes Endpoints-backed discovery (implementation TBD).
#[derive(Debug, Default)]
pub struct KubernetesServiceDiscovery {
    cache: Arc<std::sync::RwLock<Vec<ServiceInstance>>>,
}

impl KubernetesServiceDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, instances: Vec<ServiceInstance>) -> Result<()> {
        let mut guard = self
            .cache
            .write()
            .map_err(|_| BootError::Internal("k8s discovery lock poisoned".into()))?;
        *guard = instances;
        Ok(())
    }
}

impl ServiceDiscovery for KubernetesServiceDiscovery {
    fn instances(&self, service: &str) -> Result<Vec<ServiceInstance>> {
        let guard = self
            .cache
            .read()
            .map_err(|_| BootError::Internal("k8s discovery lock poisoned".into()))?;
        Ok(guard
            .iter()
            .filter(|instance| instance.service == service)
            .cloned()
            .collect())
    }
}

/// Marker for Consul-backed discovery (implementation TBD).
#[derive(Debug, Default)]
pub struct ConsulServiceDiscovery {
    cache: Arc<std::sync::RwLock<Vec<ServiceInstance>>>,
}

impl ConsulServiceDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn seed(&self, instances: Vec<ServiceInstance>) -> Result<()> {
        let mut guard = self
            .cache
            .write()
            .map_err(|_| BootError::Internal("consul discovery lock poisoned".into()))?;
        *guard = instances;
        Ok(())
    }
}

impl ServiceDiscovery for ConsulServiceDiscovery {
    fn instances(&self, service: &str) -> Result<Vec<ServiceInstance>> {
        let guard = self
            .cache
            .read()
            .map_err(|_| BootError::Internal("consul discovery lock poisoned".into()))?;
        Ok(guard
            .iter()
            .filter(|instance| instance.service == service)
            .cloned()
            .collect())
    }
}
