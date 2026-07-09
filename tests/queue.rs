#![cfg(feature = "queue")]

use a3s_boot::{
    BootApplication, BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, Queue,
    QueueContext, QueueJob, QueueJobOptions, QueueJobState, QueueModule, QueueOptions, Result,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct EmailJob {
    to: String,
}

#[tokio::test]
async fn queue_module_processes_enqueued_jobs() {
    let calls = Arc::new(AtomicUsize::new(0));
    let processor_calls = Arc::clone(&calls);
    let app = BootApplication::builder()
        .import(
            QueueModule::in_process_with_options(
                "mail-queue",
                QueueOptions::new().with_worker_count(2),
            )
            .processor("email.send", move |job: QueueJob, context: QueueContext| {
                let processor_calls = Arc::clone(&processor_calls);
                async move {
                    assert_eq!(context.queue_name, "mail-queue");
                    let email = job.data_as::<EmailJob>()?;
                    assert!(email.to.ends_with("@example.com"));
                    processor_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            }),
        )
        .build()
        .unwrap();
    let queue = app.get::<Queue>().unwrap();

    app.bootstrap().await.unwrap();
    queue
        .enqueue(
            "email.send",
            &EmailJob {
                to: "one@example.com".to_string(),
            },
        )
        .await
        .unwrap();
    queue
        .enqueue(
            "email.send",
            &EmailJob {
                to: "two@example.com".to_string(),
            },
        )
        .await
        .unwrap();
    wait_until(|| calls.load(Ordering::SeqCst) == 2).await;
    app.shutdown().await.unwrap();

    let stats = queue.stats().unwrap();
    assert_eq!(stats.completed, 2);
    assert_eq!(stats.pending, 0);
    assert_eq!(stats.active, 0);
}

#[derive(Debug)]
struct MailService {
    calls: Arc<AtomicUsize>,
}

impl MailService {
    fn send(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct UsesQueueModule {
    queue_module: QueueModule,
    calls: Arc<AtomicUsize>,
}

impl Module for UsesQueueModule {
    fn name(&self) -> &'static str {
        "uses-queue"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.queue_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![ProviderDefinition::singleton(MailService { calls })])
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let queue = module_ref.get::<Queue>()?;
            let mail = module_ref.get::<MailService>()?;
            queue.process("email.send", move |_job, context: QueueContext| {
                let mail = Arc::clone(&mail);
                async move {
                    assert!(context.module_ref.contains_provider::<Queue>()?);
                    mail.send();
                    Ok(())
                }
            })?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn queue_provider_can_register_processors_after_workers_start() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(UsesQueueModule {
            queue_module: QueueModule::in_process("mail-queue"),
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();
    let queue = app.get::<Queue>().unwrap();

    app.bootstrap().await.unwrap();
    queue
        .enqueue(
            "email.send",
            &EmailJob {
                to: "late@example.com".to_string(),
            },
        )
        .await
        .unwrap();
    wait_until(|| calls.load(Ordering::SeqCst) == 1).await;
    app.shutdown().await.unwrap();
}

#[tokio::test]
async fn queued_jobs_wait_until_a_processor_is_registered() {
    let calls = Arc::new(AtomicUsize::new(0));
    let queue = Queue::in_process("mail-queue");
    queue.start(ModuleRef::new()).await.unwrap();

    queue
        .enqueue(
            "email.send",
            &EmailJob {
                to: "waiting@example.com".to_string(),
            },
        )
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(20)).await;
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(queue.stats().unwrap().pending, 1);

    let processor_calls = Arc::clone(&calls);
    queue
        .process("email.send", move |_job, _context| {
            let processor_calls = Arc::clone(&processor_calls);
            async move {
                processor_calls.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        })
        .unwrap();

    wait_until(|| calls.load(Ordering::SeqCst) == 1).await;
    queue.shutdown().await.unwrap();
    assert_eq!(queue.stats().unwrap().completed, 1);
}

#[tokio::test]
async fn queue_module_uses_lane_job_priority_ordering() {
    let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
    let processor_seen = Arc::clone(&seen);
    let app = BootApplication::builder()
        .import(
            QueueModule::in_process_with_options(
                "priority-queue",
                QueueOptions::new()
                    .with_worker_count(1)
                    .with_poll_interval(Duration::from_millis(5)),
            )
            .processor(
                "email.send",
                move |job: QueueJob, _context: QueueContext| {
                    let processor_seen = Arc::clone(&processor_seen);
                    async move {
                        let email = job.data_as::<EmailJob>()?;
                        processor_seen.lock().unwrap().push(email.to);
                        Ok(())
                    }
                },
            ),
        )
        .build()
        .unwrap();
    let queue = app.get::<Queue>().unwrap();

    queue
        .enqueue_with_options(
            "email.send",
            &EmailJob {
                to: "low@example.com".to_string(),
            },
            QueueJobOptions::new().with_priority(100),
        )
        .await
        .unwrap();
    queue
        .enqueue_with_options(
            "email.send",
            &EmailJob {
                to: "high@example.com".to_string(),
            },
            QueueJobOptions::new().with_priority(1),
        )
        .await
        .unwrap();

    app.bootstrap().await.unwrap();
    wait_until(|| seen.lock().unwrap().len() == 2).await;
    app.shutdown().await.unwrap();

    assert_eq!(
        seen.lock().unwrap().as_slice(),
        ["high@example.com", "low@example.com"]
    );
}

#[test]
fn queue_module_exports_named_and_global_queue_providers() {
    let named = BootApplication::builder()
        .import(QueueModule::in_process("named-queue").named("mail-queue"))
        .build()
        .unwrap();
    assert!(named.get_named::<Queue>("mail-queue").is_ok());
    assert!(named.get_optional::<Queue>().unwrap().is_none());

    let global = BootApplication::builder()
        .import(QueueModule::in_process("global-queue").global())
        .build()
        .unwrap();
    assert!(global.get::<Queue>().is_ok());
}

#[tokio::test]
async fn queue_records_processor_failures_and_job_states() {
    let app = BootApplication::builder()
        .import(
            QueueModule::in_process("mail-queue").processor("email.send", |_job, _context| async {
                Err(BootError::Internal("smtp unavailable".to_string()))
            }),
        )
        .build()
        .unwrap();
    let queue = app.get::<Queue>().unwrap();

    app.bootstrap().await.unwrap();
    let receipt = queue
        .enqueue(
            "email.send",
            &EmailJob {
                to: "fail@example.com".to_string(),
            },
        )
        .await
        .unwrap();
    wait_until(|| queue.stats().unwrap().failed == 1).await;
    app.shutdown().await.unwrap();

    let failures = queue.failures().unwrap();
    assert_eq!(failures.len(), 1);
    assert_eq!(failures[0].id, receipt.id);
    assert!(failures[0].message.contains("smtp unavailable"));

    let jobs = queue.jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].state, QueueJobState::Failed);
}

#[test]
fn queue_validates_job_names_and_worker_count() {
    let app = BootApplication::builder()
        .import(QueueModule::in_process_with_options(
            "bad-queue",
            QueueOptions::new().with_worker_count(0),
        ))
        .build()
        .unwrap();
    let queue = app.get::<Queue>().unwrap();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_time()
        .build()
        .unwrap();
    let error = runtime.block_on(async { queue.start(ModuleRef::new()).await.unwrap_err() });
    assert!(matches!(error, BootError::Internal(message) if message.contains("worker count")));

    let direct = Queue::in_process("mail-queue");
    let error = runtime.block_on(async {
        direct
            .enqueue(
                " ",
                &EmailJob {
                    to: "bad@example.com".to_string(),
                },
            )
            .await
            .unwrap_err()
    });
    assert!(matches!(error, BootError::Internal(message) if message.contains("job name")));
}

async fn wait_until(predicate: impl Fn() -> bool) {
    for _ in 0..50 {
        if predicate() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("condition was not met before timeout");
}
