use a3s_boot::{
    BootApplication, BootError, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct LaterAsyncGlobalConfig {
    value: &'static str,
}

#[derive(Debug)]
struct EarlyAsyncConsumer {
    config: Arc<LaterAsyncGlobalConfig>,
}

#[derive(Debug)]
struct EarlyAsyncConsumerModule {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Module for EarlyAsyncConsumerModule {
    fn name(&self) -> &'static str {
        "early-async-consumer"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ProviderDefinition::async_factory::<
            EarlyAsyncConsumer,
            _,
            _,
        >(move |module_ref| {
            let log = Arc::clone(&log);
            async move {
                let config = module_ref.get::<LaterAsyncGlobalConfig>()?;
                log.lock().unwrap().push("consumer-factory");
                Ok(EarlyAsyncConsumer { config })
            }
        })
        .depends_on::<LaterAsyncGlobalConfig>()])
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.log.lock().unwrap().push("consumer-init");
        Ok(())
    }
}

#[derive(Debug)]
struct LaterAsyncGlobalModule {
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Module for LaterAsyncGlobalModule {
    fn name(&self) -> &'static str {
        "later-async-global"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let log = Arc::clone(&self.log);
        Ok(vec![ProviderDefinition::async_factory::<
            LaterAsyncGlobalConfig,
            _,
            _,
        >(move |_| {
            let log = Arc::clone(&log);
            async move {
                log.lock().unwrap().push("config-factory");
                Ok(LaterAsyncGlobalConfig { value: "ready" })
            }
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<LaterAsyncGlobalConfig>()])
    }

    fn is_global(&self) -> bool {
        true
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        self.log.lock().unwrap().push("global-init");
        Ok(())
    }
}

#[derive(Debug)]
struct LaterRequestGlobal;

#[derive(Debug)]
struct InvalidAsyncContextConsumer;

#[derive(Debug)]
struct InvalidAsyncContextConsumerModule {
    calls: Arc<std::sync::atomic::AtomicUsize>,
}

impl Module for InvalidAsyncContextConsumerModule {
    fn name(&self) -> &'static str {
        "invalid-async-context-consumer"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let calls = Arc::clone(&self.calls);
        Ok(vec![ProviderDefinition::async_factory::<
            InvalidAsyncContextConsumer,
            _,
            _,
        >(move |module_ref| {
            let calls = Arc::clone(&calls);
            async move {
                calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let _ = module_ref.get::<LaterRequestGlobal>()?;
                Ok(InvalidAsyncContextConsumer)
            }
        })
        .depends_on::<LaterRequestGlobal>()])
    }
}

#[derive(Debug)]
struct LaterRequestGlobalModule;

impl Module for LaterRequestGlobalModule {
    fn name(&self) -> &'static str {
        "later-request-global"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::request_scoped::<
            LaterRequestGlobal,
            _,
        >(|_| Ok(LaterRequestGlobal))])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<LaterRequestGlobal>()])
    }

    fn is_global(&self) -> bool {
        true
    }
}

#[tokio::test]
async fn async_finalization_seeds_later_global_dependencies_before_consumers() {
    let log = Arc::new(Mutex::new(Vec::new()));
    let app = BootApplication::builder()
        .import(EarlyAsyncConsumerModule {
            log: Arc::clone(&log),
        })
        .import(LaterAsyncGlobalModule {
            log: Arc::clone(&log),
        })
        .build_async()
        .await
        .unwrap();

    let config = app.get::<LaterAsyncGlobalConfig>().unwrap();
    let consumer = app.get::<EarlyAsyncConsumer>().unwrap();

    assert_eq!(config.value, "ready");
    assert!(Arc::ptr_eq(&config, &consumer.config));
    assert_eq!(
        log.lock().unwrap().as_slice(),
        [
            "config-factory",
            "consumer-factory",
            "consumer-init",
            "global-init",
        ]
    );
}

#[tokio::test]
async fn full_graph_validation_rejects_contextual_async_providers_before_factories_run() {
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let result = BootApplication::builder()
        .import(InvalidAsyncContextConsumerModule {
            calls: Arc::clone(&calls),
        })
        .import(LaterRequestGlobalModule)
        .build_async()
        .await;

    assert!(
        matches!(result, Err(BootError::Internal(message)) if message.contains("cannot depend on a request-context provider"))
    );
    assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
}
