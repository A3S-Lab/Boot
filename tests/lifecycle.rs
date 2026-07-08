use a3s_boot::{
    BootApplication, BootError, BootFactory, BoxFuture, HttpAdapter, MessageTransport, Module,
    ModuleRef, ProviderDefinition, ProviderOnApplicationBootstrap, ProviderOnApplicationShutdown,
    ProviderOnModuleInit, Result,
};
use std::sync::Arc;

struct LifecycleModule {
    name: &'static str,
    imports: Vec<Arc<dyn Module>>,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl LifecycleModule {
    fn new(name: &'static str, log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self {
            name,
            imports: Vec::new(),
            log,
        }
    }

    fn with_import(mut self, module: Arc<dyn Module>) -> Self {
        self.imports.push(module);
        self
    }
}

impl Module for LifecycleModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        self.imports.clone()
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.log.lock().unwrap().push(format!("init:{}", self.name));
        Ok(())
    }

    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("bootstrap:{name}"));
            Ok(())
        })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push(format!("shutdown:{name}"));
            Ok(())
        })
    }
}

struct LifecycleProvider {
    name: &'static str,
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl ProviderOnModuleInit for LifecycleProvider {
    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.log
            .lock()
            .unwrap()
            .push(format!("provider-init:{}", self.name));
        Ok(())
    }
}

impl ProviderOnApplicationBootstrap for LifecycleProvider {
    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock()
                .unwrap()
                .push(format!("provider-bootstrap:{name}"));
            Ok(())
        })
    }
}

impl ProviderOnApplicationShutdown for LifecycleProvider {
    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let name = self.name;
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock()
                .unwrap()
                .push(format!("provider-shutdown:{name}"));
            Ok(())
        })
    }
}

struct ProviderLifecycleModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for ProviderLifecycleModule {
    fn name(&self) -> &'static str {
        "provider-lifecycle"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(LifecycleProvider {
            name: "service",
            log: Arc::clone(&self.log),
        })
        .with_on_module_init::<LifecycleProvider>()
        .with_on_application_bootstrap::<LifecycleProvider>()
        .with_on_application_shutdown::<LifecycleProvider>()])
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.log.lock().unwrap().push("module-init".to_string());
        Ok(())
    }

    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push("module-bootstrap".to_string());
            Ok(())
        })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push("module-shutdown".to_string());
            Ok(())
        })
    }
}

struct RequestScopedLifecycleModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for RequestScopedLifecycleModule {
    fn name(&self) -> &'static str {
        "request-scoped-lifecycle"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ProviderDefinition::request_scoped::<
            LifecycleProvider,
            _,
        >(move |_| {
            Ok(LifecycleProvider {
                name: "request",
                log: Arc::clone(&log),
            })
        })
        .with_on_module_init::<LifecycleProvider>()])
    }
}

struct FailingBootstrapModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
    fail_shutdown: bool,
}

impl Module for FailingBootstrapModule {
    fn name(&self) -> &'static str {
        "failing-bootstrap"
    }

    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push("bootstrap".to_string());
            Err(BootError::Internal("bootstrap failed".to_string()))
        })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        let fail_shutdown = self.fail_shutdown;
        Box::pin(async move {
            log.lock().unwrap().push("shutdown".to_string());
            if fail_shutdown {
                return Err(BootError::Internal("shutdown failed".to_string()));
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn singleton_provider_lifecycle_hooks_run_with_module_lifecycle() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(ProviderLifecycleModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    app.bootstrap().await.unwrap();
    app.shutdown().await.unwrap();

    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "provider-init:service",
            "module-init",
            "provider-bootstrap:service",
            "module-bootstrap",
            "module-shutdown",
            "provider-shutdown:service",
        ]
    );
}

#[test]
fn provider_lifecycle_hooks_require_singleton_scope() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let result = BootApplication::builder()
        .import(RequestScopedLifecycleModule { log })
        .build();

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message.contains("provider lifecycle hooks require singleton scope")
    ));
}

