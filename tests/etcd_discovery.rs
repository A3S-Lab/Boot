#![cfg(feature = "etcd-discovery")]

//! Live etcd discovery smoke test.
//!
//! ```bash
//! ETCD_ENDPOINTS=127.0.0.1:2379 cargo test -p a3s-boot --features etcd-discovery --test etcd_discovery -- --ignored --nocapture
//! ```

use a3s_boot::{EtcdDiscoveryOptions, EtcdServiceDiscovery, ServiceDiscovery, ServiceInstance};
use std::time::Duration;

fn endpoints_from_env() -> Option<Vec<String>> {
    std::env::var("ETCD_ENDPOINTS").ok().map(|value| {
        value
            .split(',')
            .map(str::trim)
            .filter(|item| !item.is_empty())
            .map(str::to_string)
            .collect()
    })
}

#[tokio::test]
#[ignore = "requires a running etcd; set ETCD_ENDPOINTS"]
async fn etcd_register_is_visible_to_another_client() {
    let endpoints = endpoints_from_env().expect("ETCD_ENDPOINTS");
    let prefix = format!(
        "/a3s/services-test/{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    );
    let options = EtcdDiscoveryOptions::new(endpoints)
        .with_key_prefix(prefix)
        .with_lease_ttl_secs(10);

    let publisher = EtcdServiceDiscovery::connect(options.clone())
        .await
        .expect("connect publisher");
    let consumer = EtcdServiceDiscovery::connect(options)
        .await
        .expect("connect consumer");
    consumer.bootstrap().await.expect("bootstrap consumer");

    let registration = publisher
        .register(ServiceInstance::new("billing", "pod-1", "10.0.0.7", 8080))
        .await
        .expect("register");

    tokio::time::sleep(Duration::from_millis(200)).await;
    let instances = consumer.instances("billing").expect("instances");
    assert_eq!(instances.len(), 1);
    assert_eq!(instances[0].id, "pod-1");
    assert_eq!(instances[0].address, "10.0.0.7");

    registration.cancel().await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(consumer.instances("billing").unwrap().is_empty());
    consumer.shutdown().await;
}
