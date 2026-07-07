use crate::{BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};
use chrono::Utc;
use cron::Schedule as CronSchedule;
use std::collections::BTreeMap;
use std::fmt;
use std::future::Future;
use std::str::FromStr;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

/// Schedule trigger used by a scheduled job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScheduleTrigger {
    Timeout(Duration),
    Interval(Duration),
    Cron(String),
}

impl ScheduleTrigger {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Timeout(_) => "timeout",
            Self::Interval(_) => "interval",
            Self::Cron(_) => "cron",
        }
    }
}

/// Context passed to scheduled job handlers.
#[derive(Debug, Clone)]
pub struct ScheduleContext {
    pub job_name: String,
    pub trigger: ScheduleTrigger,
    pub run_count: u64,
    pub module_ref: ModuleRef,
}

/// Handler for a scheduled job.
pub trait ScheduledTask: Send + Sync + 'static {
    fn run(&self, context: ScheduleContext) -> BoxFuture<'static, Result<()>>;
}

impl<F, Fut> ScheduledTask for F
where
    F: Fn(ScheduleContext) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    fn run(&self, context: ScheduleContext) -> BoxFuture<'static, Result<()>> {
        Box::pin(self(context))
    }
}

/// A named job that can be registered with a [`Scheduler`].
#[derive(Clone)]
pub struct ScheduledJob {
    name: String,
    trigger: ScheduleTrigger,
    task: Arc<dyn ScheduledTask>,
}

impl fmt::Debug for ScheduledJob {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduledJob")
            .field("name", &self.name)
            .field("trigger", &self.trigger)
            .finish_non_exhaustive()
    }
}

impl ScheduledJob {
    pub fn timeout<T>(name: impl Into<String>, delay: Duration, task: T) -> Self
    where
        T: ScheduledTask,
    {
        Self::new(name, ScheduleTrigger::Timeout(delay), task)
    }

    pub fn interval<T>(name: impl Into<String>, interval: Duration, task: T) -> Self
    where
        T: ScheduledTask,
    {
        Self::new(name, ScheduleTrigger::Interval(interval), task)
    }

    pub fn cron<T>(name: impl Into<String>, expression: impl Into<String>, task: T) -> Self
    where
        T: ScheduledTask,
    {
        Self::new(name, ScheduleTrigger::Cron(expression.into()), task)
    }

    pub fn new<T>(name: impl Into<String>, trigger: ScheduleTrigger, task: T) -> Self
    where
        T: ScheduledTask,
    {
        Self {
            name: name.into(),
            trigger,
            task: Arc::new(task),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn trigger(&self) -> &ScheduleTrigger {
        &self.trigger
    }

    pub fn info(&self) -> ScheduledJobInfo {
        ScheduledJobInfo {
            name: self.name.clone(),
            trigger: self.trigger.clone(),
        }
    }

    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::Internal(
                "scheduled job name cannot be empty".to_string(),
            ));
        }

        match &self.trigger {
            ScheduleTrigger::Timeout(delay) | ScheduleTrigger::Interval(delay) => {
                if delay.is_zero() {
                    return Err(BootError::Internal(format!(
                        "scheduled job `{}` duration must be greater than zero",
                        self.name
                    )));
                }
            }
            ScheduleTrigger::Cron(expression) => {
                parse_cron_expression(&self.name, expression)?;
            }
        }

        Ok(())
    }
}

/// Public snapshot of a registered scheduled job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJobInfo {
    pub name: String,
    pub trigger: ScheduleTrigger,
}

/// Error captured from a background scheduled job run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledJobError {
    pub job_name: String,
    pub message: String,
}

/// Backend abstraction used by [`Scheduler`].
pub trait SchedulerBackend: Send + Sync + 'static {
    fn schedule(&self, job: ScheduledJob) -> Result<()>;

    fn remove(&self, name: &str) -> Result<bool>;

    fn clear(&self) -> Result<()>;

    fn jobs(&self) -> Result<Vec<ScheduledJobInfo>>;

    fn errors(&self) -> Result<Vec<ScheduledJobError>>;

    fn start(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>>;

    fn shutdown(&self) -> BoxFuture<'static, Result<()>>;
}