#[tokio::test]
async fn lifecycle_hooks_run_in_dependency_order_and_shutdown_reverse_order() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let child = Arc::new(LifecycleModule::new("child", Arc::clone(&log)));
    let root = LifecycleModule::new("root", Arc::clone(&log)).with_import(child);

    let app = BootApplication::builder().import(root).build().unwrap();

    app.bootstrap().await.unwrap();
    app.shutdown().await.unwrap();

    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "init:child",
            "init:root",
            "bootstrap:child",
            "bootstrap:root",
            "shutdown:root",
            "shutdown:child"
        ]
    );
}

#[tokio::test]
async fn serve_with_runs_shutdown_when_bootstrap_fails() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(FailingBootstrapModule {
            log: Arc::clone(&log),
            fail_shutdown: false,
        })
        .build()
        .unwrap();

    let result = app
        .serve_with(
            &LifecycleAdapter::new(Arc::clone(&log)),
            ([127, 0, 0, 1], 0).into(),
        )
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message)) if message == "bootstrap failed"
    ));
    assert_eq!(log.lock().unwrap().as_slice(), ["bootstrap", "shutdown"]);
}

#[tokio::test]
async fn serve_with_preserves_bootstrap_error_when_shutdown_also_fails() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(FailingBootstrapModule {
            log: Arc::clone(&log),
            fail_shutdown: true,
        })
        .build()
        .unwrap();

    let result = app
        .serve_with(
            &LifecycleAdapter::new(Arc::clone(&log)),
            ([127, 0, 0, 1], 0).into(),
        )
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message)) if message == "bootstrap failed"
    ));
    assert_eq!(log.lock().unwrap().as_slice(), ["bootstrap", "shutdown"]);
}

struct FailingShutdownModule {
    log: Arc<std::sync::Mutex<Vec<String>>>,
}

impl Module for FailingShutdownModule {
    fn name(&self) -> &'static str {
        "failing-shutdown"
    }

    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push("bootstrap".to_string());
            Ok(())
        })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let log = Arc::clone(&self.log);
        Box::pin(async move {
            log.lock().unwrap().push("shutdown".to_string());
            Err(BootError::Internal("shutdown failed".to_string()))
        })
    }
}

struct LifecycleAdapter {
    log: Arc<std::sync::Mutex<Vec<String>>>,
    fail: bool,
}

impl LifecycleAdapter {
    fn new(log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { log, fail: false }
    }

    fn failing(log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { log, fail: true }
    }
}

impl HttpAdapter for LifecycleAdapter {
    type Output = ();

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(())
    }

    fn serve(
        &self,
        _app: BootApplication,
        _addr: std::net::SocketAddr,
    ) -> BoxFuture<'static, Result<()>> {
        let fail = self.fail;
        let log = Arc::clone(&self.log);

        Box::pin(async move {
            log.lock().unwrap().push("serve".to_string());
            if fail {
                return Err(BootError::Adapter("serve failed".to_string()));
            }
            Ok(())
        })
    }
}

struct LifecycleTransport {
    log: Arc<std::sync::Mutex<Vec<String>>>,
    fail: bool,
}

impl LifecycleTransport {
    fn new(log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { log, fail: false }
    }

    fn failing(log: Arc<std::sync::Mutex<Vec<String>>>) -> Self {
        Self { log, fail: true }
    }
}

impl MessageTransport for LifecycleTransport {
    type Output = ();

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(())
    }

    fn serve(&self, _app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let fail = self.fail;
        let log = Arc::clone(&self.log);

        Box::pin(async move {
            log.lock().unwrap().push("microservice".to_string());
            if fail {
                return Err(BootError::Adapter("microservice failed".to_string()));
            }
            Ok(())
        })
    }
}

#[tokio::test]
async fn serve_with_runs_bootstrap_and_shutdown_around_adapter() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(LifecycleModule::new("app", Arc::clone(&log)))
        .build()
        .unwrap();

    app.serve_with(
        &LifecycleAdapter::new(Arc::clone(&log)),
        ([127, 0, 0, 1], 0).into(),
    )
    .await
    .unwrap();

    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "serve", "shutdown:app"]
    );
}

