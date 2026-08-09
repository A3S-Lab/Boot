#![cfg(feature = "queue-postgres")]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;
use std::{process::Command, process::Stdio};

use a3s_boot::{
    ModuleRef, PostgresQueueBackend, Queue, QueueContext, QueueJob, QueueJobOptions, QueueOptions,
    QueueRetryPolicy,
};
use serde_json::json;
use tokio::sync::Notify;
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
async fn queued_job_survives_backend_reconstruction() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("restart");
    let options = QueueOptions::new()
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(200));
    let first = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect first PostgreSQL queue backend");
    Queue::new(name.clone(), first)
        .enqueue("durable", &json!({"value": 42}))
        .await
        .expect("enqueue durable job");

    let calls = Arc::new(AtomicUsize::new(0));
    let second = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("reconnect PostgreSQL queue backend");
    let queue = Queue::new(name, second);
    let observed = Arc::clone(&calls);
    queue
        .process("durable", move |job: QueueJob, _context: QueueContext| {
            let observed = Arc::clone(&observed);
            async move {
                assert_eq!(job.data, json!({"value": 42}));
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .expect("register durable processor");
    queue
        .start(ModuleRef::new())
        .await
        .expect("start reconstructed queue");
    wait_until(|| calls.load(Ordering::SeqCst) == 1).await;
    queue.shutdown().await.expect("shutdown queue");
    assert_eq!(queue.stats().expect("queue stats").completed, 1);
}

#[tokio::test]
async fn processor_failures_follow_the_typed_retry_policy() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("retry");
    let backend = PostgresQueueBackend::connect(
        &url,
        &name,
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    )
    .await
    .expect("connect PostgreSQL queue backend");
    let queue = Queue::new(name, backend);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    queue
        .process("retry", move |_job: QueueJob, _context: QueueContext| {
            let observed = Arc::clone(&observed);
            async move {
                if observed.fetch_add(1, Ordering::SeqCst) == 0 {
                    return Err(a3s_boot::BootError::Internal("first attempt".into()));
                }
                Ok(())
            }
        })
        .expect("register retry processor");
    queue
        .enqueue_with_options(
            "retry",
            &json!({}),
            QueueJobOptions::new()
                .with_retry_policy(QueueRetryPolicy::fixed(1, Duration::from_millis(5))),
        )
        .await
        .expect("enqueue retried job");
    queue.start(ModuleRef::new()).await.expect("start queue");
    wait_until(|| calls.load(Ordering::SeqCst) == 2).await;
    queue.shutdown().await.expect("shutdown queue");
    let stats = queue.stats().expect("queue stats");
    assert_eq!(stats.completed, 1);
    assert_eq!(stats.failed, 0);
}

#[tokio::test]
async fn active_deduplication_keeps_only_the_latest_successor() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("dedup");
    let backend = PostgresQueueBackend::connect(
        &url,
        &name,
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    )
    .await
    .expect("connect PostgreSQL queue backend");
    let queue = Queue::new(name, backend);
    let started = Arc::new(Notify::new());
    let release = Arc::new(Notify::new());
    let values = Arc::new(std::sync::Mutex::new(Vec::new()));
    let processor_started = Arc::clone(&started);
    let processor_release = Arc::clone(&release);
    let observed = Arc::clone(&values);
    queue
        .process("dedup", move |job: QueueJob, _context: QueueContext| {
            let started = Arc::clone(&processor_started);
            let release = Arc::clone(&processor_release);
            let observed = Arc::clone(&observed);
            async move {
                let value = job.data["value"].as_u64().expect("numeric value");
                observed.lock().expect("values lock").push(value);
                if value == 1 {
                    started.notify_one();
                    release.notified().await;
                }
                Ok(())
            }
        })
        .expect("register deduplicated processor");
    let deduplicated = || {
        let mut options = QueueJobOptions::new().with_deduplication_id("same-target");
        options
            .deduplication
            .as_mut()
            .expect("deduplication options")
            .keep_last_if_active = true;
        options
    };
    queue
        .enqueue_with_options("dedup", &json!({"value": 1}), deduplicated())
        .await
        .expect("enqueue active owner");
    queue.start(ModuleRef::new()).await.expect("start queue");
    tokio::time::timeout(Duration::from_secs(5), started.notified())
        .await
        .expect("first job should become active");
    queue
        .enqueue_with_options("dedup", &json!({"value": 2}), deduplicated())
        .await
        .expect("enqueue first successor");
    queue
        .enqueue_with_options("dedup", &json!({"value": 3}), deduplicated())
        .await
        .expect("replace successor with latest payload");
    release.notify_one();
    wait_until(|| values.lock().expect("values lock").len() == 2).await;
    queue.shutdown().await.expect("shutdown queue");
    assert_eq!(*values.lock().expect("values lock"), vec![1, 3]);
}

#[tokio::test]
async fn processor_timeout_is_fenced_and_recorded() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("timeout");
    let backend = PostgresQueueBackend::connect(
        &url,
        &name,
        QueueOptions::new().with_poll_interval(Duration::from_millis(5)),
    )
    .await
    .expect("connect timeout queue");
    let diagnostics = backend.clone();
    let queue = Queue::new(name, backend);
    queue
        .process(
            "timeout",
            |_job: QueueJob, _context: QueueContext| async move {
                std::future::pending::<a3s_boot::Result<()>>().await
            },
        )
        .expect("register timeout processor");
    queue
        .enqueue_with_options(
            "timeout",
            &json!({}),
            QueueJobOptions::new().with_timeout(Duration::from_millis(20)),
        )
        .await
        .expect("enqueue timeout job");
    queue.start(ModuleRef::new()).await.expect("start queue");
    let timeout_result = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if diagnostics
                .stats_async()
                .await
                .expect("timeout stats")
                .failed
                == 1
            {
                break;
            }
            assert_eq!(diagnostics.last_worker_error().expect("worker error"), None);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await;
    if timeout_result.is_err() {
        let worker_error = diagnostics.last_worker_error().expect("worker error");
        let shutdown_error = queue.shutdown().await.err();
        panic!(
            "timeout job did not fail; worker_error={worker_error:?}, shutdown_error={shutdown_error:?}"
        );
    }
    queue.shutdown().await.expect("shutdown queue");
    let failures = diagnostics
        .failures_async()
        .await
        .expect("timeout failures");
    assert_eq!(failures.len(), 1);
    assert!(failures[0].message.contains("timed out"));
}