/// Injectable scheduler facade exposed by [`ScheduleModule`].
#[derive(Clone)]
pub struct Scheduler {
    backend: Arc<dyn SchedulerBackend>,
}

impl fmt::Debug for Scheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Scheduler").finish_non_exhaustive()
    }
}

impl Scheduler {
    pub fn new<B>(backend: B) -> Self
    where
        B: SchedulerBackend,
    {
        Self::from_backend_arc(Arc::new(backend))
    }

    pub fn from_backend_arc(backend: Arc<dyn SchedulerBackend>) -> Self {
        Self { backend }
    }

    pub fn in_process() -> Self {
        Self::new(InProcessScheduler::new())
    }

    pub fn schedule(&self, job: ScheduledJob) -> Result<()> {
        self.backend.schedule(job)
    }

    pub fn timeout<T>(&self, name: impl Into<String>, delay: Duration, task: T) -> Result<()>
    where
        T: ScheduledTask,
    {
        self.schedule(ScheduledJob::timeout(name, delay, task))
    }

    pub fn interval<T>(&self, name: impl Into<String>, interval: Duration, task: T) -> Result<()>
    where
        T: ScheduledTask,
    {
        self.schedule(ScheduledJob::interval(name, interval, task))
    }

    pub fn cron<T>(
        &self,
        name: impl Into<String>,
        expression: impl Into<String>,
        task: T,
    ) -> Result<()>
    where
        T: ScheduledTask,
    {
        self.schedule(ScheduledJob::cron(name, expression, task))
    }

    pub fn remove(&self, name: &str) -> Result<bool> {
        self.backend.remove(name)
    }

    pub fn clear(&self) -> Result<()> {
        self.backend.clear()
    }

    pub fn jobs(&self) -> Result<Vec<ScheduledJobInfo>> {
        self.backend.jobs()
    }

    pub fn errors(&self) -> Result<Vec<ScheduledJobError>> {
        self.backend.errors()
    }

    pub async fn start(&self, module_ref: ModuleRef) -> Result<()> {
        self.backend.start(module_ref).await
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.backend.shutdown().await
    }
}

/// In-process scheduler backend suitable for tests and single-process services.
#[derive(Clone, Default)]
pub struct InProcessScheduler {
    state: Arc<InProcessSchedulerState>,
}

impl fmt::Debug for InProcessScheduler {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let jobs = self.state.jobs.read().map(|jobs| jobs.len()).unwrap_or(0);
        let handles = self
            .state
            .handles
            .lock()
            .map(|handles| handles.len())
            .unwrap_or(0);
        f.debug_struct("InProcessScheduler")
            .field("jobs", &jobs)
            .field("handles", &handles)
            .finish()
    }
}

impl InProcessScheduler {
    pub fn new() -> Self {
        Self::default()
    }

    fn spawn_job(&self, job: ScheduledJob, module_ref: ModuleRef) -> Result<()> {
        let runtime = Handle::try_current().map_err(|error| {
            BootError::Internal(format!(
                "scheduler requires a running Tokio runtime: {error}"
            ))
        })?;
        let name = job.name.clone();
        let mut handles = self.state.lock_handles()?;
        if handles.contains_key(&name) {
            return Ok(());
        }

        let state = Arc::clone(&self.state);
        let handle = runtime.spawn(async move {
            run_job_loop(job, module_ref, state).await;
        });
        handles.insert(name, handle);
        Ok(())
    }

    fn running_module_ref(&self) -> Result<Option<ModuleRef>> {
        Ok(self.state.lock_running_module_ref()?.clone())
    }
}

impl SchedulerBackend for InProcessScheduler {
    fn schedule(&self, job: ScheduledJob) -> Result<()> {
        job.validate()?;
        let name = job.name.clone();

        {
            let mut jobs = self.state.write_jobs()?;
            if jobs.contains_key(&name) {
                return Err(BootError::Internal(format!(
                    "scheduled job is already registered: {name}"
                )));
            }
            jobs.insert(name.clone(), job.clone());
        }

        if let Some(module_ref) = self.running_module_ref()? {
            if let Err(error) = self.spawn_job(job, module_ref) {
                let _ = self.state.write_jobs()?.remove(&name);
                return Err(error);
            }
        }

        Ok(())
    }

