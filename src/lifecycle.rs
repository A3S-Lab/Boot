//! Process lifecycle helpers: etcd register on start, deregister on drop (P0.4).

use std::sync::Arc;

use crate::service_discovery::{
    EtcdDiscoveryOptions, EtcdRegistration, EtcdServiceDiscovery, ServiceInstance,
};
use crate::Result;

/// Holds a live etcd registration until dropped (cancels lease keep-alive).
pub struct ServiceLifecycle {
    _registration: EtcdRegistration,
}

impl ServiceLifecycle {
    /// Register `instance` under the discovery prefix and keep the lease alive.
    pub async fn register(
        discovery: Arc<EtcdServiceDiscovery>,
        instance: ServiceInstance,
    ) -> Result<Self> {
        let registration = discovery.register(instance).await?;
        Ok(Self {
            _registration: registration,
        })
    }
}

/// Register this process in etcd when `A3S_ETCD_ENDPOINTS` is set.
///
/// Env:
/// - `A3S_ETCD_ENDPOINTS` — comma-separated hosts (`127.0.0.1:2379`); empty → no-op
/// - `A3S_SERVICE_NAME` — logical service name (default: `default_service`)
/// - `A3S_SERVICE_ID` — instance id (default: `{name}-{pid}`)
/// - `A3S_SERVICE_HOST` — advertise address (default: `127.0.0.1`)
/// - `A3S_SERVICE_PORT` — advertise port (default: `default_port`)
///
/// Returns `Ok(None)` when discovery is disabled. Callers must keep the returned
/// value alive for the process lifetime.
pub async fn try_register_from_env(
    default_service: &str,
    default_port: u16,
) -> Result<Option<ServiceLifecycle>> {
    let raw = match std::env::var("A3S_ETCD_ENDPOINTS") {
        Ok(value) if !value.trim().is_empty() => value,
        _ => return Ok(None),
    };
    let endpoints: Vec<String> = raw
        .split(',')
        .map(str::trim)
        .filter(|part| !part.is_empty())
        .map(|part| {
            part.trim_start_matches("http://")
                .trim_start_matches("https://")
                .to_string()
        })
        .collect();
    if endpoints.is_empty() {
        return Ok(None);
    }

    let service = std::env::var("A3S_SERVICE_NAME")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| default_service.to_owned());
    let id = std::env::var("A3S_SERVICE_ID")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| format!("{service}-{}", std::process::id()));
    let host = std::env::var("A3S_SERVICE_HOST")
        .ok()
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| "127.0.0.1".to_owned());
    let port = std::env::var("A3S_SERVICE_PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(default_port);

    let discovery = EtcdServiceDiscovery::connect(EtcdDiscoveryOptions::new(endpoints)).await?;
    let instance = ServiceInstance::new(service, id, host, port);
    let lifecycle = ServiceLifecycle::register(discovery, instance).await?;
    Ok(Some(lifecycle))
}
