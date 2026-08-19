use std::collections::BTreeMap;
use std::fmt;
use std::future::{pending, Future};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use a3s_orm::{MigrationReport, PostgresExecutor};
use serde_json::Value;
use tokio::runtime::{Builder as TokioRuntimeBuilder, Handle};
use tokio::sync::{watch, Notify};
use tokio::task::JoinHandle;

use crate::{BootError, BoxFuture, ModuleRef, Result};

use super::{
    QueueBackend, QueueContext, QueueJob, QueueJobFailure, QueueJobInfo, QueueJobOptions,
    QueueJobReceipt, QueueOptions, QueueProcessor, QueueStats,
};

mod deduplication;
mod lifecycle;
mod migrations;
mod store;

use store::{ClaimedJob, PostgresQueueStore};

const RECOVERY_BATCH_SIZE: usize = 100;

/// A3S ORM-backed shared PostgreSQL queue backend.
///
/// The database URL should select a schema dedicated to Boot migrations. Boot,
/// Flow, and host applications keep separate ORM migration ledgers so one
/// component cannot silently accept another component's migration history.
#[derive(Clone)]
pub struct PostgresQueueBackend {
    state: Arc<PostgresQueueState>,
    options: QueueOptions,
}

impl fmt::Debug for PostgresQueueBackend {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        let processors = self
            .state
            .processors
            .read()
            .map(|processors| processors.len())
            .unwrap_or_default();
        let workers = self
            .state
            .lifecycle
            .lock()
            .map(|lifecycle| lifecycle.handles.len())
            .unwrap_or_default();
        formatter
            .debug_struct("PostgresQueueBackend")
            .field("queue_name", &self.state.store.queue_name())
            .field("options", &self.options)
            .field("processors", &processors)
            .field("workers", &workers)
            .finish_non_exhaustive()
    }
}

impl PostgresQueueBackend {
    /// Connect with an A3S ORM PostgreSQL executor and apply Boot migrations.
    pub async fn connect(
        database_url: impl AsRef<str>,
        queue_name: impl AsRef<str>,
        options: QueueOptions,
    ) -> Result<Self> {
        let store = PostgresQueueStore::connect(database_url.as_ref(), queue_name.as_ref()).await?;
        Ok(Self::from_store(store, options))
    }

    /// Connect and verify that the complete Boot queue schema was applied by
    /// a separate migrator, without acquiring DDL authority.
    pub async fn connect_verified(
        database_url: impl AsRef<str>,
        queue_name: impl AsRef<str>,
        options: QueueOptions,
    ) -> Result<Self> {
        let store =
            PostgresQueueStore::connect_verified(database_url.as_ref(), queue_name.as_ref())
                .await?;
        Ok(Self::from_store(store, options))
    }

    /// Use a configured A3S ORM executor and apply Boot migrations.
    pub async fn from_executor(
        executor: PostgresExecutor,
        queue_name: impl AsRef<str>,
        options: QueueOptions,
    ) -> Result<Self> {
        let store = PostgresQueueStore::from_executor(executor, queue_name.as_ref()).await?;
        Ok(Self::from_store(store, options))
    }

    /// Use a configured executor and verify the complete Boot queue schema
    /// without applying migrations.
    pub async fn from_executor_verified(
        executor: PostgresExecutor,
        queue_name: impl AsRef<str>,
        options: QueueOptions,
    ) -> Result<Self> {
        let store =
            PostgresQueueStore::from_executor_verified(executor, queue_name.as_ref()).await?;
        Ok(Self::from_store(store, options))
    }

    fn from_store(store: PostgresQueueStore, options: QueueOptions) -> Self {
        Self {
            state: Arc::new(PostgresQueueState {
                store,
                processors: RwLock::new(BTreeMap::new()),
                lifecycle: Mutex::new(PostgresQueueLifecycle::default()),
                last_worker_error: Mutex::new(None),
                notify: Notify::new(),
            }),
            options,
        }
    }

    pub fn queue_name(&self) -> &str {
        self.state.store.queue_name()
    }

    /// Return the most recent background worker failure, if one was observed.
    pub fn last_worker_error(&self) -> Result<Option<String>> {
        self.state
            .last_worker_error
            .lock()
            .map(|error| error.clone())
            .map_err(|_| {
                BootError::Internal("PostgreSQL queue worker error lock is poisoned".to_string())
            })
    }

    /// Return the current jobs without blocking the calling Tokio runtime.
    pub async fn jobs_async(&self) -> Result<Vec<QueueJobInfo>> {
        self.state.store.jobs().await
    }

    /// Return retained job failures without blocking the calling Tokio runtime.
    pub async fn failures_async(&self) -> Result<Vec<QueueJobFailure>> {
        self.state.store.failures().await
    }

    /// Return current queue counters without blocking the calling Tokio runtime.
    pub async fn stats_async(&self) -> Result<QueueStats> {
        self.state.store.stats().await
    }

