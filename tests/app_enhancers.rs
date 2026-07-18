use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, BoxFuture, CallHandler, ExceptionFilter,
    ExecutionContext, FromModuleRef, Guard, HttpMethod, Interceptor, Module, ModuleRef, Pipe,
    ProviderDefinition, ProviderDependency, Result, RouteDefinition, TestingModule,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct RequestTrace {
    id: usize,
    events: Arc<Mutex<Vec<String>>>,
}

impl RequestTrace {
    fn record(&self, stage: &str) {
        self.events
            .lock()
            .unwrap()
            .push(format!("{stage}:{}", self.id));
    }
}

#[derive(Debug)]
struct ProviderAppGuard {
    trace: Arc<RequestTrace>,
}

impl FromModuleRef for ProviderAppGuard {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            trace: module_ref.get::<RequestTrace>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<RequestTrace>()])
    }
}

impl Guard for ProviderAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.trace.record("guard");
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct ProviderAppPipe {
    trace: Arc<RequestTrace>,
}

impl FromModuleRef for ProviderAppPipe {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            trace: module_ref.get::<RequestTrace>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<RequestTrace>()])
    }
}

impl Pipe for ProviderAppPipe {
    fn transform(&self, request: BootRequest) -> BoxFuture<'static, Result<BootRequest>> {
        self.trace.record("pipe");
        Box::pin(async move { Ok(request) })
    }
}

#[derive(Debug)]
struct ProviderAppInterceptor {
    trace: Arc<RequestTrace>,
}

impl FromModuleRef for ProviderAppInterceptor {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            trace: module_ref.get::<RequestTrace>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<RequestTrace>()])
    }
}

impl Interceptor for ProviderAppInterceptor {
    fn intercept<'a>(
        &'a self,
        context: ExecutionContext,
        next: CallHandler<'a>,
    ) -> BoxFuture<'a, Result<BootResponse>> {
        Box::pin(async move {
            self.trace.record("interceptor-before");
            let response = next.handle().await?;
            self.trace.record("interceptor-after");
            let _ = context;
            Ok(response)
        })
    }
}

#[derive(Debug)]
struct ProviderAppFilter {
    trace: Arc<RequestTrace>,
}

impl FromModuleRef for ProviderAppFilter {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            trace: module_ref.get::<RequestTrace>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<RequestTrace>()])
    }
}

impl ExceptionFilter for ProviderAppFilter {
    fn catch(
        &self,
        _context: ExecutionContext,
        error: BootError,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        self.trace.record("filter");
        let response = BootResponse::text(format!("filtered:{}:{error}", self.trace.id));
        Box::pin(async move { Ok(Some(response)) })
    }
}

#[derive(Debug)]
struct AppEnhancerModule {
    calls: Arc<AtomicUsize>,
    events: Arc<Mutex<Vec<String>>>,
}

impl Module for AppEnhancerModule {
    fn name(&self) -> &'static str {
        "app-enhancers"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        let events = Arc::clone(&self.events);
        Ok(vec![
            ProviderDefinition::request_scoped::<RequestTrace, _>(move |_| {
                Ok(RequestTrace {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                    events: Arc::clone(&events),
                })
            }),
            ProviderDefinition::app_guard::<ProviderAppGuard>(),
            ProviderDefinition::app_interceptor::<ProviderAppInterceptor>(),
            ProviderDefinition::app_pipe::<ProviderAppPipe>(),
            ProviderDefinition::app_filter::<ProviderAppFilter>(),
        ])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![
            RouteDefinition::get("/module", |request: BootRequest| async move {
                let trace = request.get::<RequestTrace>()?;
                trace.record("handler");
                Ok(BootResponse::text(format!("module:{}", trace.id)))
            })?,
            RouteDefinition::get("/boom", |request: BootRequest| async move {
                let trace = request.get::<RequestTrace>()?;
                trace.record("handler");
                Err(BootError::Internal("boom".to_string()))
            })?,
        ])
    }
}

#[derive(Debug)]
struct SiblingModule;