#[tokio::test]
async fn boot_factory_listen_runs_bootstrap_and_close() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut app = BootFactory::create(LifecycleModule::new("app", Arc::clone(&log))).unwrap();

    app.listen_with(
        &LifecycleAdapter::new(Arc::clone(&log)),
        ([127, 0, 0, 1], 0).into(),
    )
    .await
    .unwrap();

    assert!(!app.is_initialized());
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "serve", "shutdown:app"]
    );
}

#[tokio::test]
async fn boot_factory_application_context_init_and_close_are_idempotent() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut context =
        BootFactory::create_application_context(LifecycleModule::new("app", Arc::clone(&log)))
            .unwrap();

    context.init().await.unwrap();
    context.init().await.unwrap();
    assert!(context.is_initialized());
    context.close().await.unwrap();
    context.close().await.unwrap();

    assert!(!context.is_initialized());
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "shutdown:app"]
    );
}

#[tokio::test]
async fn boot_factory_connects_and_starts_microservices() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut app = BootFactory::create(LifecycleModule::new("app", Arc::clone(&log))).unwrap();

    let index = app.connect_microservice(LifecycleTransport::new(Arc::clone(&log)));
    assert_eq!(index, 0);
    assert_eq!(app.connected_microservice_count(), 1);

    app.start_all_microservices().await.unwrap();
    app.listen_with(
        &LifecycleAdapter::new(Arc::clone(&log)),
        ([127, 0, 0, 1], 0).into(),
    )
    .await
    .unwrap();

    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "init:app",
            "bootstrap:app",
            "microservice",
            "serve",
            "shutdown:app"
        ]
    );
}

#[tokio::test]
async fn boot_factory_create_microservice_listens_with_lifecycle_hooks() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut microservice = BootFactory::create_microservice(
        LifecycleModule::new("app", Arc::clone(&log)),
        LifecycleTransport::new(Arc::clone(&log)),
    )
    .unwrap();

    microservice.build_client().unwrap();
    microservice.listen().await.unwrap();

    assert!(!microservice.is_initialized());
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "microservice", "shutdown:app"]
    );
}

#[tokio::test]
async fn boot_factory_microservice_preserves_serve_errors_and_closes() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut microservice = BootFactory::create_microservice(
        LifecycleModule::new("app", Arc::clone(&log)),
        LifecycleTransport::failing(Arc::clone(&log)),
    )
    .unwrap();

    let result = microservice.listen().await;

    assert!(matches!(
        result,
        Err(BootError::Adapter(message)) if message == "microservice failed"
    ));
    assert!(!microservice.is_initialized());
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "microservice", "shutdown:app"]
    );
}

#[tokio::test]
async fn serve_with_runs_shutdown_when_adapter_fails() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(LifecycleModule::new("app", Arc::clone(&log)))
        .build()
        .unwrap();

    let result = app
        .serve_with(
            &LifecycleAdapter::failing(Arc::clone(&log)),
            ([127, 0, 0, 1], 0).into(),
        )
        .await;

    assert!(matches!(
        result,
        Err(BootError::Adapter(message)) if message == "serve failed"
    ));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["init:app", "bootstrap:app", "serve", "shutdown:app"]
    );
}

#[tokio::test]
async fn serve_with_preserves_adapter_error_when_shutdown_also_fails() {
    let log = Arc::new(std::sync::Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(FailingShutdownModule {
            log: Arc::clone(&log),
        })
        .build()
        .unwrap();

    let result = app
        .serve_with(
            &LifecycleAdapter::failing(Arc::clone(&log)),
            ([127, 0, 0, 1], 0).into(),
        )
        .await;

    assert!(matches!(
        result,
        Err(BootError::Adapter(message)) if message == "serve failed"
    ));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        ["bootstrap", "serve", "shutdown"]
    );
}
