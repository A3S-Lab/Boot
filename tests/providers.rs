use a3s_boot::{
    BootApplication, BootError, BootFactory, BootRequest, BootResponse, ControllerDefinition,
    DynamicModule, ExecutionContext, FromModuleRef, HttpMethod, Module, ModuleRef,
    ProviderDefinition, ProviderDependency, ProviderOnModuleInit, ProviderRef, ProviderScope,
    ProviderToken, Result, TestingModule,
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
struct OrderIndependentRepository {
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
struct ZAsyncConfig {
    value: String,
}

#[derive(Debug)]
struct AAsyncConfigConsumer {
    config: Arc<ZAsyncConfig>,
}

#[derive(Debug)]
struct OrderIndependentProviderModule;

impl Module for OrderIndependentProviderModule {
    fn name(&self) -> &'static str {
        "order-independent-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::factory::<OrderIndependentRepository, _>(|module_ref| {
                Ok(OrderIndependentRepository {
                    config: module_ref.get::<SharedConfig>()?,
                })
            }),
            ProviderDefinition::singleton(SharedConfig { value: "late" }),
        ])
    }
}

#[derive(Debug)]
struct AsyncProviderModule {
    calls: Arc<AtomicUsize>,
}

impl Module for AsyncProviderModule {
    fn name(&self) -> &'static str {
        "async-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::singleton(SharedConfig { value: "shared" }),
            ProviderDefinition::async_factory::<RuntimeConfig, _, _>(move |module_ref| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    let shared = module_ref.get::<SharedConfig>()?;
                    Ok(RuntimeConfig {
                        value: format!("{}-async", shared.value),
                    })
                }
            }),
            ProviderDefinition::factory::<UsesRuntimeConfig, _>(|module_ref| {
                Ok(UsesRuntimeConfig {
                    config: module_ref.get::<RuntimeConfig>()?,
                })
            }),
        ])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let service = module_ref.get::<UsesRuntimeConfig>()?;
        Ok(vec![ControllerDefinition::new("/async-provider")?.get(
            "/",
            move |_| {
                let service = Arc::clone(&service);
                async move { Ok(BootResponse::text(service.config.value.clone())) }
            },
        )?])
    }
}

#[derive(Debug)]
struct AsyncOrderIndependentProviderModule {
    calls: Arc<AtomicUsize>,
}

impl Module for AsyncOrderIndependentProviderModule {
    fn name(&self) -> &'static str {
        "async-order-independent-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::factory::<UsesRuntimeConfig, _>(|module_ref| {
                Ok(UsesRuntimeConfig {
                    config: module_ref.get::<RuntimeConfig>()?,
                })
            }),
            ProviderDefinition::async_factory::<RuntimeConfig, _, _>(move |_| {
                let calls = Arc::clone(&calls);
                async move {
                    calls.fetch_add(1, Ordering::SeqCst);
                    Ok(RuntimeConfig {
                        value: "late-async".to_string(),
                    })
                }
            }),
        ])
    }
}

#[derive(Debug)]
struct AsyncProviderDeclarationOrderModule {
    calls: Arc<AtomicUsize>,
}

impl Module for AsyncProviderDeclarationOrderModule {
    fn name(&self) -> &'static str {
        "async-provider-declaration-order"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let dependency_calls = Arc::clone(&self.calls);
        let consumer_calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::async_factory::<ZAsyncConfig, _, _>(move |_| {
                let dependency_calls = Arc::clone(&dependency_calls);
                async move {
                    dependency_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(ZAsyncConfig {
                        value: "declared-first".to_string(),
                    })
                }
            }),
            ProviderDefinition::async_factory::<AAsyncConfigConsumer, _, _>(move |module_ref| {
                let consumer_calls = Arc::clone(&consumer_calls);
                async move {
                    consumer_calls.fetch_add(1, Ordering::SeqCst);
                    Ok(AAsyncConfigConsumer {
                        config: module_ref.get::<ZAsyncConfig>()?,
                    })
                }
            }),
        ])
    }
}

#[derive(Debug)]
struct BadAsyncProviderScopeModule;

impl Module for BadAsyncProviderScopeModule {
    fn name(&self) -> &'static str {
        "bad-async-provider-scope"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::async_factory::<RuntimeConfig, _, _>(|_| async {
                Ok(RuntimeConfig {
                    value: "bad".to_string(),
                })
            })
            .with_scope(ProviderScope::Transient),
        ])
    }
}

#[derive(Debug)]
struct AutoRepository {
    config: Arc<SharedConfig>,
    missing_items: Option<Arc<ItemsService>>,
}

impl FromModuleRef for AutoRepository {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            config: module_ref.get::<SharedConfig>()?,
            missing_items: module_ref.get_optional::<ItemsService>()?,
        })
    }
}

#[derive(Debug)]
struct AutoNamedRepository {
    config: Arc<SharedConfig>,
}

impl FromModuleRef for AutoNamedRepository {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            config: module_ref.get_named::<SharedConfig>("named-shared-config")?,
        })
    }
}

#[derive(Debug)]
struct ScopedCounter {
    id: usize,
}

#[derive(Debug)]
struct ScopedConsumer {
    first: Arc<ScopedCounter>,
    second: Arc<ScopedCounter>,
}

#[derive(Debug)]
struct BubbledConsumer {
    counter: Arc<ScopedCounter>,
}

#[derive(Debug)]
struct ContextualTransient {
    counter: Arc<ScopedCounter>,
}

#[derive(Debug)]
struct TransitiveBubbledConsumer {
    dependency: Arc<ContextualTransient>,
}

#[derive(Debug)]
struct LazyCounterConsumer {
    counter: ProviderRef<ScopedCounter>,
}

#[derive(Debug)]
struct NamedBubbledConsumer {
    counter: Arc<ScopedCounter>,
}

#[derive(Debug)]
struct OptionalExistingConsumer {
    counter: Option<Arc<ScopedCounter>>,
}