impl Module for SiblingModule {
    fn name(&self) -> &'static str {
        "app-enhancer-sibling"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/sibling", |_| async {
            Ok(BootResponse::text("sibling"))
        })?])
    }
}

fn take_events(events: &Arc<Mutex<Vec<String>>>) -> Vec<String> {
    std::mem::take(&mut *events.lock().unwrap())
}

#[tokio::test]
async fn provider_backed_http_app_enhancers_share_one_request_context() {
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let direct = RouteDefinition::get("/direct", |request: BootRequest| async move {
        let trace = request.get::<RequestTrace>()?;
        trace.record("handler");
        Ok(BootResponse::text(format!("direct:{}", trace.id)))
    })
    .unwrap();
    let app = BootApplication::builder()
        .route(direct)
        .import(AppEnhancerModule {
            calls: Arc::clone(&calls),
            events: Arc::clone(&events),
        })
        .import(SiblingModule)
        .build()
        .unwrap();

    let module = app
        .call(BootRequest::new(HttpMethod::Get, "/module"))
        .await
        .unwrap();
    assert_eq!(module.body_text().unwrap(), "module:1");
    assert_eq!(
        take_events(&events),
        [
            "guard:1",
            "interceptor-before:1",
            "pipe:1",
            "handler:1",
            "interceptor-after:1",
        ]
    );

    let direct = app
        .call(BootRequest::new(HttpMethod::Get, "/direct"))
        .await
        .unwrap();
    assert_eq!(direct.body_text().unwrap(), "direct:2");
    assert_eq!(
        take_events(&events),
        [
            "guard:2",
            "interceptor-before:2",
            "pipe:2",
            "handler:2",
            "interceptor-after:2",
        ]
    );

    let sibling = app
        .call(BootRequest::new(HttpMethod::Get, "/sibling"))
        .await
        .unwrap();
    assert_eq!(sibling.body_text().unwrap(), "sibling");
    assert_eq!(
        take_events(&events),
        [
            "guard:3",
            "interceptor-before:3",
            "pipe:3",
            "interceptor-after:3",
        ]
    );
    assert_eq!(calls.load(Ordering::SeqCst), 3);
}

#[tokio::test]
async fn provider_backed_app_filters_handle_pipeline_and_early_route_errors() {
    let calls = Arc::new(AtomicUsize::new(0));
    let events = Arc::new(Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(AppEnhancerModule {
            calls: Arc::clone(&calls),
            events: Arc::clone(&events),
        })
        .build()
        .unwrap();

    let handler_error = app
        .call(BootRequest::new(HttpMethod::Get, "/boom"))
        .await
        .unwrap();
    assert!(handler_error
        .body_text()
        .unwrap()
        .starts_with("filtered:1:"));
    assert_eq!(
        take_events(&events),
        [
            "guard:1",
            "interceptor-before:1",
            "pipe:1",
            "handler:1",
            "filter:1",
        ]
    );

    let method_error = app
        .call(BootRequest::new(HttpMethod::Post, "/module"))
        .await
        .unwrap();
    assert!(method_error.body_text().unwrap().starts_with("filtered:2:"));
    assert_eq!(take_events(&events), ["filter:2"]);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[derive(Debug)]
struct EnhancerOrderLog(Arc<Mutex<Vec<&'static str>>>);

#[derive(Debug)]
struct FirstDeclaredProviderGuard {
    log: Arc<EnhancerOrderLog>,
}

impl FromModuleRef for FirstDeclaredProviderGuard {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            log: module_ref.get::<EnhancerOrderLog>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<EnhancerOrderLog>()])
    }
}

impl Guard for FirstDeclaredProviderGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.log.0.lock().unwrap().push("provider-first");
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct SecondDeclaredProviderGuard {
    log: Arc<EnhancerOrderLog>,
}

impl FromModuleRef for SecondDeclaredProviderGuard {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            log: module_ref.get::<EnhancerOrderLog>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![ProviderDependency::typed::<EnhancerOrderLog>()])
    }
}

