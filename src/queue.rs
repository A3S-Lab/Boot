use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
use a3s_lane::{
    InMemoryJobQueue, Job, JobListOptions, JobQueueBackend as LaneJobQueueBackend,
    JobQueueStats as LaneJobQueueStats, JobState as LaneJobState, LaneError,
};
pub use a3s_lane::{
    JobOptions as QueueJobOptions, JobPriority as QueueJobPriority, RetryPolicy as QueueRetryPolicy,
};
use chrono::Utc;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::runtime::{Builder as TokioRuntimeBuilder, Handle};
use tokio::sync::Notify;
use tokio::task::JoinHandle;

/// Queue runtime options shared by queue modules and Lane-backed queue backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueOptions {
    pub worker_count: usize,
    pub poll_interval: Duration,
    pub lease_duration: Duration,
}

impl Default for QueueOptions {
    fn default() -> Self {
        Self {
            worker_count: 1,
            poll_interval: Duration::from_millis(250),
            lease_duration: Duration::from_secs(30),
        }
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

    pub fn with_poll_interval(mut self, poll_interval: Duration) -> Self {
        self.poll_interval = poll_interval;
        self
    }

    pub fn with_lease_duration(mut self, lease_duration: Duration) -> Self {
        self.lease_duration = lease_duration;
        self
    }

    fn validate(&self) -> Result<()> {
        if self.worker_count == 0 {
            return Err(BootError::Internal(
                "queue worker count must be greater than zero".to_string(),
            ));
        }
        if self.poll_interval.is_zero() {
            return Err(BootError::Internal(
                "queue poll interval must be greater than zero".to_string(),
            ));
        }
        if self.lease_duration.is_zero() {
            return Err(BootError::Internal(
                "queue lease duration must be greater than zero".to_string(),
            ));
        }
        Ok(())
    }
}

/// Job state exposed by Boot's queue facade.
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

    fn from_lane(state: LaneJobState) -> Self {
        match state {
            LaneJobState::Waiting | LaneJobState::Delayed | LaneJobState::WaitingChildren => {
                Self::Pending
            }
            LaneJobState::Active => Self::Active,
            LaneJobState::Completed => Self::Completed,
            LaneJobState::Failed => Self::Failed,
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

    fn from_lane(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            name: job.name.clone(),
            data: job.payload.clone(),
        }
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

impl QueueJobInfo {
    fn from_lane(job: Job) -> Self {
        Self {
            id: job.id,
            name: job.name,
            state: QueueJobState::from_lane(job.state),
            data: job.payload,
        }
    }
}

/// Failed job information captured by a queue backend.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueJobFailure {
    pub id: String,
    pub name: String,
    pub message: String,
}

impl QueueJobFailure {
    fn from_lane(job: Job) -> Self {
        Self {
            id: job.id,
            name: job.name,
            message: job
                .failed_reason
                .unwrap_or_else(|| "job failed without a retained reason".to_string()),
        }
    }
}

/// Point-in-time queue counters.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct QueueStats {
    pub pending: usize,
    pub active: usize,
    pub completed: usize,
    pub failed: usize,
}

impl From<LaneJobQueueStats> for QueueStats {
    fn from(stats: LaneJobQueueStats) -> Self {
        Self {
            pending: stats.waiting + stats.delayed + stats.waiting_children,
            active: stats.active,
            completed: stats.completed,
            failed: stats.failed,
        }
    }
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

    fn enqueue_with_options(
        &self,
        name: String,
        data: Value,
        options: QueueJobOptions,
    ) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        let _ = options;
        self.enqueue(name, data)
    }

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

    pub fn from_lane_backend_arc(
        name: impl Into<String>,
        backend: Arc<dyn LaneJobQueueBackend>,
    ) -> Self {
        let name = name.into();
        Self::new(
            name.clone(),
            InProcessQueueBackend::from_lane_backend_arc(backend),
        )
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
        self.enqueue_with_options(name, data, QueueJobOptions::new())
            .await
    }

    pub async fn enqueue_with_options<T>(
        &self,
        name: impl Into<String>,
        data: &T,
        options: QueueJobOptions,
    ) -> Result<QueueJobReceipt>
    where
        T: Serialize,
    {
        let data = serde_json::to_value(data).map_err(|error| {
            BootError::Internal(format!("failed to serialize queued job data: {error}"))
        })?;
        self.enqueue_value_with_options(name, data, options).await
    }

    pub async fn enqueue_value(
        &self,
        name: impl Into<String>,
        data: Value,
    ) -> Result<QueueJobReceipt> {
        self.enqueue_value_with_options(name, data, QueueJobOptions::new())
            .await
    }