#[derive(Debug)]
struct OptionalMissingConsumer {
    counter: Option<Arc<ScopedCounter>>,
}

#[derive(Debug)]
struct MissingLazyConsumer {
    _counter: ProviderRef<ScopedCounter>,
}

#[derive(Debug)]
struct LifecycleBubbledConsumer {
    _counter: Arc<ScopedCounter>,
}

impl ProviderOnModuleInit for LifecycleBubbledConsumer {}

#[derive(Debug)]
struct AsyncContextualConsumer {
    _counter: Arc<ScopedCounter>,
}

#[derive(Debug)]
struct AsyncDependency {
    value: &'static str,
}

#[derive(Debug)]
struct AsyncDependent {
    dependency: Arc<AsyncDependency>,
}

#[derive(Debug)]
struct AsyncCycleA {
    _dependency: Arc<AsyncCycleB>,
}

#[derive(Debug)]
struct AsyncCycleB {
    _dependency: Arc<AsyncCycleA>,
}

#[derive(Debug)]
struct ForwardContextConsumer {
    counter: Arc<ScopedCounter>,
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
struct RequestScopedChildModule {
    calls: Arc<AtomicUsize>,
}

impl Module for RequestScopedChildModule {
    fn name(&self) -> &'static str {
        "request-scoped-child"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<ScopedCounter>()])
    }
}

#[derive(Debug)]
struct RequestScopedParentModule {
    calls: Arc<AtomicUsize>,
}

impl Module for RequestScopedParentModule {
    fn name(&self) -> &'static str {
        "request-scoped-parent"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(RequestScopedChildModule {
            calls: Arc::clone(&self.calls),
        })]
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/scope")?.get(
            "/",
            |request: BootRequest| async move {
                let first = request.get::<ScopedCounter>()?;
                let second = request.get::<ScopedCounter>()?;
                Ok(BootResponse::text(format!("{}:{}", first.id, second.id)))
            },
        )?])
    }
}

#[derive(Debug)]
struct RequestScopedDependencyModule {
    calls: Arc<AtomicUsize>,
}

impl Module for RequestScopedDependencyModule {
    fn name(&self) -> &'static str {
        "request-scoped-dependency"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
            ProviderDefinition::request_scoped::<ScopedConsumer, _>(|module_ref| {
                Ok(ScopedConsumer {
                    first: module_ref.get::<ScopedCounter>()?,
                    second: module_ref.get::<ScopedCounter>()?,
                })
            }),
        ])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/dependency-scope")?.get(
            "/",
            |request: BootRequest| async move {
                let consumer = request.get::<ScopedConsumer>()?;
                let direct = request.get::<ScopedCounter>()?;
                Ok(BootResponse::text(format!(
                    "{}:{}:{}",
                    consumer.first.id, consumer.second.id, direct.id
                )))
            },
        )?])
    }
}

#[derive(Debug)]
struct BubbledProviderModule {
    calls: Arc<AtomicUsize>,
}

impl Module for BubbledProviderModule {
    fn name(&self) -> &'static str {
        "bubbled-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
            ProviderDefinition::factory::<BubbledConsumer, _>(|module_ref| {
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        ])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/bubbled")?.get(
            "/",
            |request: BootRequest| async move {
                let consumer = request.get::<BubbledConsumer>()?;
                let same = request.get::<BubbledConsumer>()?;
                let direct = request.get::<ScopedCounter>()?;
                Ok(BootResponse::text(format!(
                    "{}:{}:{}:{}",
                    consumer.counter.id,
                    same.counter.id,
                    direct.id,
                    Arc::ptr_eq(&consumer, &same)
                )))
            },
        )?])
    }
}

#[derive(Debug)]
struct ForwardContextRootModule {
    calls: Arc<AtomicUsize>,
}

impl Module for ForwardContextRootModule {
    fn name(&self) -> &'static str {
        "forward-context-root"
    }

    fn forward_imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ForwardContextFeatureModule {
            calls: Arc::clone(&self.calls),
        })]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![
            ProviderToken::of::<ScopedCounter>(),
            ProviderToken::of::<ForwardContextConsumer>(),
        ])
    }
}

#[derive(Debug)]
struct ForwardContextFeatureModule {
    calls: Arc<AtomicUsize>,
}

impl Module for ForwardContextFeatureModule {
    fn name(&self) -> &'static str {
        "forward-context-feature"
    }

    fn forward_imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ForwardContextRootModule {
            calls: Arc::clone(&self.calls),
        })]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::factory::<ForwardContextConsumer, _>(|module_ref| {
                Ok(ForwardContextConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<ForwardContextConsumer>()])
    }
}

#[derive(Debug)]
struct RequestScopedControllerModule {
    calls: Arc<AtomicUsize>,
}

impl Module for RequestScopedControllerModule {
    fn name(&self) -> &'static str {
        "request-scoped-controller"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
        ])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/scoped-controller")?
            .get_scoped("/", |module_ref| {
                let controller_counter = module_ref.get::<ScopedCounter>()?;
                Ok(move |request: BootRequest| {
                    let controller_counter = Arc::clone(&controller_counter);
                    async move {
                        let request_counter = request.get::<ScopedCounter>()?;
                        Ok(BootResponse::text(format!(
                            "{}:{}",
                            controller_counter.id, request_counter.id
                        )))
                    }
                })
            })?])
    }
}

#[derive(Debug)]
struct AliasModule;

impl Module for AliasModule {
    fn name(&self) -> &'static str {
        "alias"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::singleton(SharedConfig { value: "primary" }),
            ProviderDefinition::named_alias("config-alias", ProviderToken::of::<SharedConfig>()),
            ProviderDefinition::named_alias(
                "config-second-alias",
                ProviderToken::named("config-alias"),
            ),
        ])
    }
}

