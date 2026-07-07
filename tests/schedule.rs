#![cfg(feature = "schedule")]

use a3s_boot::{
    BootApplication, BootError, BoxFuture, Module, ModuleRef, ProviderDefinition, Result,
    ScheduleContext, ScheduleModule, ScheduleTrigger, Scheduler,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
async fn schedule_module_runs_interval_jobs_until_shutdown() {
    let calls = Arc::new(AtomicUsize::new(0));
    let job_calls = Arc::clone(&calls);
    let app = BootApplication::builder()
        .import(ScheduleModule::in_process("schedule").interval(
            "heartbeat",
            Duration::from_millis(10),
            move |context: ScheduleContext| {
                let job_calls = Arc::clone(&job_calls);
                async move {
                    assert_eq!(context.job_name, "heartbeat");
                    assert_eq!(
                        context.trigger,
                        ScheduleTrigger::Interval(Duration::from_millis(10))
                    );
                    assert!(context.run_count > 0);
                    job_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        ))
        .build()
        .unwrap();

    app.bootstrap().await.unwrap();
    tokio::time::sleep(Duration::from_millis(45)).await;
    app.shutdown().await.unwrap();

    let after_shutdown = calls.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_millis(25)).await;

    assert!(after_shutdown >= 2);
    assert_eq!(calls.load(Ordering::SeqCst), after_shutdown);
}

#[tokio::test]
async fn schedule_module_runs_timeout_jobs_once() {
    let calls = Arc::new(AtomicUsize::new(0));
    let job_calls = Arc::clone(&calls);
    let app = BootApplication::builder()
        .import(ScheduleModule::in_process("schedule").timeout(
            "ready",
            Duration::from_millis(10),
            move |_| {
                let job_calls = Arc::clone(&job_calls);
                async move {
                    job_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                }
            },
        ))
        .build()
        .unwrap();

    app.bootstrap().await.unwrap();
    tokio::time::sleep(Duration::from_millis(40)).await;
    app.shutdown().await.unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct RefreshService {
    calls: Arc<AtomicUsize>,
}

impl RefreshService {
    fn refresh(&self) {
        self.calls.fetch_add(1, Ordering::SeqCst);
    }
}

#[derive(Debug)]
struct UsesSchedulerModule {
    schedule_module: ScheduleModule,
    calls: Arc<AtomicUsize>,
}

impl Module for UsesSchedulerModule {
    fn name(&self) -> &'static str {
        "uses-scheduler"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.schedule_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![ProviderDefinition::singleton(RefreshService {
            calls,
        })])
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let scheduler = module_ref.get::<Scheduler>()?;
            let refresh = module_ref.get::<RefreshService>()?;
            scheduler.interval(
                "refresh",
                Duration::from_millis(10),
                move |context: ScheduleContext| {
                    let refresh = Arc::clone(&refresh);
                    async move {
                        assert!(context.module_ref.contains_provider::<Scheduler>()?);
                        refresh.refresh();
                        Ok(())
                    }
                },
            )?;
            Ok(())
        })
    }
}

#[tokio::test]
async fn scheduler_provider_can_register_jobs_after_bootstrap_starts() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(UsesSchedulerModule {
            schedule_module: ScheduleModule::in_process("schedule"),
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    app.bootstrap().await.unwrap();
    tokio::time::sleep(Duration::from_millis(45)).await;
    app.shutdown().await.unwrap();

    assert!(calls.load(Ordering::SeqCst) >= 2);
}

#[test]
fn schedule_module_exports_named_and_global_scheduler_providers() {
    let named = BootApplication::builder()
        .import(ScheduleModule::in_process("named-schedule").named("app-scheduler"))
        .build()
        .unwrap();
    assert!(named.get_named::<Scheduler>("app-scheduler").is_ok());
    assert!(named.get_optional::<Scheduler>().unwrap().is_none());

    let global = BootApplication::builder()
        .import(ScheduleModule::in_process("global-schedule").global())
        .build()
        .unwrap();
    assert!(global.get::<Scheduler>().is_ok());
}

#[test]
fn scheduler_validates_registered_jobs_and_exposes_job_metadata() {
    let scheduler = Scheduler::in_process();
    scheduler
        .cron("nightly", "0 0 0 * * * *", |_| async { Ok(()) })
        .unwrap();

    let jobs = scheduler.jobs().unwrap();
    assert_eq!(jobs.len(), 1);
    assert_eq!(jobs[0].name, "nightly");
    assert_eq!(
        jobs[0].trigger,
        ScheduleTrigger::Cron("0 0 0 * * * *".to_string())
    );

    let error = scheduler
        .interval("bad", Duration::ZERO, |_| async { Ok(()) })
        .unwrap_err();
    assert!(matches!(error, BootError::Internal(message) if message.contains("duration")));
}

#[test]
fn schedule_module_rejects_invalid_cron_jobs_during_build() {
    let error = match BootApplication::builder()
        .import(ScheduleModule::in_process("schedule").cron("bad", "nope", |_| async { Ok(()) }))
        .build()
    {
        Ok(_) => panic!("invalid cron job should fail application build"),
        Err(error) => error,
    };

    assert!(matches!(error, BootError::Internal(message) if message.contains("invalid cron")));
}