    fn remove(&self, name: &str) -> Result<bool> {
        let removed = self.state.write_jobs()?.remove(name).is_some();
        if let Some(handle) = self.state.lock_handles()?.remove(name) {
            handle.abort();
        }
        Ok(removed)
    }

    fn clear(&self) -> Result<()> {
        self.state.write_jobs()?.clear();
        let handles = std::mem::take(&mut *self.state.lock_handles()?);
        for handle in handles.into_values() {
            handle.abort();
        }
        Ok(())
    }

    fn jobs(&self) -> Result<Vec<ScheduledJobInfo>> {
        Ok(self
            .state
            .read_jobs()?
            .values()
            .map(ScheduledJob::info)
            .collect())
    }

    fn errors(&self) -> Result<Vec<ScheduledJobError>> {
        Ok(self.state.read_errors()?.clone())
    }

    fn start(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let scheduler = self.clone();
        Box::pin(async move {
            Handle::try_current().map_err(|error| {
                BootError::Internal(format!(
                    "scheduler requires a running Tokio runtime: {error}"
                ))
            })?;

            {
                let mut running = scheduler.state.lock_running_module_ref()?;
                if running.is_some() {
                    return Ok(());
                }
                *running = Some(module_ref.clone());
            }

            let jobs = scheduler
                .state
                .read_jobs()?
                .values()
                .cloned()
                .collect::<Vec<_>>();
            for job in jobs {
                scheduler.spawn_job(job, module_ref.clone())?;
            }

            Ok(())
        })
    }

    fn shutdown(&self) -> BoxFuture<'static, Result<()>> {
        let scheduler = self.clone();
        Box::pin(async move {
            *scheduler.state.lock_running_module_ref()? = None;
            let handles = std::mem::take(&mut *scheduler.state.lock_handles()?);
            for handle in handles.into_values() {
                handle.abort();
                let _ = handle.await;
            }
            Ok(())
        })
    }
}

#[derive(Default)]
struct InProcessSchedulerState {
    jobs: RwLock<BTreeMap<String, ScheduledJob>>,
    handles: Mutex<BTreeMap<String, JoinHandle<()>>>,
    running_module_ref: Mutex<Option<ModuleRef>>,
    errors: RwLock<Vec<ScheduledJobError>>,
}

impl InProcessSchedulerState {
    fn push_error(&self, job_name: &str, error: BootError) {
        if let Ok(mut errors) = self.errors.write() {
            errors.push(ScheduledJobError {
                job_name: job_name.to_string(),
                message: error.to_string(),
            });
        }
    }

    fn read_jobs(&self) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<String, ScheduledJob>>> {
        self.jobs
            .read()
            .map_err(|_| BootError::Internal("scheduler job registry lock is poisoned".to_string()))
    }

    fn write_jobs(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, ScheduledJob>>> {
        self.jobs
            .write()
            .map_err(|_| BootError::Internal("scheduler job registry lock is poisoned".to_string()))
    }

    fn read_errors(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<ScheduledJobError>>> {
        self.errors
            .read()
            .map_err(|_| BootError::Internal("scheduler error log lock is poisoned".to_string()))
    }

    fn lock_handles(&self) -> Result<std::sync::MutexGuard<'_, BTreeMap<String, JoinHandle<()>>>> {
        self.handles.lock().map_err(|_| {
            BootError::Internal("scheduler handle registry lock is poisoned".to_string())
        })
    }

    fn lock_running_module_ref(&self) -> Result<std::sync::MutexGuard<'_, Option<ModuleRef>>> {
        self.running_module_ref.lock().map_err(|_| {
            BootError::Internal("scheduler lifecycle registry lock is poisoned".to_string())
        })
    }
}

