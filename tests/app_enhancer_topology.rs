use a3s_boot::{
    BootApplication, BootRequest, BootResponse, BoxFuture, ExecutionContext, FromModuleRef, Guard,
    HttpMethod, Module, ModuleRef, OpenApiInfo, ProviderDefinition, ProviderDependency,
    ProviderToken, Result, RouteDefinition,
};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Debug)]
struct AliasedRequestGuard {
    activations: Arc<AtomicUsize>,
}

impl Guard for AliasedRequestGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct AliasedRequestGuardModule {
    factory_calls: Arc<AtomicUsize>,
    activations: Arc<AtomicUsize>,
}

impl Module for AliasedRequestGuardModule {
    fn name(&self) -> &'static str {
        "aliased-request-app-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let factory_calls = Arc::clone(&self.factory_calls);
        let activations = Arc::clone(&self.activations);
        Ok(vec![
            ProviderDefinition::request_scoped::<AliasedRequestGuard, _>(move |_| {
                factory_calls.fetch_add(1, Ordering::SeqCst);
                Ok(AliasedRequestGuard {
                    activations: Arc::clone(&activations),
                })
            }),
            ProviderDefinition::named_alias(
                "application-guard-alias",
                ProviderToken::of::<AliasedRequestGuard>(),
            )
            .with_app_guard::<AliasedRequestGuard>(),
        ])
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get(
            "/aliased-app-guard",
            |_| async { Ok(BootResponse::text("aliased")) },
        )?])
    }
}

#[tokio::test]
async fn alias_marker_preserves_request_scoped_app_enhancer_resolution() {
    let factory_calls = Arc::new(AtomicUsize::new(0));
    let activations = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(AliasedRequestGuardModule {
            factory_calls: Arc::clone(&factory_calls),
            activations: Arc::clone(&activations),
        })
        .build()
        .unwrap();

    for _ in 0..2 {
        let response = app
            .call(BootRequest::new(HttpMethod::Get, "/aliased-app-guard"))
            .await
            .unwrap();
        assert_eq!(response.body_text().unwrap(), "aliased");
    }

    assert_eq!(factory_calls.load(Ordering::SeqCst), 2);
    assert_eq!(activations.load(Ordering::SeqCst), 2);
}

#[derive(Debug)]
struct ValueMarkedAppGuard {
    activations: Arc<AtomicUsize>,
}

impl Guard for ValueMarkedAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.activations.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct ValueMarkedAppGuardModule {
    activations: Arc<AtomicUsize>,
}

impl Module for ValueMarkedAppGuardModule {
    fn name(&self) -> &'static str {
        "value-marked-app-guard"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(ValueMarkedAppGuard {
            activations: Arc::clone(&self.activations),
        })
        .with_app_guard::<ValueMarkedAppGuard>()])
    }
}

#[tokio::test]
async fn value_marked_app_enhancer_applies_to_framework_provided_routes() {
    let activations = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .serve_openapi(
            "/provider-openapi.json",
            OpenApiInfo::new("Provider Enhancers", "1.0.0"),
        )
        .import(ValueMarkedAppGuardModule {
            activations: Arc::clone(&activations),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/provider-openapi.json"))
        .await
        .unwrap();

    assert_eq!(response.status, 200);
    assert_eq!(activations.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct PrivateEnhancerDependency {
    events: Arc<Mutex<Vec<&'static str>>>,
}

#[derive(Debug)]
struct PrivateDependencyAppGuard {
    dependency: Arc<PrivateEnhancerDependency>,
}

impl FromModuleRef for PrivateDependencyAppGuard {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            dependency: module_ref.get::<PrivateEnhancerDependency>()?,
        })
    }

    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        Some(vec![
            ProviderDependency::typed::<PrivateEnhancerDependency>(),
        ])
    }
}

impl Guard for PrivateDependencyAppGuard {
    fn can_activate(&self, _context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        self.dependency.events.lock().unwrap().push("provider");
        Box::pin(async { Ok(true) })
    }
}

#[derive(Debug)]
struct PrivateEnhancerOwnerModule {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Module for PrivateEnhancerOwnerModule {
    fn name(&self) -> &'static str {
        "private-enhancer-owner"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::singleton(PrivateEnhancerDependency {
                events: Arc::clone(&self.events),
            }),
            ProviderDefinition::app_guard::<PrivateDependencyAppGuard>(),
        ])
    }
}

#[derive(Debug)]
struct IsolatedTargetModule {
    events: Arc<Mutex<Vec<&'static str>>>,
}

impl Module for IsolatedTargetModule {
    fn name(&self) -> &'static str {
        "isolated-app-enhancer-target"
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        let local_events = Arc::clone(&self.events);
        let route = RouteDefinition::get("/isolated-target", |request: BootRequest| async move {
            let visibility = if request
                .get_optional::<PrivateEnhancerDependency>()?
                .is_some()
            {
                "visible"
            } else {
                "hidden"
            };
            Ok(BootResponse::text(visibility))
        })?
        .with_guard(move |_| {
            let local_events = Arc::clone(&local_events);
            async move {
                local_events.lock().unwrap().push("local");
                Ok(true)
            }
        });
        Ok(vec![route])
    }
}

#[tokio::test]
async fn provider_enhancer_uses_declaring_module_without_widening_target_visibility() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let builder_events = Arc::clone(&events);
    let app = BootApplication::builder()
        .use_global_guard(move |_| {
            let builder_events = Arc::clone(&builder_events);
            async move {
                builder_events.lock().unwrap().push("builder");
                Ok(true)
            }
        })
        .import(PrivateEnhancerOwnerModule {
            events: Arc::clone(&events),
        })
        .import(IsolatedTargetModule {
            events: Arc::clone(&events),
        })
        .build()
        .unwrap();

    let response = app
        .call(BootRequest::new(HttpMethod::Get, "/isolated-target"))
        .await
        .unwrap();

    assert_eq!(response.body_text().unwrap(), "hidden");
    assert_eq!(
        events.lock().unwrap().as_slice(),
        ["builder", "provider", "local"]
    );
}
