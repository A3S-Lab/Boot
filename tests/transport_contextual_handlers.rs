use a3s_boot::{
    BootApplication, BootError, BoxFuture, CallHandler, MessagePatternDefinition, Module,
    ModuleRef, ProviderDefinition, Result, TransportContext, TransportInterceptor,
    TransportMessage, TransportReply,
};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct ScopedMessageDependency {
    id: usize,
    context_id: u64,
}

#[derive(Debug)]
struct RetryOnce;

impl TransportInterceptor for RetryOnce {
    fn intercept<'a>(
        &'a self,
        context: TransportContext,
        next: CallHandler<'a, Option<TransportReply>>,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            let _ = context;
            match next.handle().await {
                Err(BootError::ServiceUnavailable(_)) => next.handle().await,
                result => result,
            }
        })
    }
}

#[derive(Debug)]
struct ContextualPatternModule {
    dependency_calls: Arc<AtomicUsize>,
    request_factory_calls: Arc<AtomicUsize>,
    request_events: Arc<Mutex<Vec<(usize, u64, usize)>>>,
    event_factory_calls: Arc<AtomicUsize>,
    event_events: Arc<Mutex<Vec<(usize, u64)>>>,
}

impl Module for ContextualPatternModule {
    fn name(&self) -> &'static str {
        "contextual-message-patterns"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let dependency_calls = Arc::clone(&self.dependency_calls);
        Ok(vec![ProviderDefinition::request_scoped::<
            ScopedMessageDependency,
            _,
        >(move |module_ref| {
            let context_id = module_ref
                .context_id()
                .ok_or_else(|| {
                    BootError::Internal(
                        "scoped message dependency was built without a ContextId".to_string(),
                    )
                })?
                .id();
            Ok(ScopedMessageDependency {
                id: dependency_calls.fetch_add(1, Ordering::SeqCst) + 1,
                context_id,
            })
        })])
    }

    fn message_patterns(&self, _module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        let request_factory_calls = Arc::clone(&self.request_factory_calls);
        let request_events = Arc::clone(&self.request_events);
        let request =
            MessagePatternDefinition::request_scoped("contextual.request", move |module_ref| {
                request_factory_calls.fetch_add(1, Ordering::SeqCst);
                let dependency = module_ref.get::<ScopedMessageDependency>()?;
                let attempts = Arc::new(AtomicUsize::new(0));
                let request_events = Arc::clone(&request_events);
                Ok(move |_message: TransportMessage| {
                    let dependency = Arc::clone(&dependency);
                    let attempts = Arc::clone(&attempts);
                    let request_events = Arc::clone(&request_events);
                    async move {
                        let attempt = attempts.fetch_add(1, Ordering::SeqCst) + 1;
                        request_events.lock().unwrap().push((
                            dependency.id,
                            dependency.context_id,
                            attempt,
                        ));
                        if attempt == 1 {
                            return Err(BootError::ServiceUnavailable(
                                "retry contextual handler".to_string(),
                            ));
                        }
                        Ok(TransportReply::text(format!(
                            "{}:{}:{attempt}",
                            dependency.id, dependency.context_id
                        )))
                    }
                })
            })?
            .with_interceptor(RetryOnce);

        let event_factory_calls = Arc::clone(&self.event_factory_calls);
        let event_events = Arc::clone(&self.event_events);
        let event =
            MessagePatternDefinition::event_scoped("contextual.event", move |module_ref| {
                event_factory_calls.fetch_add(1, Ordering::SeqCst);
                let dependency = module_ref.get::<ScopedMessageDependency>()?;
                let event_events = Arc::clone(&event_events);
                Ok(move |_message: TransportMessage| {
                    let dependency = Arc::clone(&dependency);
                    let event_events = Arc::clone(&event_events);
                    async move {
                        event_events
                            .lock()
                            .unwrap()
                            .push((dependency.id, dependency.context_id));
                        Ok(())
                    }
                })
            })?;

        Ok(vec![request, event])
    }
}

struct ContextualPatternHarness {
    app: BootApplication,
    dependency_calls: Arc<AtomicUsize>,
    request_factory_calls: Arc<AtomicUsize>,
    request_events: Arc<Mutex<Vec<(usize, u64, usize)>>>,
    event_factory_calls: Arc<AtomicUsize>,
    event_events: Arc<Mutex<Vec<(usize, u64)>>>,
}

fn contextual_pattern_app() -> ContextualPatternHarness {
    let dependency_calls = Arc::new(AtomicUsize::new(0));
    let request_factory_calls = Arc::new(AtomicUsize::new(0));
    let request_events = Arc::new(Mutex::new(Vec::new()));
    let event_factory_calls = Arc::new(AtomicUsize::new(0));
    let event_events = Arc::new(Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(ContextualPatternModule {
            dependency_calls: Arc::clone(&dependency_calls),
            request_factory_calls: Arc::clone(&request_factory_calls),
            request_events: Arc::clone(&request_events),
            event_factory_calls: Arc::clone(&event_factory_calls),
            event_events: Arc::clone(&event_events),
        })
        .build()
        .unwrap();
    ContextualPatternHarness {
        app,
        dependency_calls,
        request_factory_calls,
        request_events,
        event_factory_calls,
        event_events,
    }
}

#[tokio::test]
async fn request_scoped_pattern_reuses_one_handler_and_context_across_retry() {
    let harness = contextual_pattern_app();

    let first = harness
        .app
        .dispatch_message(TransportMessage::new("contextual.request", json!({})))
        .await
        .unwrap()
        .unwrap();
    let second = harness
        .app
        .dispatch_message(TransportMessage::new("contextual.request", json!({})))
        .await
        .unwrap()
        .unwrap();

    let events = harness.request_events.lock().unwrap();
    assert_eq!(events.len(), 4);
    assert_eq!(events[0], (1, events[0].1, 1));
    assert_eq!(events[1], (1, events[0].1, 2));
    assert_eq!(events[2], (2, events[2].1, 1));
    assert_eq!(events[3], (2, events[2].1, 2));
    assert_ne!(events[0].1, events[2].1);
    assert_eq!(first, TransportReply::text(format!("1:{}:2", events[0].1)));
    assert_eq!(second, TransportReply::text(format!("2:{}:2", events[2].1)));
    assert_eq!(harness.request_factory_calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.dependency_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn event_scoped_pattern_uses_a_fresh_private_dependency_per_message() {
    let harness = contextual_pattern_app();

    harness
        .app
        .emit_message(TransportMessage::new("contextual.event", json!({})))
        .await
        .unwrap();
    harness
        .app
        .emit_message(TransportMessage::new("contextual.event", json!({})))
        .await
        .unwrap();

    let events = harness.event_events.lock().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0, 1);
    assert_eq!(events[1].0, 2);
    assert_ne!(events[0].1, events[1].1);
    assert_eq!(harness.event_factory_calls.load(Ordering::SeqCst), 2);
    assert_eq!(harness.dependency_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn standalone_scoped_pattern_requires_a_module_context_before_factory_runs() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let observed_calls = Arc::clone(&factory_calls);
    let pattern =
        MessagePatternDefinition::request_scoped("standalone.contextual", move |_module_ref| {
            observed_calls.fetch_add(1, Ordering::SeqCst);
            Ok(|_message: TransportMessage| async { Ok(TransportReply::text("unexpected")) })
        })
        .unwrap();

    let error = pattern
        .dispatch(TransportMessage::new("standalone.contextual", json!({})))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::Internal(message)
            if message.contains("scoped") && message.contains("module context")
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}