impl Guard for SecondDeclaredProviderGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.log.0.lock().unwrap().push("provider-second");
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct EnhancerDeclarationOrderModule {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Module for EnhancerDeclarationOrderModule {
    fn name(&self) -> &'static str {
        "enhancer-declaration-order"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::singleton(EnhancerOrderLog(Arc::clone(&self.log))),
            ProviderDefinition::app_guard::<FirstDeclaredProviderGuard>(),
            ProviderDefinition::app_guard::<SecondDeclaredProviderGuard>(),
        ])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/enhancer-order", |_| async {
            Ok(BootResponse::text("ordered"))
        })?])
    }
}

#[tokio::test]
async fn provider_app_enhancers_follow_builder_globals_in_declaration_order() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let builder_log = Arc::clone(&log);
    let app = BootApplication::builder()
        .use_global_guard(move |_| {
            let builder_log = Arc::clone(&builder_log);
            async move {
                builder_log.lock().unwrap().push("builder-global");
                Ok(true)
            }
        })
        .import(EnhancerDeclarationOrderModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/enhancer-order"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "ordered");
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["builder-global", "provider-first", "provider-second"]
    );
}

#[derive(Debug)]
struct NamedFactoryMarkerGuard {
    activations: Arc<AtomicUsize>,
}

impl Guard for NamedFactoryMarkerGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct NamedFactoryMarkerModule {
    factory_calls: Arc<AtomicUsize>,
    activations: Arc<AtomicUsize>,
}

impl Module for NamedFactoryMarkerModule {
    fn name(&self) -> &'static str {
        "named-factory-marker"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let factory_calls = Arc::clone(&self.factory_calls);
        let activations = Arc::clone(&self.activations);
        Ok(vec![ProviderDefinition::named_factory::<
            NamedFactoryMarkerGuard,
            _,
        >("named-factory-app-guard", move |_| {
            factory_calls.fetch_add(1, Ordering::SeqCst);
            Ok(NamedFactoryMarkerGuard {
                activations: Arc::clone(&activations),
            })
        })
        .with_app_guard::<NamedFactoryMarkerGuard>()])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/named-factory-marker",
            |_| async { Ok(BootResponse::text("named")) },
        )?])
    }
}

#[tokio::test]
async fn named_custom_factory_marker_resolves_provider_backed_app_guard() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(NamedFactoryMarkerModule {
            factory_calls: Arc::clone(&factory_calls),
            activations: Arc::clone(&activations),
        })
        .build()
        .unwrap();

    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/named-factory-marker"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "named");
    assert_eq!(factory_calls.load(Ordering::SeqCst), 1);
    assert_eq!(activations.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct TestingProviderBackedDenyGuard;

impl FromModuleRef for TestingProviderBackedDenyGuard {
    fn from_module_ref(_module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self)
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(Vec::new())
    }
}

impl Guard for TestingProviderBackedDenyGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(false) })
    }
}

#[derive(Debug)]
struct TestingProviderBackedAllowGuard {
    calls: Arc<AtomicUsize>,
}

impl Guard for TestingProviderBackedAllowGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct TestingProviderBackedGuardModule;

impl Module for TestingProviderBackedGuardModule {
    fn name(&self) -> &'static str {
        "testing-provider-backed-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::app_guard::<
            TestingProviderBackedDenyGuard,
        >()])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/testing-provider-guard",
            |_| async { Ok(BootResponse::text("allowed")) },
        )?])
    }
}