    /// Remove every job owned by this queue without blocking the calling Tokio runtime.
    pub async fn clear_async(&self) -> Result<()> {
        let result = self.state.store.clear().await;
        self.state.notify.notify_waiters();
        result
    }
}

/// Apply the canonical Boot PostgreSQL queue migration set with a dedicated
/// migration executor.
pub async fn migrate_postgres_queue(executor: &PostgresExecutor) -> Result<MigrationReport> {
    store::migrate_schema(executor).await
}

impl QueueBackend for PostgresQueueBackend {
    fn enqueue(&self, name: String, data: Value) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        self.enqueue_with_options(name, data, QueueJobOptions::new())
    }

    fn enqueue_with_options(
        &self,
        name: String,
        data: Value,
        options: QueueJobOptions,
    ) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        let backend = self.clone();
        Box::pin(async move {
            let receipt = backend.state.store.enqueue(name, data, options).await?;
            backend.state.notify.notify_waiters();
            Ok(receipt)
        })
    }

    fn process(&self, name: String, processor: Arc<dyn QueueProcessor>) -> Result<()> {
        let name = name.trim().to_string();
        if name.is_empty() {
            return Err(BootError::BadRequest(
                "PostgreSQL queue processor name cannot be empty".to_string(),
            ));
        }
        let mut processors = self.state.write_processors()?;
        if processors.contains_key(&name) {
            return Err(BootError::Conflict(format!(
                "PostgreSQL queue processor is already registered: {name}"
            )));
        }
        processors.insert(name, processor);
        self.state.notify.notify_waiters();
        Ok(())
    }

    fn jobs(&self) -> Result<Vec<QueueJobInfo>> {
        let store = self.state.store.clone();
        postgres_sync_wait(async move { store.jobs().await })
    }

    fn failures(&self) -> Result<Vec<QueueJobFailure>> {
        let store = self.state.store.clone();
        postgres_sync_wait(async move { store.failures().await })
    }

    fn stats(&self) -> Result<QueueStats> {
        let store = self.state.store.clone();
        postgres_sync_wait(async move { store.stats().await })
    }

    fn clear(&self) -> Result<()> {
        let store = self.state.store.clone();
        let result = postgres_sync_wait(async move { store.clear().await });
        self.state.notify.notify_waiters();
        result
    }

    fn start(&self, queue_name: String, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let backend = self.clone();
        Box::pin(async move {
            backend.options.validate()?;
            if queue_name != backend.queue_name() {
                return Err(BootError::BadRequest(format!(
                    "Boot queue name {queue_name} does not match PostgreSQL queue {}",
                    backend.queue_name()
                )));
            }
            let runtime = Handle::try_current().map_err(|error| {
                BootError::Internal(format!(
                    "PostgreSQL queue requires a running Tokio runtime: {error}"
                ))
            })?;
            let mut lifecycle = backend.state.lock_lifecycle()?;
            if lifecycle.shutdown.is_some() {
                return Ok(());
            }
            let (shutdown, _) = watch::channel(false);
            for index in 0..backend.options.worker_count {
                let state = Arc::clone(&backend.state);
                let module_ref = module_ref.clone();
                let receiver = shutdown.subscribe();
                let options = backend.options;
                let worker_id = format!("{queue_name}-worker-{}", index + 1);
                lifecycle.handles.push(runtime.spawn(async move {
                    run_postgres_worker(state, module_ref, options, worker_id, receiver).await;
                }));
            }
            lifecycle.shutdown = Some(shutdown);
            drop(lifecycle);
            backend.state.notify.notify_waiters();
            Ok(())
        })
    }

    fn shutdown(&self) -> BoxFuture<'static, Result<()>> {
        let backend = self.clone();
        Box::pin(async move {
            let (shutdown, handles) = {
                let mut lifecycle = backend.state.lock_lifecycle()?;
                (
                    lifecycle.shutdown.take(),
                    std::mem::take(&mut lifecycle.handles),
                )
            };
            if let Some(shutdown) = shutdown {
                let _ = shutdown.send(true);
            }
            backend.state.notify.notify_waiters();
            for handle in handles {
                handle.await.map_err(|error| {
                    BootError::Internal(format!("PostgreSQL queue worker failed: {error}"))
                })?;
            }
            Ok(())
        })
    }
}

struct PostgresQueueState {
    store: PostgresQueueStore,
    processors: RwLock<BTreeMap<String, Arc<dyn QueueProcessor>>>,
    lifecycle: Mutex<PostgresQueueLifecycle>,
    last_worker_error: Mutex<Option<String>>,
    notify: Notify,
}

#[derive(Default)]
struct PostgresQueueLifecycle {
    shutdown: Option<watch::Sender<bool>>,
    handles: Vec<JoinHandle<()>>,
}

