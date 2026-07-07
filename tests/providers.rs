use a3s_boot::{
    BootApplication, BootError, BootRequest, BootResponse, ControllerDefinition, DynamicModule,
    ExecutionContext, HttpMethod, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
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
struct GlobalConfig {
    value: &'static str,
}

#[derive(Debug)]
struct UsesGlobalConfig {
    config: Arc<GlobalConfig>,
}

#[derive(Debug)]
struct RuntimeConfig {
    value: String,
}

#[derive(Debug)]
struct UsesRuntimeConfig {
    config: Arc<RuntimeConfig>,
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

#[derive(Debug)]
struct PrivateConfigModule;

impl Module for PrivateConfigModule {
    fn name(&self) -> &'static str {
        "private-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(SharedConfig {
            value: "private",
        })])
    }
}

#[derive(Debug)]
struct NeedsPrivateConfigModule;

impl Module for NeedsPrivateConfigModule {
    fn name(&self) -> &'static str {
        "needs-private-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(PrivateConfigModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<UsesSharedConfig, _>(
            |module_ref| {
                Ok(UsesSharedConfig {
                    config: module_ref.get::<SharedConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct ExportedConfigModule;

impl Module for ExportedConfigModule {
    fn name(&self) -> &'static str {
        "exported-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(SharedConfig {
            value: "exported",
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<SharedConfig>()])
    }
}

#[derive(Debug)]
struct UsesExportedConfigModule;

impl Module for UsesExportedConfigModule {
    fn name(&self) -> &'static str {
        "uses-exported-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ExportedConfigModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<UsesSharedConfig, _>(
            |module_ref| {
                Ok(UsesSharedConfig {
                    config: module_ref.get::<SharedConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct ReExportedConfigModule;

impl Module for ReExportedConfigModule {
    fn name(&self) -> &'static str {
        "re-exported-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ExportedConfigModule)]
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<SharedConfig>()])
    }
}

#[derive(Debug)]
struct UsesReExportedConfigModule;

impl Module for UsesReExportedConfigModule {
    fn name(&self) -> &'static str {
        "uses-re-exported-config"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ReExportedConfigModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<UsesSharedConfig, _>(
            |module_ref| {
                Ok(UsesSharedConfig {
                    config: module_ref.get::<SharedConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct GlobalConfigModule;

impl Module for GlobalConfigModule {
    fn name(&self) -> &'static str {
        "global-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GlobalConfig {
            value: "global",
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<GlobalConfig>()])
    }

    fn is_global(&self) -> bool {
        true
    }
}

#[derive(Debug)]
struct UsesGlobalConfigModule;

impl Module for UsesGlobalConfigModule {
    fn name(&self) -> &'static str {
        "uses-global-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<UsesGlobalConfig, _>(
            |module_ref| {
                Ok(UsesGlobalConfig {
                    config: module_ref.get::<GlobalConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct UsesRuntimeConfigModule;

impl Module for UsesRuntimeConfigModule {
    fn name(&self) -> &'static str {
        "uses-runtime-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<UsesRuntimeConfig, _>(
            |module_ref| {
                Ok(UsesRuntimeConfig {
                    config: module_ref.get::<RuntimeConfig>()?,
                })
            },
        )])
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
fn duplicate_providers_are_scoped_to_declaring_modules() {
    let init_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(DuplicateProviderParentModule {
            init_calls: Arc::clone(&init_calls),
        })
        .build()
        .unwrap();

    assert!(app.get::<ItemsService>().is_ok());
    assert_eq!(init_calls.load(Ordering::SeqCst), 1);
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
fn unexported_imported_providers_are_not_visible_to_importing_modules() {
    let result = BootApplication::builder()
        .import(NeedsPrivateConfigModule)
        .build();

    assert!(matches!(
        result,
        Err(BootError::MissingProvider(message))
            if message == ProviderToken::of::<SharedConfig>().to_string()
    ));
}

#[test]
fn exported_imported_providers_are_visible_to_importing_modules() {
    let app = BootApplication::builder()
        .import(UsesExportedConfigModule)
        .build()
        .unwrap();

    let config = app.get::<SharedConfig>().unwrap();
    let dependent = app.get::<UsesSharedConfig>().unwrap();

    assert_eq!(config.value, "exported");
    assert_eq!(dependent.config.value, "exported");
}

#[test]
fn imported_exports_can_be_re_exported_transitively() {
    let app = BootApplication::builder()
        .import(UsesReExportedConfigModule)
        .build()
        .unwrap();

    let config = app.get::<SharedConfig>().unwrap();
    let dependent = app.get::<UsesSharedConfig>().unwrap();

    assert_eq!(config.value, "exported");
    assert_eq!(dependent.config.value, "exported");
}

#[test]
fn global_modules_expose_exported_providers_to_other_modules() {
    let app = BootApplication::builder()
        .import(GlobalConfigModule)
        .import(UsesGlobalConfigModule)
        .build()
        .unwrap();

    let config = app.get::<GlobalConfig>().unwrap();
    let dependent = app.get::<UsesGlobalConfig>().unwrap();

    assert_eq!(config.value, "global");
    assert_eq!(dependent.config.value, "global");
}

#[test]
fn dynamic_modules_can_provide_exported_runtime_configuration() {
    let dynamic_config = DynamicModule::new("runtime-config")
        .provider(ProviderDefinition::singleton(RuntimeConfig {
            value: "dynamic".to_string(),
        }))
        .export::<RuntimeConfig>()
        .global();
    let app = BootApplication::builder()
        .import(dynamic_config)
        .import(UsesRuntimeConfigModule)
        .build()
        .unwrap();

    let config = app.get::<RuntimeConfig>().unwrap();
    let dependent = app.get::<UsesRuntimeConfig>().unwrap();

    assert_eq!(config.value, "dynamic");
    assert_eq!(dependent.config.value, "dynamic");
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
