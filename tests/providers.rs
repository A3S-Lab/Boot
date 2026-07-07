use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition, ExecutionContext,
    HttpMethod, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct ItemsService;

impl ItemsService {
    fn find_all(&self) -> &'static str {
        "item-a,item-b"
    }
}

#[derive(Debug)]
struct ItemsModule;

impl Module for ItemsModule {
    fn name(&self) -> &'static str {
        "items"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(ItemsService)])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let items = module_ref.get::<ItemsService>()?;
        Ok(vec![ControllerDefinition::new("/items")?.get(
            "/",
            move |_| {
                let items = Arc::clone(&items);
                async move { Ok(BootResponse::text(items.find_all())) }
            },
        )?])
    }
}

#[derive(Debug)]
struct SharedConfig {
    value: &'static str,
}

#[derive(Debug)]
struct UsesSharedConfig {
    config: Arc<SharedConfig>,
}

#[derive(Debug)]
struct ArcFactoryModule;

impl Module for ArcFactoryModule {
    fn name(&self) -> &'static str {
        "arc-factory"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::factory_arc::<SharedConfig, _>(|_| {
                Ok(Arc::new(SharedConfig { value: "shared" }))
            }),
            ProviderDefinition::factory::<UsesSharedConfig, _>(|module_ref| {
                Ok(UsesSharedConfig {
                    config: module_ref.get::<SharedConfig>()?,
                })
            }),
            ProviderDefinition::named_factory_arc::<SharedConfig, _>("named-config", |_| {
                Ok(Arc::new(SharedConfig { value: "named" }))
            }),
        ])
    }
}

#[derive(Debug)]
struct DuplicateFactoryModule {
    calls: Arc<AtomicUsize>,
}

impl Module for DuplicateFactoryModule {
    fn name(&self) -> &'static str {
        "duplicate-factory"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::singleton(ItemsService),
            ProviderDefinition::factory::<ItemsService, _>(move |_| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(ItemsService)
            }),
        ])
    }
}

#[derive(Debug)]
struct DuplicateProviderChildModule;

impl Module for DuplicateProviderChildModule {
    fn name(&self) -> &'static str {
        "duplicate-provider-child"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(ItemsService)])
    }
}

#[derive(Debug)]
struct DuplicateProviderParentModule {
    init_calls: Arc<AtomicUsize>,
}

impl Module for DuplicateProviderParentModule {
    fn name(&self) -> &'static str {
        "duplicate-provider-parent"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(DuplicateProviderChildModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(ItemsService)])
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.init_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn module_ref_exposes_optional_lookup_and_presence_checks() {
    let module_ref = ModuleRef::new();
    let items_token = ProviderToken::of::<ItemsService>();

    assert!(module_ref.get_optional::<ItemsService>().unwrap().is_none());
    assert!(!module_ref.contains(&items_token).unwrap());
    assert!(!module_ref.contains_provider::<ItemsService>().unwrap());
    assert!(!module_ref.contains_named("named-config").unwrap());

    module_ref.insert(ItemsService).unwrap();
    module_ref
        .register(ProviderDefinition::named_singleton(
            "named-config",
            SharedConfig { value: "named" },
        ))
        .unwrap();

    let items = module_ref.get_optional::<ItemsService>().unwrap().unwrap();
    let named = module_ref
        .get_optional_named::<SharedConfig>("named-config")
        .unwrap()
        .unwrap();
    let tokens = module_ref.tokens().unwrap();

    assert_eq!(items.find_all(), "item-a,item-b");
    assert_eq!(named.value, "named");
    assert!(module_ref.contains(&items_token).unwrap());
    assert!(module_ref.contains_provider::<ItemsService>().unwrap());
    assert!(module_ref.contains_named("named-config").unwrap());
    assert!(tokens.contains(&items_token));
    assert!(tokens.contains(&ProviderToken::named("named-config")));
}

#[test]
fn optional_provider_lookup_preserves_type_mismatch_errors() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::named_singleton(
            "config",
            SharedConfig { value: "named" },
        ))
        .unwrap();

    let error = module_ref
        .get_optional_named::<ItemsService>("config")
        .unwrap_err();

    assert!(matches!(error, BootError::ProviderTypeMismatch(message) if message == "config"));
}

#[test]
fn duplicate_providers_across_imported_modules_stop_parent_initialization() {
    let init_calls = Arc::new(AtomicUsize::new(0));
    let result = BootApplication::builder()
        .import(DuplicateProviderParentModule {
            init_calls: Arc::clone(&init_calls),
        })
        .build();

    assert!(matches!(result, Err(BootError::DuplicateProvider(_))));
    assert_eq!(init_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn provider_factories_can_return_shared_arc_values() {
    let app = BootApplication::builder()
        .import(ArcFactoryModule)
        .build()
        .unwrap();

    let config = app.get::<SharedConfig>().unwrap();
    let dependent = app.get::<UsesSharedConfig>().unwrap();
    let named = app.get_named::<SharedConfig>("named-config").unwrap();

    assert_eq!(config.value, "shared");
    assert_eq!(dependent.config.value, "shared");
    assert!(Arc::ptr_eq(&config, &dependent.config));
    assert_eq!(named.value, "named");
}

#[test]
fn duplicate_provider_factories_are_rejected_before_execution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let result = BootApplication::builder()
        .import(DuplicateFactoryModule {
            calls: Arc::clone(&calls),
        })
        .build();

    assert!(matches!(result, Err(BootError::DuplicateProvider(_))));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
}

#[test]
fn application_exposes_optional_provider_lookup() {
    let app = BootApplication::builder()
        .import(ArcFactoryModule)
        .build()
        .unwrap();

    let config = app.get_optional::<SharedConfig>().unwrap();
    let missing = app.get_optional::<ItemsService>().unwrap();
    let named = app
        .get_optional_named::<SharedConfig>("named-config")
        .unwrap();
    let missing_named = app.get_optional_named::<ItemsService>("missing").unwrap();

    assert_eq!(config.unwrap().value, "shared");
    assert_eq!(named.unwrap().value, "named");
    assert!(missing.is_none());
    assert!(missing_named.is_none());
}

#[tokio::test]
async fn applies_global_prefix_to_controller_routes_without_changing_controller_context() {
    let app = BootApplication::builder()
        .global_prefix("/api/v1")
        .use_global_guard(|context: ExecutionContext| async move {
            Ok(context.controller_prefix.as_deref() == Some("/items"))
        })
        .import(ItemsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes()[0].path(), "/api/v1/items");

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/api/v1/items"))
        .await
        .unwrap();

    assert_eq!(response.body, b"item-a,item-b");
}

#[tokio::test]
async fn registers_controller_routes_with_provider_injection() {
    let app = BootApplication::builder()
        .import(ItemsModule)
        .build()
        .unwrap();

    assert_eq!(app.routes()[0].path(), "/items");
    assert!(app.get::<ItemsService>().is_ok());

    let response = app.routes()[0]
        .call(BootRequest::new(HttpMethod::Get, "/items"))
        .await
        .unwrap();

    assert_eq!(response.body, b"item-a,item-b");
}