impl PostgresQueueState {
    fn write_processors(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, Arc<dyn QueueProcessor>>>> {
        self.processors.write().map_err(|_| {
            BootError::Internal("PostgreSQL queue processor registry lock is poisoned".to_string())
        })
    }

    fn processor_for(&self, name: &str) -> Result<Option<Arc<dyn QueueProcessor>>> {
        Ok(self
            .processors
            .read()
            .map_err(|_| {
                BootError::Internal(
                    "PostgreSQL queue processor registry lock is poisoned".to_string(),
                )
            })?
            .get(name)
            .map(Arc::clone))
    }

    fn lock_lifecycle(&self) -> Result<std::sync::MutexGuard<'_, PostgresQueueLifecycle>> {
        self.lifecycle.lock().map_err(|_| {
            BootError::Internal("PostgreSQL queue lifecycle lock is poisoned".to_string())
        })
    }

    fn record_worker_error(&self, error: &BootError) {
        if let Ok(mut last_error) = self.last_worker_error.lock() {
            *last_error = Some(error.to_string());
        }
    }
}

async fn run_postgres_worker(
    state: Arc<PostgresQueueState>,
    module_ref: ModuleRef,
    options: QueueOptions,
    worker_id: String,
    mut shutdown: watch::Receiver<bool>,
) {
    loop {
        if *shutdown.borrow() {
            return;
        }
        let result =
            run_postgres_worker_once(&state, &module_ref, options, &worker_id, &mut shutdown).await;
        if let Err(error) = result {
            state.record_worker_error(&error);
            wait_for_work(&state, options.poll_interval, &mut shutdown).await;
        }
    }
}

async fn run_postgres_worker_once(
    state: &Arc<PostgresQueueState>,
    module_ref: &ModuleRef,
    options: QueueOptions,
    worker_id: &str,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()> {
    state.store.recover_expired(RECOVERY_BATCH_SIZE).await?;
    let Some(job) = state.store.claim(worker_id, options.lease_duration).await? else {
        wait_for_work(state, options.poll_interval, shutdown).await;
        return Ok(());
    };
    let Some(processor) = state.processor_for(&job.name)? else {
        state.store.release(&job.id, &job.lock_token).await?;
        wait_for_work(state, options.poll_interval, shutdown).await;
        return Ok(());
    };
    process_claimed_job(state, module_ref, options, job, processor, shutdown).await
}

async fn process_claimed_job(
    state: &Arc<PostgresQueueState>,
    module_ref: &ModuleRef,
    options: QueueOptions,
    job: ClaimedJob,
    processor: Arc<dyn QueueProcessor>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<()> {
    let queue_job = QueueJob {
        id: job.id.clone(),
        name: job.name,
        data: job.payload,
    };
    let context = QueueContext {
        queue_name: state.store.queue_name().to_string(),
        module_ref: module_ref.clone(),
    };
    let processing = processor.process(queue_job, context);
    tokio::pin!(processing);
    let timeout = job.options.timeout;
    let timeout_future = async move {
        match timeout {
            Some(timeout) => tokio::time::sleep(timeout).await,
            None => pending::<()>().await,
        }
    };
    tokio::pin!(timeout_future);
    let heartbeat_every = heartbeat_interval(options.lease_duration);
    let mut heartbeat = tokio::time::interval_at(
        tokio::time::Instant::now() + heartbeat_every,
        heartbeat_every,
    );
    loop {
        tokio::select! {
            changed = shutdown.changed() => {
                let _ = changed;
                state.store.release(&job.id, &job.lock_token).await?;
                return Ok(());
            }
            result = &mut processing => {
                return match result {
                    Ok(()) => state.store.complete(&job.id, &job.lock_token).await,
                    Err(error) => state.store.fail(&job.id, &job.lock_token, error.to_string()).await,
                };
            }
            () = &mut timeout_future => {
                return state.store.fail(
                    &job.id,
                    &job.lock_token,
                    format!("queue processor timed out after {timeout:?}"),
                ).await;
            }
            _ = heartbeat.tick() => {
                state.store.heartbeat(&job.id, &job.lock_token, options.lease_duration).await?;
            }
        }
    }
}

async fn wait_for_work(
    state: &PostgresQueueState,
    poll_interval: Duration,
    shutdown: &mut watch::Receiver<bool>,
) {
    tokio::select! {
        _ = state.notify.notified() => {}
        _ = tokio::time::sleep(poll_interval) => {}
        _ = shutdown.changed() => {}
    }
}

fn heartbeat_interval(lease_duration: Duration) -> Duration {
    let interval = lease_duration / 3;
    if interval.is_zero() {
        Duration::from_millis(1)
    } else {
        interval
    }
}

fn postgres_sync_wait<T, Fut>(future: Fut) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
{
    if Handle::try_current().is_ok() {
        std::thread::spawn(move || run_on_postgres_runtime(future))
            .join()
            .map_err(|_| BootError::Internal("PostgreSQL queue runtime thread panicked".into()))?
    } else {
        run_on_postgres_runtime(future)
    }
}

fn run_on_postgres_runtime<T, Fut>(future: Fut) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    TokioRuntimeBuilder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|error| {
            BootError::Internal(format!(
                "could not create PostgreSQL queue runtime: {error}"
            ))
        })?
        .block_on(future)
}
