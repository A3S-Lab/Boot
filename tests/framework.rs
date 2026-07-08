use a3s_boot::{
    BootApplication, BootError, BootResponse, BoxFuture, ControllerDefinition, HttpAdapter,
    HttpMethod, Module, ModuleRef, Result, RouteDefinition,
};
use std::sync::Arc;

#[derive(Debug)]
struct HealthModule;

impl Module for HealthModule {
    fn name(&self) -> &'static str {
        "health"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("ok"))
        })?])
    }
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(HealthModule)]
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/", |_| async {
            Ok(BootResponse::text("hello"))
        })?])
    }
}

#[derive(Debug)]
struct CycleRootModule;

impl Module for CycleRootModule {
    fn name(&self) -> &'static str {
        "cycle-root"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(CycleFeatureModule)]
    }
}

#[derive(Debug)]
struct CycleFeatureModule;

impl Module for CycleFeatureModule {
    fn name(&self) -> &'static str {
        "cycle-feature"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(CycleRootModule)]
    }
}

#[test]
fn registers_imports_before_parent_modules() {
    let app = BootApplication::builder()
        .import(AppModule)
        .build()
        .unwrap();

    assert_eq!(app.module_names(), ["health", "app"]);
    assert_eq!(app.routes().len(), 2);
}

#[test]
fn deduplicates_imported_modules_by_name() {
    let health = Arc::new(HealthModule);
    let app = BootApplication::builder()
        .import_arc(health.clone())
        .import_arc(health)
        .build()
        .unwrap();

    assert_eq!(app.module_names(), ["health"]);
    assert_eq!(app.routes().len(), 1);
}

#[test]
fn module_import_cycles_return_contextual_errors() {
    let result = BootApplication::builder().import(CycleRootModule).build();

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message == "cyclic module import detected: cycle-root -> cycle-feature -> cycle-root"
    ));
}

#[tokio::test]
async fn async_module_import_cycles_return_contextual_errors() {
    let result = BootApplication::builder()
        .import(CycleRootModule)
        .build_async()
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message == "cyclic module import detected: cycle-root -> cycle-feature -> cycle-root"
    ));
}

#[test]
fn rejects_empty_module_names() {
    struct EmptyModule;

    impl Module for EmptyModule {
        fn name(&self) -> &'static str {
            ""
        }
    }

    let result = BootApplication::builder().import(EmptyModule).build();

    assert!(matches!(result, Err(BootError::EmptyModuleName)));
}

#[derive(Debug)]
struct MetadataModule;

impl Module for MetadataModule {
    fn name(&self) -> &'static str {
        "metadata"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/items")?
            .get("/{id}", |_| async {
                Ok(BootResponse::text("controller"))
            })?])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/module-health", |_| async {
            Ok(BootResponse::text("module"))
        })?])
    }
}

#[test]
fn exposes_route_module_and_controller_metadata() {
    let app = BootApplication::builder()
        .global_prefix("/api")
        .route(
            RouteDefinition::get("/shell-health", |_| async {
                Ok(BootResponse::text("shell"))
            })
            .unwrap(),
        )
        .import(MetadataModule)
        .build()
        .unwrap();

    let shell_route = app
        .routes()
        .iter()
        .find(|route| route.path() == "/api/shell-health")
        .unwrap();
    let module_route = app
        .routes()
        .iter()
        .find(|route| route.path() == "/api/module-health")
        .unwrap();
    let controller_route = app
        .routes()
        .iter()
        .find(|route| route.path() == "/api/items/{id}")
        .unwrap();

    assert_eq!(shell_route.module_name(), None);
    assert_eq!(shell_route.controller_prefix(), None);
    assert_eq!(module_route.module_name(), Some("metadata"));
    assert_eq!(module_route.controller_prefix(), None);
    assert_eq!(controller_route.module_name(), Some("metadata"));
    assert_eq!(controller_route.controller_prefix(), Some("/items"));
}

struct RecordingAdapter;

impl HttpAdapter for RecordingAdapter {
    type Output = Vec<(HttpMethod, String)>;

    fn build(&self, app: BootApplication) -> Result<Self::Output> {
        Ok(app
            .routes()
            .iter()
            .map(|route| (route.method(), route.path().to_string()))
            .collect())
    }

    fn serve(
        &self,
        app: BootApplication,
        _addr: std::net::SocketAddr,
    ) -> BoxFuture<'static, Result<()>> {
        let route_count = app.routes().len();
        Box::pin(async move {
            assert!(route_count > 0);
            Ok(())
        })
    }
}

#[test]
fn builds_with_a_custom_http_adapter() {
    let app = BootApplication::builder()
        .import(HealthModule)
        .build()
        .unwrap();

    let routes = app.into_adapter(&RecordingAdapter).unwrap();

    assert_eq!(routes, vec![(HttpMethod::Get, "/health".to_string())]);
}
