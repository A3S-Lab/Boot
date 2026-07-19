use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, ExceptionFilter,
    ExecutionContext, FromModuleRef, Guard, HttpMethod, Module, ModuleRef, ProviderDefinition,
    ProviderDependency, ProviderOnModuleInit, Result, RouteDefinition,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct NotFoundRequestProbe {
    id: usize,
}

#[derive(Debug)]
struct ProviderNotFoundFilter {
    probe: Arc<NotFoundRequestProbe>,
}

impl FromModuleRef for ProviderNotFoundFilter {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            probe: module_ref.get::<NotFoundRequestProbe>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<NotFoundRequestProbe>()])
    }
}

impl ExceptionFilter for ProviderNotFoundFilter {
    fn catch(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        let has_context = context.request.context_id().is_some();
        let response =
            BootResponse::text(format!("not-found:{}:{has_context}:{error}", self.probe.id));
        Box::pin(async move { Ok(Some(response)) })
    }
}

#[derive(Debug)]
struct ProviderNotFoundFilterModule {
    probe_calls: Arc<AtomicUsize>,
}

impl Module for ProviderNotFoundFilterModule {
    fn name(&self) -> &'static str {
        "provider-not-found-filter"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let probe_calls = Arc::clone(&self.probe_calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<NotFoundRequestProbe, _>(move |_| {
                Ok(NotFoundRequestProbe {
                    id: probe_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
            ProviderDefinition::app_filter::<ProviderNotFoundFilter>(),
        ])
    }
}

#[tokio::test]
async fn provider_app_filter_handles_unmatched_application_404_with_request_context() {
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(ProviderNotFoundFilterModule {
            probe_calls: Arc::clone(&probe_calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/missing"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/still-missing"))
        .await
        .unwrap();

    assert!(first.body_text().unwrap().starts_with("not-found:1:true:"));
    assert!(second.body_text().unwrap().starts_with("not-found:2:true:"));
    assert_eq!(probe_calls.load(Ordering::SeqCst), 2);
}

#[derive(Debug)]
struct FailingRequestGuard;

impl Guard for FailingRequestGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct ResolutionFilterProbe {
    id: usize,
}

#[derive(Debug)]
struct ResolutionFailureFilter {
    probe: Arc<ResolutionFilterProbe>,
}

impl FromModuleRef for ResolutionFailureFilter {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            probe: module_ref.get::<ResolutionFilterProbe>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<ResolutionFilterProbe>()])
    }
}

impl ExceptionFilter for ResolutionFailureFilter {
    fn catch(
        &self,
        _context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        let response = BootResponse::text(format!("resolved:{}:{error}", self.probe.id));
        Box::pin(async move { Ok(Some(response)) })
    }
}

#[derive(Debug)]
struct ResolutionFailureModule {
    guard_factory_calls: Arc<AtomicUsize>,
    probe_calls: Arc<AtomicUsize>,
}

impl Module for ResolutionFailureModule {
    fn name(&self) -> &'static str {
        "app-enhancer-resolution-failure"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let guard_factory_calls = Arc::clone(&self.guard_factory_calls);
        let probe_calls = Arc::clone(&self.probe_calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<FailingRequestGuard, _>(move |_| {
                guard_factory_calls.fetch_add(1, Ordering::SeqCst);
                Err(BootError::Internal(
                    "request guard construction failed".to_string(),
                ))
            })
            .with_app_guard::<FailingRequestGuard>(),
            ProviderDefinition::request_scoped::<ResolutionFilterProbe, _>(move |_| {
                Ok(ResolutionFilterProbe {
                    id: probe_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
            ProviderDefinition::app_filter::<ResolutionFailureFilter>(),
        ])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/resolution-failure",
            |_| async { Ok(BootResponse::text("unreachable")) },
        )?])
    }
}

#[tokio::test]
async fn provider_filter_handles_request_scoped_app_enhancer_resolution_failure() {
    let guard_factory_calls = Arc::new(AtomicUsize::new(0));
    let probe_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(ResolutionFailureModule {
            guard_factory_calls: Arc::clone(&guard_factory_calls),
            probe_calls: Arc::clone(&probe_calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/resolution-failure"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/resolution-failure"))
        .await
        .unwrap();

    assert!(first.body_text().unwrap().starts_with("resolved:1:"));
    assert!(second.body_text().unwrap().starts_with("resolved:2:"));
    assert_eq!(guard_factory_calls.load(Ordering::SeqCst), 2);
    assert_eq!(probe_calls.load(Ordering::SeqCst), 2);
}

#[derive(Debug)]
struct WrongEnhancerValue;

#[derive(Debug)]
struct ExpectedEnhancerGuard;

impl Guard for ExpectedEnhancerGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct MismatchedEnhancerMarkerModule;

impl Module for MismatchedEnhancerMarkerModule {
    fn name(&self) -> &'static str {
        "mismatched-app-enhancer-marker"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_singleton(
            "mismatched-app-guard",
            WrongEnhancerValue,
        )
        .with_app_guard::<ExpectedEnhancerGuard>()])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/mismatched-app-guard",
            |_| async { Ok(BootResponse::text("unreachable")) },
        )?])
    }
}