#[tokio::test]
async fn heartbeat_prevents_competing_workers_from_reclaiming_live_work() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("heartbeat");
    let options = QueueOptions::new()
        .with_worker_count(2)
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(60));
    let backend = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect heartbeat queue");
    let diagnostics = backend.clone();
    let queue = Queue::new(name, backend);
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    queue
        .process("slow", move |_job: QueueJob, _context: QueueContext| {
            let observed = Arc::clone(&observed);
            async move {
                observed.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(220)).await;
                Ok(())
            }
        })
        .expect("register slow processor");
    queue
        .enqueue("slow", &json!({}))
        .await
        .expect("enqueue slow job");
    queue.start(ModuleRef::new()).await.expect("start queue");
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if diagnostics
                .stats_async()
                .await
                .expect("heartbeat stats")
                .completed
                == 1
            {
                break;
            }
            assert_eq!(diagnostics.last_worker_error().expect("worker error"), None);
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("heartbeat job should complete");
    queue.shutdown().await.expect("shutdown queue");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        diagnostics
            .stats_async()
            .await
            .expect("heartbeat stats")
            .failed,
        0
    );
}

#[tokio::test]
async fn expired_lease_recovers_the_same_job_after_process_death() {
    let Some(url) = postgres_url() else {
        eprintln!("skipping PostgreSQL queue test; set A3S_BOOT_POSTGRES_URL");
        return;
    };
    let name = queue_name("process-death");
    let options = QueueOptions::new()
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(150));
    let backend = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect process-death queue");
    let queue = Queue::new(name.clone(), backend);
    let receipt = queue
        .enqueue("block", &json!({"effectId": "stable-effect"}))
        .await
        .expect("enqueue process-death job");

    let executable = std::env::current_exe().expect("resolve PostgreSQL test executable");
    let child = Command::new(executable)
        .arg("--exact")
        .arg("postgres_queue_process_death_probe")
        .arg("--nocapture")
        .env("A3S_BOOT_POSTGRES_URL", &url)
        .env("A3S_BOOT_POSTGRES_CRASH_PROBE", "1")
        .env("A3S_BOOT_POSTGRES_CRASH_QUEUE", &name)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn PostgreSQL queue crash probe");
    let mut child = ChildGuard(Some(child));
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if queue.stats().expect("process-death stats").active == 1 {
                return;
            }
            if child
                .0
                .as_mut()
                .expect("crash probe child")
                .try_wait()
                .expect("poll crash probe")
                .is_some()
            {
                panic!("PostgreSQL queue crash probe exited before leasing work");
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("crash probe should lease the job");
    let mut killed = child.0.take().expect("take crash probe child");
    killed.kill().expect("kill PostgreSQL queue worker process");
    let killed_status = killed.wait().expect("wait for killed queue worker");
    assert!(!killed_status.success());

    tokio::time::sleep(Duration::from_millis(200)).await;
    let recovered = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("reconnect after process death");
    let recovered_queue = Queue::new(name, recovered);
    let recovered_job_id = receipt.id.clone();
    let completed = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&completed);
    recovered_queue
        .process("block", move |job: QueueJob, _context: QueueContext| {
            let observed = Arc::clone(&observed);
            let recovered_job_id = recovered_job_id.clone();
            async move {
                assert_eq!(job.id, recovered_job_id);
                assert_eq!(job.data["effectId"], "stable-effect");
                observed.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .expect("register recovered processor");
    recovered_queue
        .start(ModuleRef::new())
        .await
        .expect("start recovered queue");
    wait_until(|| completed.load(Ordering::SeqCst) == 1).await;
    recovered_queue
        .shutdown()
        .await
        .expect("shutdown recovered queue");
    let stats = recovered_queue.stats().expect("recovered queue stats");
    assert_eq!(stats.active, 0);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.completed, 1);
    assert_eq!(stats.failed, 0);
}

#[tokio::test]
async fn postgres_queue_process_death_probe() {
    if std::env::var("A3S_BOOT_POSTGRES_CRASH_PROBE").as_deref() != Ok("1") {
        return;
    }
    let url = std::env::var("A3S_BOOT_POSTGRES_URL").expect("crash probe PostgreSQL URL");
    let name = std::env::var("A3S_BOOT_POSTGRES_CRASH_QUEUE").expect("crash probe queue name");
    let options = QueueOptions::new()
        .with_poll_interval(Duration::from_millis(5))
        .with_lease_duration(Duration::from_millis(150));
    let backend = PostgresQueueBackend::connect(&url, &name, options)
        .await
        .expect("connect crash probe queue");
    let queue = Queue::new(name, backend);
    queue
        .process(
            "block",
            |_job: QueueJob, _context: QueueContext| async move {
                std::future::pending::<a3s_boot::Result<()>>().await
            },
        )
        .expect("register blocking crash probe processor");
    queue
        .start(ModuleRef::new())
        .await
        .expect("start crash probe queue");
    std::future::pending::<()>().await;
}

struct ChildGuard(Option<std::process::Child>);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        if let Some(child) = self.0.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