    pub async fn enqueue_value_with_options(
        &self,
        name: impl Into<String>,
        data: Value,
        options: QueueJobOptions,
    ) -> Result<QueueJobReceipt> {
        self.backend
            .enqueue_with_options(name.into(), data, options)
            .await
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

/// Lane-backed queue backend suitable for tests and single-process services.
#[derive(Clone)]
pub struct InProcessQueueBackend {
    state: Arc<LaneQueueState>,
    options: QueueOptions,
}

impl Default for InProcessQueueBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for InProcessQueueBackend {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let processors = self
            .state
            .processors
            .read()
            .map(|processors| processors.len())
            .unwrap_or(0);
        let workers = self
            .state
            .handles
            .lock()
            .map(|handles| handles.len())
            .unwrap_or(0);
        f.debug_struct("InProcessQueueBackend")
            .field("engine", &"a3s-lane")
            .field("options", &self.options)
            .field("processors", &processors)
            .field("workers", &workers)
            .finish()
    }
}

impl InProcessQueueBackend {
    pub fn new() -> Self {
        Self::with_options(QueueOptions::default())
    }

    pub fn with_options(options: QueueOptions) -> Self {
        Self::from_lane_backend_arc_with_options(
            Arc::new(InMemoryJobQueue::new("boot-queue")),
            options,
        )
    }

    pub fn from_lane_backend<B>(backend: B) -> Self
    where
        B: LaneJobQueueBackend + 'static,
    {
        Self::from_lane_backend_arc(Arc::new(backend))
    }

    pub fn from_lane_backend_arc(backend: Arc<dyn LaneJobQueueBackend>) -> Self {
        Self::from_lane_backend_arc_with_options(backend, QueueOptions::default())
    }

    pub fn from_lane_backend_with_options<B>(backend: B, options: QueueOptions) -> Self
    where
        B: LaneJobQueueBackend + 'static,
    {
        Self::from_lane_backend_arc_with_options(Arc::new(backend), options)
    }

    pub fn from_lane_backend_arc_with_options(
        backend: Arc<dyn LaneJobQueueBackend>,
        options: QueueOptions,
    ) -> Self {
        Self {
            state: Arc::new(LaneQueueState::new(backend)),
            options,
        }
    }
}

impl QueueBackend for InProcessQueueBackend {
    fn enqueue(&self, name: String, data: Value) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        self.enqueue_with_options(name, data, QueueJobOptions::new())
    }