#[derive(Debug)]
struct RequestScopedAliasModule {
    calls: Arc<AtomicUsize>,
}

#[derive(Debug)]
struct CircularA {
    _b: Arc<CircularB>,
}

#[derive(Debug)]
struct CircularB {
    _a: Arc<CircularA>,
}

#[derive(Debug)]
struct LazyCircularA {
    b: ProviderRef<LazyCircularB>,
}

#[derive(Debug)]
struct LazyCircularB {
    a: Arc<LazyCircularA>,
}

#[derive(Debug)]
struct SingletonCycleModule;

impl Module for SingletonCycleModule {
    fn name(&self) -> &'static str {
        "singleton-cycle"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::named_factory::<CircularA, _>("cycle-a", |module_ref| {
                Ok(CircularA {
                    _b: module_ref.get_named::<CircularB>("cycle-b")?,
                })
            }),
            ProviderDefinition::named_factory::<CircularB, _>("cycle-b", |module_ref| {
                Ok(CircularB {
                    _a: module_ref.get_named::<CircularA>("cycle-a")?,
                })
            }),
        ])
    }
}

#[derive(Debug)]
struct LazyCycleModule;

impl Module for LazyCycleModule {
    fn name(&self) -> &'static str {
        "lazy-cycle"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::factory::<LazyCircularA, _>(|module_ref| {
                Ok(LazyCircularA {
                    b: module_ref.provider_ref::<LazyCircularB>(),
                })
            }),
            ProviderDefinition::factory::<LazyCircularB, _>(|module_ref| {
                Ok(LazyCircularB {
                    a: module_ref.get::<LazyCircularA>()?,
                })
            }),
        ])
    }
}

impl Module for RequestScopedAliasModule {
    fn name(&self) -> &'static str {
        "request-scoped-alias"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![
            ProviderDefinition::request_scoped::<ScopedCounter, _>(move |_| {
                Ok(ScopedCounter {
                    id: calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            }),
            ProviderDefinition::named_alias("counter-alias", ProviderToken::of::<ScopedCounter>()),
        ])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/alias-scope")?.get(
            "/",
            |request: BootRequest| async move {
                let direct = request.get::<ScopedCounter>()?;
                let alias = request.get_named::<ScopedCounter>("counter-alias")?;
                Ok(BootResponse::text(format!("{}:{}", direct.id, alias.id)))
            },
        )?])
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

#[derive(Debug)]
struct AsyncGlobalRuntimeConfigModule;

impl Module for AsyncGlobalRuntimeConfigModule {
    fn name(&self) -> &'static str {
        "async-global-runtime-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::async_factory::<RuntimeConfig, _, _>(|_| async {
                Ok(RuntimeConfig {
                    value: "async-global".to_string(),
                })
            }),
        ])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<RuntimeConfig>()])
    }

    fn is_global(&self) -> bool {
        true
    }
}

#[derive(Debug)]
struct AutoProviderModule;

impl Module for AutoProviderModule {
    fn name(&self) -> &'static str {
        "auto-provider"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::singleton(SharedConfig { value: "auto" }),
            ProviderDefinition::named_singleton(
                "named-shared-config",
                SharedConfig {
                    value: "named-auto",
                },
            ),
            ProviderDefinition::injectable::<AutoRepository>(),
            ProviderDefinition::named_injectable::<AutoNamedRepository>("named-auto-repository"),
        ])
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
fn injectable_provider_factories_resolve_dependencies() {
    let app = BootApplication::builder()
        .import(AutoProviderModule)
        .build()
        .unwrap();

    let repository = app.get::<AutoRepository>().unwrap();
    let named = app
        .get_named::<AutoNamedRepository>("named-auto-repository")
        .unwrap();

    assert_eq!(repository.config.value, "auto");
    assert!(repository.missing_items.is_none());
    assert_eq!(named.config.value, "named-auto");
}

#[test]
fn transient_injectable_providers_are_rebuilt_per_resolution() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::singleton(SharedConfig {
            value: "auto",
        }))
        .unwrap();
    module_ref
        .register(ProviderDefinition::transient_injectable::<AutoRepository>())
        .unwrap();

    let first = module_ref.get::<AutoRepository>().unwrap();
    let second = module_ref.get::<AutoRepository>().unwrap();

    assert!(!Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&first.config, &second.config));
}

#[test]
fn module_ref_can_create_unregistered_injectables() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::singleton(SharedConfig {
            value: "created",
        }))
        .unwrap();

    let first = module_ref.create_arc::<AutoRepository>().unwrap();
    let second = module_ref.create_arc::<AutoRepository>().unwrap();

    assert_eq!(first.config.value, "created");
    assert!(first.missing_items.is_none());
    assert!(Arc::ptr_eq(&first.config, &second.config));
    assert!(!Arc::ptr_eq(&first, &second));
    assert!(!module_ref.contains_provider::<AutoRepository>().unwrap());
}