#[tokio::test]
async fn mismatched_app_enhancer_marker_reports_provider_type_at_resolution() {
    let app = BootApplication::builder()
        .import(MismatchedEnhancerMarkerModule)
        .build()
        .unwrap();

    let error = app
        .call(BootRequest::new(HttpMethod::Get, "/mismatched-app-guard"))
        .await
        .unwrap_err();

    assert!(matches!(
        error,
        BootError::ProviderTypeMismatch(provider)
            if provider == "mismatched-app-guard"
    ));
}

#[derive(Debug)]
struct AsyncSingletonAppGuard {
    activations: Arc<AtomicUsize>,
}

impl Guard for AsyncSingletonAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct AsyncSingletonAppGuardModule {
    factory_calls: Arc<AtomicUsize>,
    activations: Arc<AtomicUsize>,
}

impl Module for AsyncSingletonAppGuardModule {
    fn name(&self) -> &'static str {
        "async-singleton-app-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let factory_calls = Arc::clone(&self.factory_calls);
        let activations = Arc::clone(&self.activations);
        Ok(vec![ProviderDefinition::async_factory::<
            AsyncSingletonAppGuard,
            _,
            _,
        >(move |_| {
            let factory_calls = Arc::clone(&factory_calls);
            let activations = Arc::clone(&activations);
            async move {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(AsyncSingletonAppGuard { activations })
            }
        })
        .with_dependencies(Vec::new())
        .with_app_guard::<AsyncSingletonAppGuard>()])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/async-app-guard", |_| async {
            Ok(BootResponse::text("allowed"))
        })?])
    }
}

#[tokio::test]
async fn build_async_supports_static_async_provider_app_enhancers() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(AsyncSingletonAppGuardModule {
            factory_calls: Arc::clone(&factory_calls),
            activations: Arc::clone(&activations),
        })
        .build_async()
        .await
        .unwrap();

    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
    for _ in 0..2 {
        let response = app
            .call(BootRequest::new(HttpMethod::Get, "/async-app-guard"))
            .await
            .unwrap();
        assert_eq!(response.body_text().unwrap(), "allowed");
    }
    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
    assert_eq!(activations.load(Ordering::SeqCst), 2);
}

#[derive(Debug)]
struct ContextualDependency;

#[derive(Debug)]
struct ContextualAsyncAppGuard {
    _dependency: Arc<ContextualDependency>,
}

impl Guard for ContextualAsyncAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct ContextualAsyncAppGuardModule {
    factory_calls: Arc<AtomicUsize>,
}

impl Module for ContextualAsyncAppGuardModule {
    fn name(&self) -> &'static str {
        "contextual-async-app-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let factory_calls = Arc::clone(&self.factory_calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ContextualDependency, _>(|_| {
                Ok(ContextualDependency)
            }),
            ProviderDefinition::async_factory::<ContextualAsyncAppGuard, _, _>(move |module_ref| {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(ContextualAsyncAppGuard {
                        _dependency: module_ref.get::<ContextualDependency>()?,
                    })
                }
            })
            .depends_on::<ContextualDependency>()
            .with_app_guard::<ContextualAsyncAppGuard>(),
        ])
    }
}

#[tokio::test]
async fn contextual_async_app_enhancer_is_rejected_before_factory_invocation() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let result = BootApplication::builder()
        .import(ContextualAsyncAppGuardModule {
            factory_calls: Arc::clone(&factory_calls),
        })
        .build_async()
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message.contains("ContextualAsyncAppGuard")
                && message.contains("cannot depend on a request-context provider")
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}

#[derive(Debug)]
struct ContextualLifecycleAppGuard {
    _dependency: Arc<ContextualDependency>,
}

impl Guard for ContextualLifecycleAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

impl ProviderOnModuleInit for ContextualLifecycleAppGuard {}

#[derive(Debug)]
struct ContextualLifecycleAppGuardModule {
    factory_calls: Arc<AtomicUsize>,
}

impl Module for ContextualLifecycleAppGuardModule {
    fn name(&self) -> &'static str {
        "contextual-lifecycle-app-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let factory_calls = Arc::clone(&self.factory_calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ContextualDependency, _>(|_| {
                Ok(ContextualDependency)
            }),
            ProviderDefinition::factory::<ContextualLifecycleAppGuard, _>(move |module_ref| {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(ContextualLifecycleAppGuard {
                    _dependency: module_ref.get::<ContextualDependency>()?,
                })
            })
            .depends_on::<ContextualDependency>()
            .with_on_module_init::<ContextualLifecycleAppGuard>()
            .with_app_guard::<ContextualLifecycleAppGuard>(),
        ])
    }
}

#[test]
fn contextual_lifecycle_app_enhancer_is_rejected_before_factory_invocation() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let result = BootApplication::builder()
        .import(ContextualLifecycleAppGuardModule {
            factory_calls: Arc::clone(&factory_calls),
        })
        .build();

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message.contains("ContextualLifecycleAppGuard")
                && message.contains("singleton lifecycle hooks")
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}
