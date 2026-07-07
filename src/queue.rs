use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use tokio::runtime::Handle;
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Queue runtime options shared by queue modules and in-process backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueOptions {
    pub worker_count: usize,
}

impl Default for QueueOptions {
    fn default() -> Self {
        Self { worker_count: 1 }
    }
}

impl QueueOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_worker_count(mut self, worker_count: usize) -> Self {
        self.worker_count = worker_count;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.worker_count == 0 {
            return Err(BootError::Internal(
                "queue worker count must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Job state tracked by the in-process queue backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueJobState {
    Pending,
    Active,
    Completed,
    Failed,
}

impl QueueJobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Active => "active",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

/// Queue job passed to processors.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueJob {
    pub id: String,
    pub name: String,
    pub data: Value,
}

impl QueueJob {
    pub fn data_as<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_value(self.data.clone()).map_err(|error| {
            BootError::BadRequest(format!("invalid queued job data for {}: {error}", self.id))
        })
    }
}

/// Receipt returned when a job is accepted by a queue backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueJobReceipt {
    pub id: String,
    pub name: String,
}

/// Public snapshot of a queued job.
#[derive(Debug, Clone, PartialEq)]
pub struct QueueJobInfo {
    pub id: String,
    pub name: String,
    pub state: QueueJobState,
    pub data: Value,
}

/// Failed job information captured by a queue backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueJobFailure {
    pub id: String,
    pub name: String,
    pub message: String,
}

/// Point-in-time queue counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueStats {
    pub pending: usize,
    pub active: usize,
    pub completed: usize,
    pub failed: usize,
}

/// Context passed to queue processors.
#[derive(Debug, Clone)]
pub struct QueueContext {
    pub queue_name: String,
    pub module_ref: ModuleRef,
}

/// Async processor for queued jobs.
pub trait QueueProcessor: Send + Sync + 'static {
    fn process(&self, job: QueueJob, context: QueueContext) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> QueueProcessor for F
where
    F: Fn(QueueJob, QueueContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn process(&self, job: QueueJob, context: QueueContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(job, context))
    }
}

/// Backend abstraction used by [`Queue`].
pub trait QueueBackend: Send + Sync + 'static {
    fn enqueue(&self, name: String, data: Value) -> BoxFuture<'static, Result<QueueJobReceipt>>;

    fn process(&self, name: String, processor: Arc<dyn QueueProcessor>) -> Result<()>;

    fn jobs(&self) -> Result<Vec<QueueJobInfo>>;

    fn failures(&self) -> Result<Vec<QueueJobFailure>>;

    fn stats(&self) -> Result<QueueStats>;

    fn clear(&self) -> Result<()>;

    fn start(&self, queue_name: String, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>>;

    fn shutdown(&self) -> BoxFuture<'static, Result<()>>;
}

/// Injectable queue facade exposed by [`QueueModule`].
#[derive(Clone)]
pub struct Queue {
    name: String,
    backend: Arc<dyn QueueBackend>,
}

impl fmt::Debug for Queue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Queue")
            .field("name", &self.name)
            .finish_non_exhaustive()
    }
}

impl Queue {
    pub fn new<B>(name: impl Into<String>, backend: B) -> Self
    where
        B: QueueBackend,
    {
        Self::from_backend_arc(name, Arc::new(backend))
    }

    pub fn from_backend_arc(name: impl Into<String>, backend: Arc<dyn QueueBackend>) -> Self {
        Self {
            name: name.into(),
            backend,
        }
    }

    pub fn in_process(name: impl Into<String>) -> Self {
        Self::new(name, InProcessQueueBackend::new())
    }