    fn enqueue_with_options(
        &self,
        name: String,
        data: Value,
        options: QueueJobOptions,
    ) -> BoxFuture<'static, Result<QueueJobReceipt>> {
        let backend = Arc::clone(&self.state.backend);
        let notify = self.state.notify.clone();
        Box::pin(async move {
            let name = validate_job_name(name)?;
            let job = backend
                .add_job(name, data, options)
                .await
                .map_err(lane_error)?;
            notify.notify_one();
            Ok(QueueJobReceipt {
                id: job.id,
                name: job.name,
            })
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
        let backend = Arc::clone(&self.state.backend);
        sync_wait(async move {
            let total = backend.stats().await.map_err(lane_error)?.total;
            let page = backend
                .list_jobs(JobListOptions::new().with_limit(total))
                .await
                .map_err(lane_error)?;
            Ok(page.jobs.into_iter().map(QueueJobInfo::from_lane).collect())
        })
    }

    fn failures(&self) -> Result<Vec<QueueJobFailure>> {
        let backend = Arc::clone(&self.state.backend);
        sync_wait(async move {
            let total = backend.stats().await.map_err(lane_error)?.failed;
            let page = backend
                .list_jobs(
                    JobListOptions::new()
                        .with_state(LaneJobState::Failed)
                        .with_limit(total),
                )
                .await
                .map_err(lane_error)?;
            Ok(page
                .jobs
                .into_iter()
                .map(QueueJobFailure::from_lane)
                .collect())
        })
    }

    fn stats(&self) -> Result<QueueStats> {
        let backend = Arc::clone(&self.state.backend);
        sync_wait(async move {
            let stats = backend.stats().await.map_err(lane_error)?;
            Ok(QueueStats::from(stats))
        })
    }

    fn clear(&self) -> Result<()> {
        let backend = Arc::clone(&self.state.backend);
        let notify = self.state.notify.clone();
        sync_wait(async move {
            backend.obliterate(true).await.map_err(lane_error)?;
            notify.notify_waiters();
            Ok(())
        })
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
            for index in 0..backend.options.worker_count {
                let state = Arc::clone(&backend.state);
                let worker_queue_name = queue_name.clone();
                let worker_module_ref = module_ref.clone();
                let options = backend.options;
                let worker_id = format!("{worker_queue_name}-worker-{}", index + 1);
                let handle = runtime.spawn(async move {
                    run_queue_worker(
                        worker_queue_name,
                        state,
                        worker_module_ref,
                        options,
                        worker_id,
                    )
                    .await;
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
            backend.state.notify.notify_waiters();
            let handles = std::mem::take(&mut *backend.state.lock_handles()?);
            for handle in handles {
                let _ = handle.await;
            }
            Ok(())
        })
    }
}

struct LaneQueueState {
    backend: Arc<dyn LaneJobQueueBackend>,
    processors: RwLock<BTreeMap<String, Arc<dyn QueueProcessor>>>,
    handles: Mutex<Vec<JoinHandle<()>>>,
    running_module_ref: Mutex<Option<ModuleRef>>,
    notify: Arc<Notify>,
}

impl LaneQueueState {
    fn new(backend: Arc<dyn LaneJobQueueBackend>) -> Self {
        Self {
            backend,
            processors: RwLock::new(BTreeMap::new()),
            handles: Mutex::new(Vec::new()),
            running_module_ref: Mutex::new(None),
            notify: Arc::new(Notify::new()),
        }
    }

    fn processor_for(&self, name: &str) -> Result<Option<Arc<dyn QueueProcessor>>> {
        Ok(self.read_processors()?.get(name).map(Arc::clone))
    }

    fn has_processors(&self) -> bool {
        self.processors
            .read()
            .map(|processors| !processors.is_empty())
            .unwrap_or(false)
    }

    fn is_running(&self) -> bool {
        self.running_module_ref
            .lock()
            .map(|running| running.is_some())
            .unwrap_or(false)
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

async fn run_queue_worker(
    queue_name: String,
    state: Arc<LaneQueueState>,
    module_ref: ModuleRef,
    options: QueueOptions,
    worker_id: String,
) {
    while state.is_running() {
        if !state.has_processors() {
            wait_for_queue_signal(&state, options.poll_interval).await;
            continue;
        }

        if let Err(_error) =
            run_queue_worker_once(&queue_name, &state, &module_ref, options, &worker_id).await
        {
            wait_for_queue_signal(&state, options.poll_interval).await;
        }
    }
}

async fn run_queue_worker_once(
    queue_name: &str,
    state: &Arc<LaneQueueState>,
    module_ref: &ModuleRef,
    options: QueueOptions,
    worker_id: &str,
) -> Result<()> {
    state
        .backend
        .promote_due_jobs(Utc::now())
        .await
        .map_err(lane_error)?;

    let Some(job) = state
        .backend
        .claim_next(worker_id.to_string(), options.lease_duration, Utc::now())
        .await
        .map_err(lane_error)?
    else {
        wait_for_queue_signal(state, options.poll_interval).await;
        return Ok(());
    };

    process_claimed_job(queue_name, state, module_ref, options, job).await
}

async fn process_claimed_job(
    queue_name: &str,
    state: &Arc<LaneQueueState>,
    module_ref: &ModuleRef,
    options: QueueOptions,
    job: Job,
) -> Result<()> {
    let lock_token = job.lock_token.clone().ok_or_else(|| {
        BootError::Internal(format!("claimed queue job {} has no lock token", job.id))
    })?;

    let processor = match wait_for_processor(state, &job.name, options.poll_interval).await? {
        Some(processor) => processor,
        None => {
            state
                .backend
                .release_active_job(&job.id, &lock_token, Utc::now())
                .await
                .map_err(lane_error)?;
            return Ok(());
        }
    };

    let queue_job = QueueJob::from_lane(&job);
    let context = QueueContext {
        queue_name: queue_name.to_string(),
        module_ref: module_ref.clone(),
    };
    match processor.process(queue_job, context).await {
        Ok(()) => {
            state
                .backend
                .complete_job(&job.id, &lock_token, Value::Null, Utc::now())
                .await
                .map_err(lane_error)?;
        }
        Err(error) => {
            state
                .backend
                .fail_job_discarding_retry(&job.id, &lock_token, error.to_string(), Utc::now())
                .await
                .map_err(lane_error)?;
        }
    }
    Ok(())
}

async fn wait_for_processor(
    state: &Arc<LaneQueueState>,
    name: &str,
    poll_interval: Duration,
) -> Result<Option<Arc<dyn QueueProcessor>>> {
    loop {
        if let Some(processor) = state.processor_for(name)? {
            return Ok(Some(processor));
        }
        if !state.is_running() {
            return Ok(None);
        }
        wait_for_queue_signal(state, poll_interval).await;
    }
}

async fn wait_for_queue_signal(state: &Arc<LaneQueueState>, poll_interval: Duration) {
    tokio::select! {
        _ = state.notify.notified() => {}
        _ = tokio::time::sleep(poll_interval) => {}
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

fn lane_error(error: LaneError) -> BootError {
    BootError::Internal(format!("lane queue error: {error}"))
}

fn sync_wait<T, Fut>(future: Fut) -> Result<T>
where
    T: Send + 'static,
    Fut: Future<Output = Result<T>> + Send + 'static,
{
    if Handle::try_current().is_ok() {
        std::thread::spawn(move || run_on_new_runtime(future))
            .join()
            .map_err(|_| BootError::Internal("queue runtime thread panicked".to_string()))?
    } else {
        run_on_new_runtime(future)
    }
}

fn run_on_new_runtime<T, Fut>(future: Fut) -> Result<T>
where
    Fut: Future<Output = Result<T>>,
{
    let runtime = TokioRuntimeBuilder::new_current_thread()
        .enable_time()
        .build()
        .map_err(|error| BootError::Internal(format!("failed to create queue runtime: {error}")))?;
    runtime.block_on(future)
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

    pub fn from_lane_backend_arc(
        name: &'static str,
        backend: Arc<dyn LaneJobQueueBackend>,
    ) -> Self {
        Self::from_queue(name, Queue::from_lane_backend_arc(name, backend))
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
