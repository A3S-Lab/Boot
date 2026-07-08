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

#[derive(Debug)]
struct PrefixedChildModule;

impl Module for PrefixedChildModule {
    fn name(&self) -> &'static str {
        "prefixed-child"
    }

    fn route_prefix(&self) -> Option<&str> {
        Some("/children")
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/cats")?
            .get("/{id}", |_| async {
                Ok(BootResponse::text("cat"))
            })?])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("child"))
        })?])
    }
}

#[derive(Debug)]
struct PrefixedParentModule;

impl Module for PrefixedParentModule {
    fn name(&self) -> &'static str {
        "prefixed-parent"
    }

    fn route_prefix(&self) -> Option<&str> {
        Some("/api")
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(PrefixedChildModule)]
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/parent", |_| async {
            Ok(BootResponse::text("parent"))
        })?])
    }
}

#[derive(Debug)]
struct InvalidRoutePrefixModule;

impl Module for InvalidRoutePrefixModule {
    fn name(&self) -> &'static str {
        "invalid-route-prefix"
    }

    fn route_prefix(&self) -> Option<&str> {
        Some("api")
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

#[tokio::test]
async fn module_route_prefixes_apply_to_nested_http_routes() {
    let app = BootApplication::builder()
        .global_prefix("/v1")
        .import(PrefixedParentModule)
        .build_async()
        .await
        .unwrap();

    assert!(app
        .routes()
        .iter()
        .any(|route| route.path() == "/v1/api/parent"));
    let child_route = app
        .routes()
        .iter()
        .find(|route| route.path() == "/v1/api/children/health")
        .unwrap();
    let controller_route = app
        .routes()
        .iter()
        .find(|route| route.path() == "/v1/api/children/cats/{id}")
        .unwrap();

    assert_eq!(child_route.module_name(), Some("prefixed-child"));
    assert_eq!(child_route.controller_prefix(), None);
    assert_eq!(controller_route.module_name(), Some("prefixed-child"));
    assert_eq!(controller_route.controller_prefix(), Some("/cats"));

    let response = app
        .call(a3s_boot::BootRequest::new(
            HttpMethod::Get,
            "/v1/api/children/cats/42",
        ))
        .await
        .unwrap();
    assert_eq!(response.body_text().unwrap(), "cat");
}

#[test]
fn dynamic_module_route_prefixes_apply_to_declared_routes() {
    let app = BootApplication::builder()
        .import(
            a3s_boot::DynamicModule::new("dynamic-routes")
                .route_prefix("/dynamic")
                .route(
                    RouteDefinition::get("/health", |_| async {
                        Ok(BootResponse::text("dynamic"))
                    })
                    .unwrap(),
                ),
        )
        .build()
        .unwrap();

    assert_eq!(app.routes()[0].path(), "/dynamic/health");
}

#[test]
fn module_route_prefixes_reject_relative_paths() {
    let result = BootApplication::builder()
        .import(InvalidRoutePrefixModule)
        .build();

    assert!(matches!(
        result,
        Err(BootError::InvalidRoutePath(path)) if path == "api"
    ));
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