    pub fn in_process_with_options(name: impl Into<String>, options: QueueOptions) -> Self {
        Self::new(name, InProcessQueueBackend::with_options(options))
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn enqueue<T>(&self, name: impl Into<String>, data: &T) -> Result<QueueJobReceipt>
    where
        T: Serialize,
    {
        let data = serde_json::to_value(data).map_err(|error| {
            BootError::Internal(format!("failed to serialize queued job data: {error}"))
        })?;
        self.enqueue_value(name, data).await
    }

    pub async fn enqueue_value(
        &self,
        name: impl Into<String>,
        data: Value,
    ) -> Result<QueueJobReceipt> {
        self.backend.enqueue(name.into(), data).await
    }

    pub fn process<P>(&self, name: impl Into<String>, processor: P) -> Result<()>
    where
        P: QueueProcessor,
    {
        self.process_arc(name, Arc::new(processor))
    }

    pub fn process_arc(
        &self,
        name: impl Into<String>,
        processor: Arc<dyn QueueProcessor>,
    ) -> Result<()> {
        self.backend.process(name.into(), processor)
    }

    pub fn jobs(&self) -> Result<Vec<QueueJobInfo>> {
        self.backend.jobs()
    }

    pub fn failures(&self) -> Result<Vec<QueueJobFailure>> {
        self.backend.failures()
    }

    pub fn stats(&self) -> Result<QueueStats> {
        self.backend.stats()
    }

    pub fn clear(&self) -> Result<()> {
        self.backend.clear()
    }

    pub async fn start(&self, module_ref: ModuleRef) -> Result<()> {
        self.backend.start(self.name.clone(), module_ref).await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.backend.shutdown().await
    }
}

/// In-process queue backend suitable for tests and single-process services.
#[derive(Clone)]
pub struct InProcessQueueBackend {
    state: Arc<InProcessQueueState>,
    options: QueueOptions,
}

impl Default for InProcessQueueBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InProcessQueueBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let jobs = self.state.jobs.read().map(|jobs| jobs.len()).unwrap_or(0);
        let workers = self
            .state
            .handles
            .lock()
            .map(|handles| handles.len())
            .unwrap_or(0);
        f.debug_struct("InProcessQueueBackend")
            .field("options", &self.options)
            .field("jobs", &jobs)
            .field("workers", &workers)
            .finish()
    }
}

impl InProcessQueueBackend {
    pub fn new() -> Self {
        Self::with_options(QueueOptions::default())
    }

    pub fn with_options(options: QueueOptions) -> Self {
        Self {
            state: Arc::new(InProcessQueueState::default()),
            options,
        }
    }

    fn next_job_id(&self) -> String {
        let next = self.state.sequence.fetch_add(1, Ordering::SeqCst) + 1;
        format!("job-{next}")
    }
}

impl QueueBackend for InProcessQueueBackend {
    fn enqueue(&self, name: String, data: Value) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        let backend = self.clone();
        Box::pin(async move {
            let name = validate_job_name(name)?;
            let id = backend.next_job_id();
            let job = QueueJob {
                id: id.clone(),
                name: name.clone(),
                data,
            };

            backend.state.write_jobs()?.insert(
                id.clone(),
                QueueJobRecord {
                    job,
                    state: QueueJobState::Pending,
                },
            );
            backend.state.lock_pending()?.push_back(id.clone());
            backend.state.notify.notify_one();

            Ok(QueueJobReceipt { id, name })
        })
    }

    fn process(&self, name: String, processor: Arc<dyn QueueProcessor>) -> Result<()> {
        let name = validate_job_name(name)?;
        let mut processors = self.state.write_processors()?;
        if processors.contains_key(&name) {
            return Err(BootError::Internal(format!(
                "queue processor is already registered: {name}"
            )));
        }
        processors.insert(name, processor);
        self.state.notify.notify_waiters();
        Ok(())
    }

    fn jobs(&self) -> Result<Vec<QueueJobInfo>> {
        Ok(self
            .state
            .read_jobs()?
            .values()
            .map(QueueJobRecord::info)
            .collect())
    }

    fn failures(&self) -> Result<Vec<QueueJobFailure>> {
        Ok(self.state.read_failures()?.clone())
    }

    fn stats(&self) -> Result<QueueStats> {
        let mut stats = QueueStats::default();
        for record in self.state.read_jobs()?.values() {
            match record.state {
                QueueJobState::Pending => stats.pending += 1,
                QueueJobState::Active => stats.active += 1,
                QueueJobState::Completed => stats.completed += 1,
                QueueJobState::Failed => stats.failed += 1,
            }
        }
        Ok(stats)
    }

    fn clear(&self) -> Result<()> {
        self.state.write_jobs()?.clear();
        self.state.lock_pending()?.clear();
        self.state.write_failures()?.clear();
        Ok(())
    }

    fn start(&self, queue_name: String, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let backend = self.clone();
        Box::pin(async move {
            backend.options.validate()?;
            let runtime = Handle::try_current().map_err(|error| {
                BootError::Internal(format!("queue requires a running Tokio runtime: {error}"))
            })?;

            {
                let mut running = backend.state.lock_running_module_ref()?;
                if running.is_some() {
                    return Ok(());
                }
                *running = Some(module_ref.clone());
            }

            let mut handles = backend.state.lock_handles()?;
            for _ in 0..backend.options.worker_count {
                let state = Arc::clone(&backend.state);
                let worker_queue_name = queue_name.clone();
                let worker_module_ref = module_ref.clone();
                let handle = runtime.spawn(async move {
                    run_queue_worker(worker_queue_name, state, worker_module_ref).await;
                });
                handles.push(handle);
            }

            backend.state.notify.notify_waiters();
            Ok(())
        })
    }

    fn shutdown(&self) -> BoxFuture<'static, Result<()>> {
        let backend = self.clone();
        Box::pin(async move {
            *backend.state.lock_running_module_ref()? = None;
            let handles = std::mem::take(&mut *backend.state.lock_handles()?);
            for handle in handles {
                handle.abort();
                let _ = handle.await;
            }
            Ok(())
        })
    }
}