#[tokio::test]
async fn testing_module_override_guard_replaces_provider_backed_component() {
    let replacement_calls = Arc::new(AtomicUsize::new(0));
    let testing = TestingModule::builder()
        .import(TestingProviderBackedGuardModule)
        .override_guard::<TestingProviderBackedDenyGuard, _>(TestingProviderBackedAllowGuard {
            calls: Arc::clone(&replacement_calls),
        })
        .compile()
        .unwrap();

    let response = testing
        .call(BootRequest::new(HttpMethod::Get, "/testing-provider-guard"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "allowed");
    assert_eq!(replacement_calls.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct MarkerPreservingOverrideGuard {
    allow: bool,
    activations: Arc<AtomicUsize>,
}

impl Guard for MarkerPreservingOverrideGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        let allow = self.allow;
        Box::pin(async move { Ok(allow) })
    }
}

#[derive(Debug)]
struct MarkerPreservingOverrideModule {
    original_factory_calls: Arc<AtomicUsize>,
    original_activations: Arc<AtomicUsize>,
}

impl Module for MarkerPreservingOverrideModule {
    fn name(&self) -> &'static str {
        "marker-preserving-provider-override"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let original_factory_calls = Arc::clone(&self.original_factory_calls);
        let original_activations = Arc::clone(&self.original_activations);
        Ok(vec![ProviderDefinition::factory::<
            MarkerPreservingOverrideGuard,
            _,
        >(move |_| {
            original_factory_calls.fetch_add(1, Ordering::SeqCst);
            Ok(MarkerPreservingOverrideGuard {
                allow: false,
                activations: Arc::clone(&original_activations),
            })
        })
        .with_app_guard::<MarkerPreservingOverrideGuard>()])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/provider-marker-override",
            |_| async { Ok(BootResponse::text("overridden")) },
        )?])
    }
}

#[tokio::test]
async fn override_provider_retains_original_app_enhancer_marker() {
    let original_factory_calls = Arc::new(AtomicUsize::new(0));
    let original_activations = Arc::new(AtomicUsize::new(0));
    let replacement_activations = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(MarkerPreservingOverrideModule {
            original_factory_calls: Arc::clone(&original_factory_calls),
            original_activations: Arc::clone(&original_activations),
        })
        .override_provider(ProviderDefinition::singleton(
            MarkerPreservingOverrideGuard {
                allow: true,
                activations: Arc::clone(&replacement_activations),
            },
        ))
        .build()
        .unwrap();

    assert_eq!(original_factory_calls.load(Ordering::SeqCst), 0);

    let response = app
        .call(BootRequest::new(
            HttpMethod::Get,
            "/provider-marker-override",
        ))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "overridden");
    assert_eq!(original_factory_calls.load(Ordering::SeqCst), 0);
    assert_eq!(original_activations.load(Ordering::SeqCst), 0);
    assert_eq!(replacement_activations.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct LazyEnhancerPreflightProbe;

#[derive(Debug)]
struct LazyRejectedProviderGuard;

impl Guard for LazyRejectedProviderGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct LazyRejectedEnhancerModule {
    probe_factory_calls: Arc<AtomicUsize>,
    enhancer_factory_calls: Arc<AtomicUsize>,
}

impl Module for LazyRejectedEnhancerModule {
    fn name(&self) -> &'static str {
        "lazy-app-enhancer-preflight"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let probe_factory_calls = Arc::clone(&self.probe_factory_calls);
        let enhancer_factory_calls = Arc::clone(&self.enhancer_factory_calls);
        Ok(vec![
            ProviderDefinition::factory::<LazyEnhancerPreflightProbe, _>(move |_| {
                probe_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(LazyEnhancerPreflightProbe)
            }),
            ProviderDefinition::factory::<LazyRejectedProviderGuard, _>(move |_| {
                enhancer_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(LazyRejectedProviderGuard)
            })
            .with_app_guard::<LazyRejectedProviderGuard>(),
        ])
    }
}

#[test]
fn lazy_module_loader_rejects_app_enhancer_before_provider_factories_run() {
    let probe_factory_calls = Arc::new(AtomicUsize::new(0));
    let enhancer_factory_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder().build().unwrap();
    let loader = app.lazy_module_loader().unwrap();

    let error = loader
        .load(LazyRejectedEnhancerModule {
            probe_factory_calls: Arc::clone(&probe_factory_calls),
            enhancer_factory_calls: Arc::clone(&enhancer_factory_calls),
        })
        .unwrap_err();

    match error {
        BootError::Internal(message) => {
            assert!(message.contains("lazy-app-enhancer-preflight"));
            assert!(message.contains("APP_*"));
        }
        other => panic!("expected a lazy application-enhancer error, got {other}"),
    }
    assert_eq!(probe_factory_calls.load(Ordering::SeqCst), 0);
    assert_eq!(enhancer_factory_calls.load(Ordering::SeqCst), 0);
}