async fn run_job_loop(
    job: ScheduledJob,
    module_ref: ModuleRef,
    state: Arc<InProcessSchedulerState>,
) {
    match job.trigger.clone() {
        ScheduleTrigger::Timeout(delay) => {
            tokio::time::sleep(delay).await;
            run_job_once(&job, module_ref, &state, 1).await;
        }
        ScheduleTrigger::Interval(interval) => {
            let mut run_count = 0;
            loop {
                tokio::time::sleep(interval).await;
                run_count += 1;
                run_job_once(&job, module_ref.clone(), &state, run_count).await;
            }
        }
        ScheduleTrigger::Cron(expression) => {
            run_cron_loop(job, module_ref, state, expression).await;
        }
    }
}

async fn run_cron_loop(
    job: ScheduledJob,
    module_ref: ModuleRef,
    state: Arc<InProcessSchedulerState>,
    expression: String,
) {
    let schedule = match parse_cron_expression(&job.name, &expression) {
        Ok(schedule) => schedule,
        Err(error) => {
            state.push_error(&job.name, error);
            return;
        }
    };

    let mut run_count = 0;
    loop {
        let now = Utc::now();
        let Some(next) = schedule.after(&now).next() else {
            return;
        };
        let delay = next
            .signed_duration_since(now)
            .to_std()
            .unwrap_or(Duration::ZERO);
        tokio::time::sleep(delay).await;
        run_count += 1;
        run_job_once(&job, module_ref.clone(), &state, run_count).await;
    }
}

async fn run_job_once(
    job: &ScheduledJob,
    module_ref: ModuleRef,
    state: &InProcessSchedulerState,
    run_count: u64,
) {
    let context = ScheduleContext {
        job_name: job.name.clone(),
        trigger: job.trigger.clone(),
        run_count,
        module_ref,
    };

    if let Err(error) = job.task.run(context).await {
        state.push_error(&job.name, error);
    }
}

fn parse_cron_expression(job_name: &str, expression: &str) -> Result<CronSchedule> {
    let expression = expression.trim();
    if expression.is_empty() {
        return Err(BootError::Internal(format!(
            "scheduled job `{job_name}` cron expression cannot be empty"
        )));
    }

    CronSchedule::from_str(expression).map_err(|error| {
        BootError::Internal(format!(
            "invalid cron expression for scheduled job `{job_name}`: {error}"
        ))
    })
}

/// Module that registers and exports a [`Scheduler`] provider.
#[derive(Clone)]
pub struct ScheduleModule {
    name: &'static str,
    token: ProviderToken,
    scheduler: Arc<Scheduler>,
    jobs: Vec<ScheduledJob>,
    global: bool,
}

impl fmt::Debug for ScheduleModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScheduleModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("jobs", &self.jobs.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl ScheduleModule {
    pub fn in_process(name: &'static str) -> Self {
        Self::from_scheduler(name, Scheduler::in_process())
    }

    pub fn from_scheduler(name: &'static str, scheduler: Scheduler) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Scheduler>(),
            scheduler: Arc::new(scheduler),
            jobs: Vec::new(),
            global: false,
        }
    }

    pub fn job(mut self, job: ScheduledJob) -> Self {
        self.jobs.push(job);
        self
    }

    pub fn timeout<T>(self, name: impl Into<String>, delay: Duration, task: T) -> Self
    where
        T: ScheduledTask,
    {
        self.job(ScheduledJob::timeout(name, delay, task))
    }

    pub fn interval<T>(self, name: impl Into<String>, interval: Duration, task: T) -> Self
    where
        T: ScheduledTask,
    {
        self.job(ScheduledJob::interval(name, interval, task))
    }

    pub fn cron<T>(self, name: impl Into<String>, expression: impl Into<String>, task: T) -> Self
    where
        T: ScheduledTask,
    {
        self.job(ScheduledJob::cron(name, expression, task))
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

impl Module for ScheduleModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.scheduler),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        for job in &self.jobs {
            self.scheduler.schedule(job.clone())?;
        }
        Ok(())
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let scheduler = Arc::clone(&self.scheduler);
        Box::pin(async move { scheduler.start(module_ref).await })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let scheduler = Arc::clone(&self.scheduler);
        Box::pin(async move { scheduler.shutdown().await })
    }
}
