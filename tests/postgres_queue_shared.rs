#![cfg(feature = "queue-postgres")]

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use a3s_boot::{ModuleRef, PostgresQueueBackend, Queue, QueueContext, QueueJob, QueueOptions};
use serde_json::json;
use uuid::Uuid;

fn postgres_url() -> Option<String> {
    std::env::var("A3S_BOOT_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

#[tokio::test]
async fn independent_backends_share_leases_without_duplicate_processing() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = format!("boot-shared-{}", Uuid::new_v4());
    let options = QueueOptions::new()
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(60));
    let first_backend = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect first shared backend");
    let diagnostics = first_backend.clone();
    let second_backend = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect second shared backend");
    let first = Queue::new(name.clone(), first_backend);
    let second = Queue::new(name, second_backend);
    let calls = Arc::new(AtomicUsize::new(0));
    let processed = Arc::new(Mutex::new(Vec::new()));
    let processor = || {
        let calls = Arc::clone(&calls);
        let processed = Arc::clone(&processed);
        move |job: QueueJob, _context: QueueContext| {
            let calls = Arc::clone(&calls);
            let processed = Arc::clone(&processed);
            async move {
                tokio::time::sleep(Duration::from_millis(90)).await;
                processed.lock().expect("processed lock").push(job.id);
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }
    };
    first
        .process("shared", processor())
        .expect("register first processor");
    second
        .process("shared", processor())
        .expect("register second processor");

    let mut expected = BTreeSet::new();
    for value in 0..12 {
        let receipt = first
            .enqueue("shared", &json!({"value": value}))
            .await
            .expect("enqueue shared job");
        expected.insert(receipt.id);
    }
    first
        .start(ModuleRef::new())
        .await
        .expect("start first queue");
    second
        .start(ModuleRef::new())
        .await
        .expect("start second queue");
    tokio::time::timeout(Duration::from_secs(10), async {
        while calls.load(Ordering::SeqCst) < expected.len() {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("both backends should drain shared work");
    first.shutdown().await.expect("shutdown first queue");
    second.shutdown().await.expect("shutdown second queue");

    let actual = processed
        .lock()
        .expect("processed lock")
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>();
    assert_eq!(calls.load(Ordering::SeqCst), expected.len());
    assert_eq!(actual, expected);
    assert_eq!(
        diagnostics
            .stats_async()
            .await
            .expect("queue stats")
            .completed,
        12
    );
}