#[derive(Default)]
struct InProcessQueueState {
    jobs: RwLock<BTreeMap<String, QueueJobRecord>>,
    processors: RwLock<BTreeMap<String, Arc<dyn QueueProcessor>>>,
    pending: Mutex<VecDeque<String>>,
    failures: RwLock<Vec<QueueJobFailure>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    running_module_ref: Mutex<Option<ModuleRef>>,
    notify: Notify,
    sequence: AtomicU64,
}

impl InProcessQueueState {
    fn claim_next_job(&self) -> Result<Option<(QueueJob, Arc<dyn QueueProcessor>)>> {
        let processors = self.read_processors()?;
        let mut pending = self.lock_pending()?;
        let jobs = self.read_jobs()?;
        let Some((index, id, processor)) = pending.iter().enumerate().find_map(|(index, id)| {
            let record = jobs.get(id)?;
            let processor = processors.get(&record.job.name)?;
            Some((index, id.clone(), Arc::clone(processor)))
        }) else {
            return Ok(None);
        };
        drop(jobs);
        drop(processors);
        pending.remove(index);
        drop(pending);

        let mut jobs = self.write_jobs()?;
        let Some(record) = jobs.get_mut(&id) else {
            return Ok(None);
        };
        record.state = QueueJobState::Active;
        Ok(Some((record.job.clone(), processor)))
    }

    fn finish_job(&self, job: &QueueJob, result: Result<()>) {
        match result {
            Ok(()) => {
                if let Ok(mut jobs) = self.jobs.write() {
                    if let Some(record) = jobs.get_mut(&job.id) {
                        record.state = QueueJobState::Completed;
                    }
                }
            }
            Err(error) => {
                if let Ok(mut jobs) = self.jobs.write() {
                    if let Some(record) = jobs.get_mut(&job.id) {
                        record.state = QueueJobState::Failed;
                    }
                }
                if let Ok(mut failures) = self.failures.write() {
                    failures.push(QueueJobFailure {
                        id: job.id.clone(),
                        name: job.name.clone(),
                        message: error.to_string(),
                    });
                }
            }
        }
    }