#[test]
fn module_ref_resolve_uses_a_fresh_request_resolution_context() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let counter_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: counter_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedConsumer, _>(
            |module_ref| {
                Ok(ScopedConsumer {
                    first: module_ref.get::<ScopedCounter>()?,
                    second: module_ref.get::<ScopedCounter>()?,
                })
            },
        ))
        .unwrap();

    let first = module_ref.resolve::<ScopedConsumer>().unwrap();
    let second = module_ref.resolve::<ScopedConsumer>().unwrap();

    assert_eq!(first.first.id, 1);
    assert_eq!(first.second.id, 1);
    assert!(Arc::ptr_eq(&first.first, &first.second));
    assert_eq!(second.first.id, 2);
    assert_eq!(second.second.id, 2);
    assert!(Arc::ptr_eq(&second.first, &second.second));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn request_scope_propagates_to_declared_singleton_dependencies() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let counter_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: counter_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<BubbledConsumer, _>(|module_ref| {
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        )
        .unwrap();

    assert!(module_ref
        .provider_is_contextual::<BubbledConsumer>()
        .unwrap());
    let error = module_ref.get::<BubbledConsumer>().unwrap_err();
    assert!(matches!(
        error,
        BootError::Internal(message)
            if message.contains("requires an active request scope")
    ));

    let first_scope = module_ref.request_scope();
    let first = first_scope.get::<BubbledConsumer>().unwrap();
    let same = first_scope.get::<BubbledConsumer>().unwrap();
    let direct = first_scope.get::<ScopedCounter>().unwrap();
    let second = module_ref.request_scope().get::<BubbledConsumer>().unwrap();

    assert!(Arc::ptr_eq(&first, &same));
    assert!(Arc::ptr_eq(&first.counter, &direct));
    assert_eq!(first.counter.id, 1);
    assert_eq!(second.counter.id, 2);
    assert!(!Arc::ptr_eq(&first, &second));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn request_scope_propagates_through_transient_and_alias_dependencies() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let counter_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: counter_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::transient::<ContextualTransient, _>(|module_ref| {
                Ok(ContextualTransient {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<TransitiveBubbledConsumer, _>(|module_ref| {
                Ok(TransitiveBubbledConsumer {
                    dependency: module_ref.get::<ContextualTransient>()?,
                })
            })
            .depends_on::<ContextualTransient>(),
        )
        .unwrap();
    module_ref
        .register(ProviderDefinition::named_alias(
            "request-counter",
            ProviderToken::of::<ScopedCounter>(),
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::named_factory::<BubbledConsumer, _>(
                "alias-consumer",
                |module_ref| {
                    Ok(BubbledConsumer {
                        counter: module_ref.get_named::<ScopedCounter>("request-counter")?,
                    })
                },
            )
            .depends_on_named("request-counter"),
        )
        .unwrap();

    assert!(module_ref
        .provider_is_contextual::<ContextualTransient>()
        .unwrap());
    assert!(module_ref
        .provider_is_contextual::<TransitiveBubbledConsumer>()
        .unwrap());
    assert!(module_ref
        .named_provider_is_contextual("alias-consumer")
        .unwrap());

    let scope = module_ref.request_scope();
    let transitive = scope.get::<TransitiveBubbledConsumer>().unwrap();
    let alias = scope
        .get_named::<BubbledConsumer>("alias-consumer")
        .unwrap();
    assert!(Arc::ptr_eq(&transitive.dependency.counter, &alias.counter));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn lazy_dependencies_do_not_bubble_but_require_an_explicit_resolution_context() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            |_| Ok(ScopedCounter { id: 1 }),
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<LazyCounterConsumer, _>(|module_ref| {
                Ok(LazyCounterConsumer {
                    counter: module_ref.provider_ref::<ScopedCounter>(),
                })
            })
            .with_dependency(ProviderDependency::typed::<ScopedCounter>().lazy()),
        )
        .unwrap();

    assert!(!module_ref
        .provider_is_contextual::<LazyCounterConsumer>()
        .unwrap());
    let consumer = module_ref.get::<LazyCounterConsumer>().unwrap();
    assert!(consumer.counter.get().is_err());
    assert_eq!(consumer.counter.resolve().unwrap().id, 1);
}

#[test]
fn opaque_singleton_factories_cannot_capture_request_scoped_providers() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            |_| Ok(ScopedCounter { id: 1 }),
        ))
        .unwrap();
    let factory_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::factory::<BubbledConsumer, _>(
            move |module_ref| {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            },
        ))
        .unwrap();

    let error = module_ref.get::<BubbledConsumer>().unwrap_err();
    assert!(matches!(
        error,
        BootError::Internal(message)
            if message.contains("BubbledConsumer")
                && message.contains("ScopedCounter")
                && message.contains(" -> ")
                && message.contains("declare factory dependencies")
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn dependency_plans_validate_required_contextual_and_lazy_edges() {
    let contextual_ref = ModuleRef::new();
    contextual_ref
        .register(
            ProviderDefinition::named_request_scoped::<ItemsService, _>(
                "request-with-missing-dependency",
                |_| Ok(ItemsService),
            )
            .depends_on_named("missing-eager-dependency"),
        )
        .unwrap();

    let contextual_error = contextual_ref
        .named_provider_is_contextual("request-with-missing-dependency")
        .unwrap_err();
    assert!(matches!(
        contextual_error,
        BootError::MissingProvider(token) if token == "missing-eager-dependency"
    ));

    let lazy_ref = ModuleRef::new();
    lazy_ref
        .register(
            ProviderDefinition::factory::<MissingLazyConsumer, _>(|module_ref| {
                Ok(MissingLazyConsumer {
                    _counter: module_ref.provider_ref::<ScopedCounter>(),
                })
            })
            .with_dependency(ProviderDependency::typed::<ScopedCounter>().lazy()),
        )
        .unwrap();

    let lazy_error = lazy_ref
        .provider_is_contextual::<MissingLazyConsumer>()
        .unwrap_err();
    assert!(matches!(
        lazy_error,
        BootError::MissingProvider(token)
            if token == ProviderToken::of::<ScopedCounter>().to_string()
    ));
}

#[test]
fn optional_dependencies_bubble_only_when_the_provider_exists() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let counter_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: counter_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<OptionalExistingConsumer, _>(|module_ref| {
                Ok(OptionalExistingConsumer {
                    counter: module_ref.get_optional::<ScopedCounter>()?,
                })
            })
            .with_dependency(ProviderDependency::typed::<ScopedCounter>().optional()),
        )
        .unwrap();
    module_ref
        .register(
            ProviderDefinition::factory::<OptionalMissingConsumer, _>(|module_ref| {
                Ok(OptionalMissingConsumer {
                    counter: module_ref.get_optional_named::<ScopedCounter>("missing-counter")?,
                })
            })
            .with_dependency(ProviderDependency::named("missing-counter").optional()),
        )
        .unwrap();

    assert!(module_ref
        .provider_is_contextual::<OptionalExistingConsumer>()
        .unwrap());
    assert!(!module_ref
        .provider_is_contextual::<OptionalMissingConsumer>()
        .unwrap());
    assert!(module_ref
        .get::<OptionalMissingConsumer>()
        .unwrap()
        .counter
        .is_none());

    let scope = module_ref.request_scope();
    let consumer = scope.get::<OptionalExistingConsumer>().unwrap();
    let direct = scope.get::<ScopedCounter>().unwrap();
    assert!(Arc::ptr_eq(consumer.counter.as_ref().unwrap(), &direct));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn contextual_plans_use_the_declaring_module_for_imports_and_named_aliases() {
    let calls = Arc::new(AtomicUsize::new(0));
    let request_calls = Arc::clone(&calls);
    let child = DynamicModule::new("contextual-owner-child")
        .provider(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: request_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .provider(ProviderDefinition::named_alias(
            "contextual-owner-alias",
            ProviderToken::of::<ScopedCounter>(),
        ))
        .export::<ScopedCounter>()
        .export_named("contextual-owner-alias");
    let parent = DynamicModule::new("contextual-owner-parent")
        .import(child)
        .provider(
            ProviderDefinition::factory::<BubbledConsumer, _>(|module_ref| {
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        )
        .provider(
            ProviderDefinition::factory::<NamedBubbledConsumer, _>(|module_ref| {
                Ok(NamedBubbledConsumer {
                    counter: module_ref.get_named::<ScopedCounter>("contextual-owner-alias")?,
                })
            })
            .depends_on_named("contextual-owner-alias"),
        );
    let sibling = DynamicModule::new("contextual-owner-sibling")
        .provider(ProviderDefinition::singleton(ScopedCounter { id: 99 }));

    let app = BootApplication::builder()
        .import(sibling)
        .import(parent)
        .build()
        .unwrap();

    assert!(app
        .module_ref()
        .provider_is_contextual::<BubbledConsumer>()
        .unwrap());
    assert!(app
        .module_ref()
        .provider_is_contextual::<NamedBubbledConsumer>()
        .unwrap());
    assert_eq!(app.get::<ScopedCounter>().unwrap().id, 99);

    let scope = app.module_ref().request_scope();
    let typed = scope.get::<BubbledConsumer>().unwrap();
    let named = scope.get::<NamedBubbledConsumer>().unwrap();
    assert_eq!(typed.counter.id, 1);
    assert!(Arc::ptr_eq(&typed.counter, &named.counter));
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn request_scope_bubbles_from_a_late_global_provider() {
    let calls = Arc::new(AtomicUsize::new(0));
    let consumer = DynamicModule::new("global-context-consumer").provider(
        ProviderDefinition::factory::<BubbledConsumer, _>(|module_ref| {
            Ok(BubbledConsumer {
                counter: module_ref.get::<ScopedCounter>()?,
            })
        })
        .depends_on::<ScopedCounter>(),
    );
    let request_calls = Arc::clone(&calls);
    let global = DynamicModule::new("global-context-provider")
        .provider(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: request_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .export::<ScopedCounter>()
        .global();

    let app = BootApplication::builder()
        .import(consumer)
        .import(global)
        .build()
        .unwrap();

    assert!(app
        .module_ref()
        .provider_is_contextual::<BubbledConsumer>()
        .unwrap());
    let first = app
        .module_ref()
        .request_scope()
        .get::<BubbledConsumer>()
        .unwrap();
    let second = app
        .module_ref()
        .request_scope()
        .get::<BubbledConsumer>()
        .unwrap();
    assert_eq!(first.counter.id, 1);
    assert_eq!(second.counter.id, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn request_scope_bubbles_across_forward_imports_after_graph_registration() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(ForwardContextRootModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    assert!(app
        .module_ref()
        .provider_is_contextual::<ForwardContextConsumer>()
        .unwrap());
    let first_scope = app.module_ref().request_scope();
    let first = first_scope.get::<ForwardContextConsumer>().unwrap();
    let direct = first_scope.get::<ScopedCounter>().unwrap();
    let second = app
        .module_ref()
        .request_scope()
        .get::<ForwardContextConsumer>()
        .unwrap();

    assert!(Arc::ptr_eq(&first.counter, &direct));
    assert_eq!(first.counter.id, 1);
    assert_eq!(second.counter.id, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn provider_overrides_recalculate_contextual_dependency_plans() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module = DynamicModule::new("contextual-provider-override")
        .provider(ProviderDefinition::singleton(ScopedCounter { id: 0 }))
        .provider(
            ProviderDefinition::factory::<BubbledConsumer, _>(|module_ref| {
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        );
    let request_calls = Arc::clone(&calls);
    let app = BootApplication::builder()
        .import(module)
        .override_provider(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: request_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .build()
        .unwrap();

    assert!(app
        .module_ref()
        .provider_is_contextual::<BubbledConsumer>()
        .unwrap());
    assert!(app.get::<BubbledConsumer>().is_err());
    let first = app
        .module_ref()
        .request_scope()
        .get::<BubbledConsumer>()
        .unwrap();
    let second = app
        .module_ref()
        .request_scope()
        .get::<BubbledConsumer>()
        .unwrap();
    assert_eq!(first.counter.id, 1);
    assert_eq!(second.counter.id, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn declared_static_singleton_graphs_are_initialized_only_once() {
    let dependency_calls = Arc::new(AtomicUsize::new(0));
    let consumer_calls = Arc::new(AtomicUsize::new(0));
    let dependency_factory_calls = Arc::clone(&dependency_calls);
    let consumer_factory_calls = Arc::clone(&consumer_calls);
    let module = DynamicModule::new("declared-static-singletons")
        .provider(ProviderDefinition::factory::<ScopedCounter, _>(move |_| {
            dependency_factory_calls.fetch_add(1, Ordering::SeqCst);
            Ok(ScopedCounter { id: 7 })
        }))
        .provider(
            ProviderDefinition::factory::<BubbledConsumer, _>(move |module_ref| {
                consumer_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(BubbledConsumer {
                    counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>(),
        );

    let app = BootApplication::builder().import(module).build().unwrap();
    let first = app.get::<BubbledConsumer>().unwrap();
    let second = app.get::<BubbledConsumer>().unwrap();

    assert!(!app
        .module_ref()
        .provider_is_contextual::<BubbledConsumer>()
        .unwrap());
    assert!(Arc::ptr_eq(&first, &second));
    assert!(Arc::ptr_eq(&first.counter, &second.counter));
    assert_eq!(dependency_calls.load(Ordering::SeqCst), 1);
    assert_eq!(consumer_calls.load(Ordering::SeqCst), 1);
}

#[test]
fn module_ref_resolve_supports_named_and_optional_lookup() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::named_singleton(
            "named-config",
            SharedConfig { value: "named" },
        ))
        .unwrap();

    let resolved = module_ref
        .resolve_named::<SharedConfig>("named-config")
        .unwrap();
    let optional = module_ref.resolve_optional::<SharedConfig>().unwrap();
    let optional_named = module_ref
        .resolve_optional_named::<SharedConfig>("named-config")
        .unwrap();

    assert_eq!(resolved.value, "named");
    assert!(optional.is_none());
    assert!(Arc::ptr_eq(&resolved, &optional_named.unwrap()));
}

#[test]
fn provider_aliases_resolve_existing_singletons() {
    let app = BootApplication::builder()
        .import(AliasModule)
        .build()
        .unwrap();

    let original = app.get::<SharedConfig>().unwrap();
    let alias = app.get_named::<SharedConfig>("config-alias").unwrap();
    let second_alias = app
        .get_named::<SharedConfig>("config-second-alias")
        .unwrap();

    assert_eq!(alias.value, "primary");
    assert!(Arc::ptr_eq(&original, &alias));
    assert!(Arc::ptr_eq(&original, &second_alias));
}

#[tokio::test]
async fn provider_aliases_preserve_request_scoped_resolution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(RequestScopedAliasModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/alias-scope"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/alias-scope"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:1");
    assert_eq!(second.body_text().unwrap(), "2:2");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn provider_alias_cycles_return_contextual_errors() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::named_alias(
            "alias-a",
            ProviderToken::named("alias-b"),
        ))
        .unwrap();
    module_ref
        .register(ProviderDefinition::named_alias(
            "alias-b",
            ProviderToken::named("alias-a"),
        ))
        .unwrap();

    let error = module_ref.get_named::<SharedConfig>("alias-a").unwrap_err();

    assert!(matches!(
        error,
        BootError::Internal(message)
            if message == "cyclic provider alias detected: alias-a -> alias-b -> alias-a"
    ));
}

#[test]
fn transient_provider_dependency_cycles_return_contextual_errors() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::named_transient::<CircularA, _>(
            "cycle-a",
            |module_ref| {
                Ok(CircularA {
                    _b: module_ref.get_named::<CircularB>("cycle-b")?,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(ProviderDefinition::named_transient::<CircularB, _>(
            "cycle-b",
            |module_ref| {
                Ok(CircularB {
                    _a: module_ref.get_named::<CircularA>("cycle-a")?,
                })
            },
        ))
        .unwrap();

    let error = module_ref.get_named::<CircularA>("cycle-a").unwrap_err();

    assert!(matches!(
        error,
        BootError::Internal(message)
            if message == "cyclic provider dependency detected: cycle-a -> cycle-b -> cycle-a"
    ));
}

#[test]
fn singleton_provider_dependency_cycles_return_contextual_errors() {
    let result = BootApplication::builder()
        .import(SingletonCycleModule)
        .build();

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message == "cyclic provider dependency detected: cycle-a -> cycle-b -> cycle-a"
    ));
}

#[test]
fn provider_refs_resolve_lazily_and_break_singleton_cycles() {
    let app = BootApplication::builder()
        .import(LazyCycleModule)
        .build()
        .unwrap();

    let first_a = app.get::<LazyCircularA>().unwrap();
    let b = first_a.b.get().unwrap();
    let second_a = b.a.clone();

    assert!(Arc::ptr_eq(&first_a, &second_a));
    assert_eq!(
        first_a.b.token().to_string(),
        ProviderToken::of::<LazyCircularB>().to_string()
    );
}

#[test]
fn request_scoped_provider_dependency_cycles_return_contextual_errors() {
    let module_ref = ModuleRef::new();
    module_ref
        .register(ProviderDefinition::named_request_scoped::<CircularA, _>(
            "cycle-a",
            |module_ref| {
                Ok(CircularA {
                    _b: module_ref.get_named::<CircularB>("cycle-b")?,
                })
            },
        ))
        .unwrap();
    module_ref
        .register(ProviderDefinition::named_request_scoped::<CircularB, _>(
            "cycle-b",
            |module_ref| {
                Ok(CircularB {
                    _a: module_ref.get_named::<CircularA>("cycle-a")?,
                })
            },
        ))
        .unwrap();

    let request_scope = module_ref.request_scope();
    let error = request_scope.get_named::<CircularA>("cycle-a").unwrap_err();

    assert!(matches!(
        error,
        BootError::Internal(message)
            if message == "cyclic provider dependency detected: cycle-a -> cycle-b -> cycle-a"
    ));
}

#[test]
fn provider_refs_preserve_request_scope_when_created_from_request_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let counter_calls = Arc::clone(&calls);
    module_ref
        .register(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: counter_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();

    let first_scope = module_ref.request_scope();
    let first_ref = first_scope.provider_ref::<ScopedCounter>();
    let first = first_ref.get().unwrap();
    let second = first_ref.get().unwrap();
    let second_scope = module_ref.request_scope();
    let third = second_scope.provider_ref::<ScopedCounter>().get().unwrap();

    assert_eq!(first.id, 1);
    assert_eq!(second.id, 1);
    assert_eq!(third.id, 2);
    assert!(Arc::ptr_eq(&first, &second));
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[test]
fn transient_providers_are_built_for_each_resolution() {
    let calls = Arc::new(AtomicUsize::new(0));
    let module_ref = ModuleRef::new();
    let provider_calls = Arc::clone(&calls);

    module_ref
        .register(ProviderDefinition::transient::<ScopedCounter, _>(
            move |_| {
                Ok(ScopedCounter {
                    id: provider_calls.fetch_add(1, Ordering::SeqCst) + 1,
                })
            },
        ))
        .unwrap();

    let first = module_ref.get::<ScopedCounter>().unwrap();
    let second = module_ref.get::<ScopedCounter>().unwrap();

    assert_eq!(first.id, 1);
    assert_eq!(second.id, 2);
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn request_scoped_providers_are_cached_per_request_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(RequestScopedParentModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/scope"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/scope"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:1");
    assert_eq!(second.body_text().unwrap(), "2:2");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn request_scoped_provider_dependencies_share_the_request_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(RequestScopedDependencyModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/dependency-scope"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/dependency-scope"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:1:1");
    assert_eq!(second.body_text().unwrap(), "2:2:2");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn request_scope_bubbles_into_declared_singletons_per_request() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(BubbledProviderModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    assert!(app.get::<BubbledConsumer>().is_err());
    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/bubbled"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/bubbled"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:1:1:true");
    assert_eq!(second.body_text().unwrap(), "2:2:2:true");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn scoped_controller_handlers_are_built_for_each_request_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(RequestScopedControllerModule {
            calls: Arc::clone(&calls),
        })
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/scoped-controller"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/scoped-controller"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:1");
    assert_eq!(second.body_text().unwrap(), "2:2");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
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
fn singleton_provider_factories_can_depend_on_later_module_providers() {
    let app = BootApplication::builder()
        .import(OrderIndependentProviderModule)
        .build()
        .unwrap();

    let repository = app.get::<OrderIndependentRepository>().unwrap();
    let config = app.get::<SharedConfig>().unwrap();

    assert_eq!(repository.config.value, "late");
    assert!(Arc::ptr_eq(&repository.config, &config));
}

#[test]
fn contextual_singletons_with_lifecycle_hooks_are_rejected_before_construction() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let provider_factory_calls = Arc::clone(&factory_calls);
    let module = DynamicModule::new("contextual-lifecycle-provider")
        .provider(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            |_| Ok(ScopedCounter { id: 1 }),
        ))
        .provider(
            ProviderDefinition::factory::<LifecycleBubbledConsumer, _>(move |module_ref| {
                provider_factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(LifecycleBubbledConsumer {
                    _counter: module_ref.get::<ScopedCounter>()?,
                })
            })
            .depends_on::<ScopedCounter>()
            .with_on_module_init::<LifecycleBubbledConsumer>(),
        );

    let result = BootApplication::builder().import(module).build();

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message.contains("LifecycleBubbledConsumer")
                && message.contains("singleton lifecycle hooks")
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn contextual_async_providers_are_rejected_before_factory_invocation() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let provider_factory_calls = Arc::clone(&factory_calls);
    let module = DynamicModule::new("contextual-async-provider")
        .provider(ProviderDefinition::request_scoped::<ScopedCounter, _>(
            |_| Ok(ScopedCounter { id: 1 }),
        ))
        .provider(
            ProviderDefinition::async_factory::<AsyncContextualConsumer, _, _>(move |module_ref| {
                provider_factory_calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(AsyncContextualConsumer {
                        _counter: module_ref.get::<ScopedCounter>()?,
                    })
                }
            })
            .depends_on::<ScopedCounter>(),
        );

    let result = BootApplication::builder()
        .import(module)
        .build_async()
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message.contains("AsyncContextualConsumer")
                && message.contains("cannot depend on a request-context provider")
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn async_singletons_seed_declared_dependencies_before_their_dependents() {
    let dependency_calls = Arc::new(AtomicUsize::new(0));
    let dependent_calls = Arc::new(AtomicUsize::new(0));
    let dependency_factory_calls = Arc::clone(&dependency_calls);
    let dependent_factory_calls = Arc::clone(&dependent_calls);
    let module = DynamicModule::new("async-provider-topology")
        .provider(
            ProviderDefinition::async_factory::<AsyncDependent, _, _>(move |module_ref| {
                dependent_factory_calls.fetch_add(1, Ordering::SeqCst);
                async move {
                    Ok(AsyncDependent {
                        dependency: module_ref.get::<AsyncDependency>()?,
                    })
                }
            })
            .depends_on::<AsyncDependency>(),
        )
        .provider(ProviderDefinition::async_factory::<AsyncDependency, _, _>(
            move |_| {
                dependency_factory_calls.fetch_add(1, Ordering::SeqCst);
                async {
                    Ok(AsyncDependency {
                        value: "dependency-first",
                    })
                }
            },
        ));

    let app = BootApplication::builder()
        .import(module)
        .build_async()
        .await
        .unwrap();
    let dependent = app.get::<AsyncDependent>().unwrap();
    let dependency = app.get::<AsyncDependency>().unwrap();

    assert_eq!(dependent.dependency.value, "dependency-first");
    assert!(Arc::ptr_eq(&dependent.dependency, &dependency));
    assert_eq!(dependency_calls.load(Ordering::SeqCst), 1);
    assert_eq!(dependent_calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn async_dependency_cycles_are_rejected_without_running_factories() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let first_calls = Arc::clone(&factory_calls);
    let second_calls = Arc::clone(&factory_calls);
    let module = DynamicModule::new("async-provider-cycle")
        .provider(
            ProviderDefinition::named_async_factory::<AsyncCycleA, _, _>(
                "async-cycle-a",
                move |module_ref| {
                    first_calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Ok(AsyncCycleA {
                            _dependency: module_ref.get_named::<AsyncCycleB>("async-cycle-b")?,
                        })
                    }
                },
            )
            .depends_on_named("async-cycle-b"),
        )
        .provider(
            ProviderDefinition::named_async_factory::<AsyncCycleB, _, _>(
                "async-cycle-b",
                move |module_ref| {
                    second_calls.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Ok(AsyncCycleB {
                            _dependency: module_ref.get_named::<AsyncCycleA>("async-cycle-a")?,
                        })
                    }
                },
            )
            .depends_on_named("async-cycle-a"),
        );

    let result = BootApplication::builder()
        .import(module)
        .build_async()
        .await;

    assert!(matches!(
        result,
        Err(BootError::Internal(message))
            if message
                == "cyclic async provider dependency detected: async-cycle-a -> async-cycle-b -> async-cycle-a"
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn missing_declared_async_dependencies_fail_before_factory_invocation() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let provider_factory_calls = Arc::clone(&factory_calls);
    let module = DynamicModule::new("async-provider-missing-dependency").provider(
        ProviderDefinition::async_factory::<AsyncDependency, _, _>(move |_| {
            provider_factory_calls.fetch_add(1, Ordering::SeqCst);
            async {
                Ok(AsyncDependency {
                    value: "unreachable",
                })
            }
        })
        .depends_on_named("missing-async-dependency"),
    );

    let result = BootApplication::builder()
        .import(module)
        .build_async()
        .await;

    assert!(matches!(
        result,
        Err(BootError::MissingProvider(token)) if token == "missing-async-dependency"
    ));
    assert_eq!(factory_calls.load(Ordering::SeqCst), 0);
}

#[tokio::test]
async fn async_provider_factories_are_awaited_before_controllers_build() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(AsyncProviderModule {
            calls: Arc::clone(&calls),
        })
        .build_async()
        .await
        .unwrap();

    let config = app.get::<RuntimeConfig>().unwrap();
    let dependent = app.get::<UsesRuntimeConfig>().unwrap();
    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/async-provider"))
        .await
        .unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(config.value, "shared-async");
    assert!(Arc::ptr_eq(&config, &dependent.config));
    assert_eq!(response.body_text().unwrap(), "shared-async");
}

#[tokio::test]
async fn async_build_seeds_async_singletons_before_sync_singleton_factories() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(AsyncOrderIndependentProviderModule {
            calls: Arc::clone(&calls),
        })
        .build_async()
        .await
        .unwrap();

    let config = app.get::<RuntimeConfig>().unwrap();
    let dependent = app.get::<UsesRuntimeConfig>().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(config.value, "late-async");
    assert!(Arc::ptr_eq(&config, &dependent.config));
}

#[tokio::test]
async fn async_provider_factories_keep_declaration_order_after_registration() {
    let calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(AsyncProviderDeclarationOrderModule {
            calls: Arc::clone(&calls),
        })
        .build_async()
        .await
        .unwrap();

    let config = app.get::<ZAsyncConfig>().unwrap();
    let consumer = app.get::<AAsyncConfigConsumer>().unwrap();

    assert_eq!(calls.load(Ordering::SeqCst), 2);
    assert_eq!(config.value, "declared-first");
    assert!(Arc::ptr_eq(&config, &consumer.config));
}

#[test]
fn sync_build_rejects_async_provider_factories() {
    let result = BootApplication::builder()
        .import(AsyncProviderModule {
            calls: Arc::new(AtomicUsize::new(0)),
        })
        .build();

    assert!(
        matches!(result, Err(BootError::Internal(message)) if message.contains("async provider factory requires async registration"))
    );
}

#[tokio::test]
async fn async_provider_factories_must_be_singletons() {
    let result = BootApplication::builder()
        .import(BadAsyncProviderScopeModule)
        .build_async()
        .await;

    assert!(
        matches!(result, Err(BootError::Internal(message)) if message.contains("async provider factories require singleton scope"))
    );
}

#[tokio::test]
async fn testing_module_compile_async_resolves_async_provider_factories() {
    let testing = TestingModule::builder()
        .provider(ProviderDefinition::async_factory::<RuntimeConfig, _, _>(
            |_| async {
                Ok(RuntimeConfig {
                    value: "testing-async".to_string(),
                })
            },
        ))
        .compile_async()
        .await
        .unwrap();

    assert_eq!(
        testing.get::<RuntimeConfig>().unwrap().value,
        "testing-async"
    );
}

#[tokio::test]
async fn boot_factory_create_async_resolves_async_provider_factories() {
    let handle = BootFactory::create_async(AsyncProviderModule {
        calls: Arc::new(AtomicUsize::new(0)),
    })
    .await
    .unwrap();

    assert_eq!(handle.get::<RuntimeConfig>().unwrap().value, "shared-async");
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
        .import(UsesGlobalConfigModule)
        .import(GlobalConfigModule)
        .build()
        .unwrap();

    let config = app.get::<GlobalConfig>().unwrap();
    let dependent = app.get::<UsesGlobalConfig>().unwrap();

    assert_eq!(config.value, "global");
    assert_eq!(dependent.config.value, "global");
}

#[tokio::test]
async fn async_global_modules_are_seeded_after_the_full_graph_is_registered() {
    let app = BootApplication::builder()
        .import(UsesRuntimeConfigModule)
        .import(AsyncGlobalRuntimeConfigModule)
        .build_async()
        .await
        .unwrap();

    let config = app.get::<RuntimeConfig>().unwrap();
    let dependent = app.get::<UsesRuntimeConfig>().unwrap();

    assert_eq!(config.value, "async-global");
    assert!(Arc::ptr_eq(&config, &dependent.config));
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
