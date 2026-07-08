use a3s_boot::{
    BootApplication, BootError, BootResponse, ControllerDefinition, LazyModuleLoader, Module,
    ModuleRef, ProviderDefinition, ProviderOnModuleInit, ProviderToken, Result,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[derive(Debug)]
struct LazyConfig {
    value: &'static str,
}

#[derive(Debug)]
struct LazyService {
    config: Arc<LazyConfig>,
}

#[derive(Debug)]
struct LazyFeatureModule {
    calls: Arc<AtomicUsize>,
}

impl Module for LazyFeatureModule {
    fn name(&self) -> &'static str {
        "lazy-feature"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::factory::<LazyService, _>(move |module_ref| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(LazyService {
                    config: module_ref.get::<LazyConfig>()?,
                })
            }),
            ProviderDefinition::singleton(LazyConfig { value: "lazy" }),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<LazyService>()])
    }
}

#[derive(Debug)]
struct ImportedConfigModule;

impl Module for ImportedConfigModule {
    fn name(&self) -> &'static str {
        "imported-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(LazyConfig {
            value: "imported",
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<LazyConfig>()])
    }
}

#[derive(Debug)]
struct LazyConsumerModule;

impl Module for LazyConsumerModule {
    fn name(&self) -> &'static str {
        "lazy-consumer"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ImportedConfigModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<LazyService, _>(
            |module_ref| {
                Ok(LazyService {
                    config: module_ref.get::<LazyConfig>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct AsyncLazyConfig {
    value: String,
}

#[derive(Debug)]
struct AsyncLazyService {
    config: Arc<AsyncLazyConfig>,
}

#[derive(Debug)]
struct AsyncLazyModule {
    calls: Arc<AtomicUsize>,
}

impl Module for AsyncLazyModule {
    fn name(&self) -> &'static str {
        "async-lazy"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::factory::<AsyncLazyService, _>(|module_ref| {
                Ok(AsyncLazyService {
                    config: module_ref.get::<AsyncLazyConfig>()?,
                })
            }),
            ProviderDefinition::async_factory::<AsyncLazyConfig, _, _>(move |_| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(AsyncLazyConfig {
                        value: "async-lazy".to_string(),
                    })
                }
            }),
        ])
    }
}

#[derive(Debug)]
struct LoaderConsumer {
    loader: Arc<LazyModuleLoader>,
}

#[derive(Debug)]
struct LoaderConsumerModule;

impl Module for LoaderConsumerModule {
    fn name(&self) -> &'static str {
        "loader-consumer"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<LoaderConsumer, _>(
            |module_ref| {
                Ok(LoaderConsumer {
                    loader: module_ref.get::<LazyModuleLoader>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct LazyHookProvider {
    calls: Arc<AtomicUsize>,
}

impl ProviderOnModuleInit for LazyHookProvider {
    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[derive(Debug)]
struct LazyLifecycleModule {
    module_calls: Arc<AtomicUsize>,
    provider_calls: Arc<AtomicUsize>,
    controller_calls: Arc<AtomicUsize>,
}

impl Module for LazyLifecycleModule {
    fn name(&self) -> &'static str {
        "lazy-lifecycle"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(LazyHookProvider {
            calls: Arc::clone(&self.provider_calls),
        })
        .with_on_module_init::<LazyHookProvider>()])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        self.controller_calls.fetch_add(1, Ordering::SeqCst);
        Ok(vec![ControllerDefinition::new("/lazy-lifecycle")?
            .get("/", |_| async { Ok(BootResponse::text("lazy")) })?])
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.module_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn lazy_module_loader_loads_provider_graph_on_demand() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder().build().unwrap();
    let loader = app.lazy_module_loader().unwrap();

    assert!(matches!(
        app.get::<LazyService>(),
        Err(BootError::MissingProvider(_))
    ));

    let loaded = loader
        .load(LazyFeatureModule {
            calls: Arc::clone(&calls),
        })
        .unwrap();
    let service = loaded.get::<LazyService>().unwrap();

    assert_eq!(loaded.name(), "lazy-feature");
    assert_eq!(service.config.value, "lazy");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(matches!(
        app.get::<LazyService>(),
        Err(BootError::MissingProvider(_))
    ));
}

#[test]
fn lazy_module_loader_caches_loaded_modules_by_name() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder().build().unwrap();
    let loader = app.lazy_module_loader().unwrap();

    let first = loader
        .load(LazyFeatureModule {
            calls: Arc::clone(&calls),
        })
        .unwrap();
    let second = loader
        .load(LazyFeatureModule {
            calls: Arc::clone(&calls),
        })
        .unwrap();

    let first_service = first.get::<LazyService>().unwrap();
    let second_service = second.get::<LazyService>().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&first_service, &second_service));
}

#[test]
fn lazy_module_loader_reuses_eagerly_registered_modules() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(LazyFeatureModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();
    let eager_service = app.get::<LazyService>().unwrap();

    let loaded = app
        .lazy_module_loader()
        .unwrap()
        .load(LazyFeatureModule {
            calls: Arc::clone(&calls),
        })
        .unwrap();
    let loaded_service = loaded.get::<LazyService>().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(Arc::ptr_eq(&eager_service, &loaded_service));
}

#[test]
fn lazy_modules_can_use_imported_exports() {
    let app = BootApplication::builder().build().unwrap();
    let loaded = app
        .lazy_module_loader()
        .unwrap()
        .load(LazyConsumerModule)
        .unwrap();

    let service = loaded.get::<LazyService>().unwrap();

    assert_eq!(service.config.value, "imported");
}

#[tokio::test]
async fn lazy_module_loader_supports_async_provider_factories() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder().build().unwrap();
    let loader = app.lazy_module_loader().unwrap();

    let sync_result = loader.load(AsyncLazyModule {
        calls: Arc::clone(&calls),
    });
    assert!(
        matches!(sync_result, Err(BootError::Internal(message)) if message.contains("async provider factory requires async registration"))
    );

    let loaded = loader
        .load_async(AsyncLazyModule {
            calls: Arc::clone(&calls),
        })
        .await
        .unwrap();
    let config = loaded.get::<AsyncLazyConfig>().unwrap();
    let service = loaded.get::<AsyncLazyService>().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(config.value, "async-lazy");
    assert!(Arc::ptr_eq(&config, &service.config));
}

#[test]
fn lazy_module_loader_is_injectable() {
    let app = BootApplication::builder()
        .import(LoaderConsumerModule)
        .build()
        .unwrap();

    let app_loader = app.lazy_module_loader().unwrap();
    let consumer = app.get::<LoaderConsumer>().unwrap();

    assert!(Arc::ptr_eq(&app_loader, &consumer.loader));
}

#[test]
fn lazy_modules_do_not_register_controllers_or_lifecycle_hooks() {
    let module_calls = Arc::new(AtomicUsize::new(0));
    let provider_calls = Arc::new(AtomicUsize::new(0));
    let controller_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder().build().unwrap();

    let loaded = app
        .lazy_module_loader()
        .unwrap()
        .load(LazyLifecycleModule {
            module_calls: Arc::clone(&module_calls),
            provider_calls: Arc::clone(&provider_calls),
            controller_calls: Arc::clone(&controller_calls),
        })
        .unwrap();

    assert!(loaded.get::<LazyHookProvider>().is_ok());
    assert!(app.routes().is_empty());
    assert_eq!(module_calls.load(Ordering::SeqCst), 0);
    assert_eq!(provider_calls.load(Ordering::SeqCst), 0);
    assert_eq!(controller_calls.load(Ordering::SeqCst), 0);
}