    fn read_jobs(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, QueueJobRecord>>> {
        self.jobs
            .read()
            .map_err(|_| BootError::Internal("queue job registry lock is poisoned".to_string()))
    }

    fn write_jobs(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, QueueJobRecord>>> {
        self.jobs
            .write()
            .map_err(|_| BootError::Internal("queue job registry lock is poisoned".to_string()))
    }

    fn read_processors(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, Arc<dyn QueueProcessor>>>> {
        self.processors.read().map_err(|_| {
            BootError::Internal("queue processor registry lock is poisoned".to_string())
        })
    }

    fn write_processors(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, Arc<dyn QueueProcessor>>>> {
        self.processors.write().map_err(|_| {
            BootError::Internal("queue processor registry lock is poisoned".to_string())
        })
    }

    fn lock_pending(&self) -> Result<std::sync::MutexGuard<'_, VecDeque<String>>> {
        self.pending
            .lock()
            .map_err(|_| BootError::Internal("queue pending registry lock is poisoned".to_string()))
    }

    fn read_failures(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<QueueJobFailure>>> {
        self.failures
            .read()
            .map_err(|_| BootError::Internal("queue failure registry lock is poisoned".to_string()))
    }

    fn write_failures(&self) -> Result<std::sync::RwLockWriteGuard<'_, Vec<QueueJobFailure>>> {
        self.failures
            .write()
            .map_err(|_| BootError::Internal("queue failure registry lock is poisoned".to_string()))
    }

    fn lock_handles(&self) -> Result<std::sync::MutexGuard<'_, Vec<JoinHandle<()>>>> {
        self.handles
            .lock()
            .map_err(|_| BootError::Internal("queue worker registry lock is poisoned".to_string()))
    }

    fn lock_running_module_ref(&self) -> Result<std::sync::MutexGuard<'_, Option<ModuleRef>>> {
        self.running_module_ref.lock().map_err(|_| {
            BootError::Internal("queue lifecycle registry lock is poisoned".to_string())
        })
    }
}

#[derive(Debug, Clone)]
struct QueueJobRecord {
    job: QueueJob,
    state: QueueJobState,
}

impl QueueJobRecord {
    fn info(&self) -> QueueJobInfo {
        QueueJobInfo {
            id: self.job.id.clone(),
            name: self.job.name.clone(),
            state: self.state,
            data: self.job.data.clone(),
        }
    }
}

async fn run_queue_worker(
    queue_name: String,
    state: Arc<InProcessQueueState>,
    module_ref: ModuleRef,
) {
    loop {
        match state.claim_next_job() {
            Ok(Some((job, processor))) => {
                let context = QueueContext {
                    queue_name: queue_name.clone(),
                    module_ref: module_ref.clone(),
                };
                let result = processor.process(job.clone(), context).await;
                state.finish_job(&job, result);
            }
            Ok(None) => {
                state.notify.notified().await;
            }
            Err(error) => {
                let failure = QueueJobFailure {
                    id: "queue-worker".to_string(),
                    name: queue_name.clone(),
                    message: error.to_string(),
                };
                if let Ok(mut failures) = state.failures.write() {
                    failures.push(failure);
                }
                state.notify.notified().await;
            }
        }
    }
}

fn validate_job_name(name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() {
        return Err(BootError::Internal(
            "queue job name cannot be empty".to_string(),
        ));
    }
    Ok(name)
}

/// Module that registers and exports a [`Queue`] provider.
#[derive(Clone)]
pub struct QueueModule {
    name: &'static str,
    token: ProviderToken,
    queue: Arc<Queue>,
    processors: Vec<(String, Arc<dyn QueueProcessor>)>,
    global: bool,
}

impl fmt::Debug for QueueModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("queue", &self.queue)
            .field("processors", &self.processors.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl QueueModule {
    pub fn in_process(name: &'static str) -> Self {
        Self::from_queue(name, Queue::in_process(name))
    }

    pub fn in_process_with_options(name: &'static str, options: QueueOptions) -> Self {
        Self::from_queue(name, Queue::in_process_with_options(name, options))
    }

    pub fn from_queue(name: &'static str, queue: Queue) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Queue>(),
            queue: Arc::new(queue),
            processors: Vec::new(),
            global: false,
        }
    }

    pub fn processor<P>(mut self, name: impl Into<String>, processor: P) -> Self
    where
        P: QueueProcessor,
    {
        self.processors.push((name.into(), Arc::new(processor)));
        self
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for QueueModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.queue),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        for (name, processor) in &self.processors {
            self.queue
                .process_arc(name.clone(), Arc::clone(processor))?;
        }
        Ok(())
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move { queue.start(module_ref).await })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move { queue.shutdown().await })
    }
}
