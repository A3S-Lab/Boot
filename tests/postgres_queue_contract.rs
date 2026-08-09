#![cfg(feature = "queue-postgres")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{
    BootError, ModuleRef, PostgresQueueBackend, Queue, QueueJobOptions, QueueJobRetention,
    QueueOptions,
};
use serde_json::json;
use uuid::Uuid;

fn postgres_url() -> Option<String> {
    std::env::var("A3S_BOOT_POSTGRES_URL")
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn queue_name(label: &str) -> String {
    format!("boot-{label}-{}", Uuid::new_v4())
}

async fn wait_until(mut predicate: impl FnMut() -> bool) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if predicate() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("PostgreSQL queue condition should become true");
}

#[tokio::test]
async fn caller_assigned_job_ids_are_idempotent_and_conflicts_are_explicit() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("idempotency");
    let backend = PostgresQueueBackend::connect(&url, &name, QueueOptions::new())
        .await
        .expect("connect idempotency queue");
    let diagnostics = backend.clone();
    let queue = Queue::new(name, backend);
    let options = QueueJobOptions::new().with_job_id("stable-job-id");

    let first = queue
        .enqueue_with_options("work", &json!({"value": 1}), options.clone())
        .await
        .expect("enqueue caller-assigned job");
    let replay = queue
        .enqueue_with_options("work", &json!({"value": 1}), options.clone())
        .await
        .expect("replay caller-assigned job");
    assert_eq!(first, replay);
    assert_eq!(
        diagnostics
            .stats_async()
            .await
            .expect("queue stats")
            .pending,
        1
    );

    let error = queue
        .enqueue_with_options("work", &json!({"value": 2}), options)
        .await
        .expect_err("different work must not reuse a caller-assigned job id");
    assert!(matches!(error, BootError::Conflict(_)));

    diagnostics.clear_async().await.expect("clear queue");
    assert_eq!(
        diagnostics
            .stats_async()
            .await
            .expect("queue stats")
            .pending,
        0
    );
}

#[tokio::test]
async fn completion_retention_keeps_only_the_newest_terminal_job() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("retention");
    let backend = PostgresQueueBackend::connect(
        &url,
        &name,
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    )
    .await
    .expect("connect retention queue");
    let diagnostics = backend.clone();
    let queue = Queue::new(name, backend);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    queue
        .process("retain", move |_job, _context| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .expect("register retention processor");
    for value in 1..=3 {
        queue
            .enqueue_with_options(
                "retain",
                &json!({"value": value}),
                QueueJobOptions::new().with_completion_retention(QueueJobRetention::count(1)),
            )
            .await
            .expect("enqueue retained job");
    }
    queue.start(ModuleRef::new()).await.expect("start queue");
    wait_until(|| calls.load(Ordering::SeqCst) == 3).await;
    queue.shutdown().await.expect("shutdown queue");

    let jobs = diagnostics.jobs_async().await.expect("retained jobs");
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].data, json!({"value": 3}));
    assert_eq!(
        diagnostics
            .stats_async()
            .await
            .expect("queue stats")
            .completed,
        1
    );
}

#[tokio::test]
async fn malformed_retention_is_rejected_before_storage() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("retention-validation");
    let backend = PostgresQueueBackend::connect(&url, &name, QueueOptions::new())
        .await
        .expect("connect retention validation queue");
    let queue = Queue::new(name, backend);
    let malformed = QueueJobRetention {
        age: None,
        count: None,
        limit: None,
    };

    let error = queue
        .enqueue_with_options(
            "work",
            &json!({}),
            QueueJobOptions::new().with_completion_retention(malformed),
        )
        .await
        .expect_err("empty retention policy must be rejected");
    assert!(matches!(error, BootError::BadRequest(_)));
}
