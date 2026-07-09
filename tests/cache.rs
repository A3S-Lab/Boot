#![cfg(feature = "cache")]

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, Cache, CacheInterceptor, CacheModule, CacheOptions,
    ControllerDefinition, HttpMethod, Module, ModuleRef, ProviderDefinition, Result,
    RouteDefinition,
};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct CatDto {
    id: String,
    name: String,
}

#[test]
fn cache_sets_gets_removes_and_clears_typed_values() {
    let cache = Cache::in_memory();
    let cat = CatDto {
        id: "1".to_string(),
        name: "Milo".to_string(),
    };

    cache.set("cat:1", &cat).unwrap();
    let cached = cache.get::<CatDto>("cat:1").unwrap();
    let removed = cache.remove("cat:1").unwrap();
    let missing = cache.get::<CatDto>("cat:1").unwrap();

    cache.set("cat:2", &cat).unwrap();
    cache.clear().unwrap();

    assert_eq!(cached, Some(cat));
    assert!(removed);
    assert_eq!(missing, None);
    assert_eq!(cache.get::<CatDto>("cat:2").unwrap(), None);
}

#[test]
fn cache_default_ttl_expires_values() {
    let cache = Cache::in_memory().with_default_ttl(Duration::from_millis(10));

    cache
        .set(
            "cat:ttl",
            &CatDto {
                id: "ttl".to_string(),
                name: "Short".to_string(),
            },
        )
        .unwrap();

    assert!(cache.get::<CatDto>("cat:ttl").unwrap().is_some());
    std::thread::sleep(Duration::from_millis(25));
    assert_eq!(cache.get::<CatDto>("cat:ttl").unwrap(), None);
}

#[test]
fn cache_get_or_insert_uses_cached_values() {
    let cache = Cache::in_memory();
    let calls = AtomicUsize::new(0);

    let first = cache
        .get_or_insert_with("cat:1", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(CatDto {
                id: "1".to_string(),
                name: "Milo".to_string(),
            })
        })
        .unwrap();
    let second = cache
        .get_or_insert_with("cat:1", || {
            calls.fetch_add(1, Ordering::SeqCst);
            Ok(CatDto {
                id: "1".to_string(),
                name: "Luna".to_string(),
            })
        })
        .unwrap();

    assert_eq!(first, second);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[derive(Debug)]
struct CachedCatsService {
    cache: Arc<Cache>,
}

impl CachedCatsService {
    fn find_one(&self, id: &str) -> Result<CatDto> {
        self.cache.get_or_insert_with(format!("cat:{id}"), || {
            Ok(CatDto {
                id: id.to_string(),
                name: "Milo".to_string(),
            })
        })
    }
}

#[derive(Debug)]
struct UsesCacheModule {
    cache_module: CacheModule,
}

impl Module for UsesCacheModule {
    fn name(&self) -> &'static str {
        "uses-cache"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.cache_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<CachedCatsService, _>(
            |module_ref: &ModuleRef| {
                Ok(CachedCatsService {
                    cache: module_ref.get::<Cache>()?,
                })
            },
        )])
    }
}

#[test]
fn cache_module_exports_cache_to_importing_modules() {
    let app = BootApplication::builder()
        .import(UsesCacheModule {
            cache_module: CacheModule::in_memory_with_options(
                "cache",
                CacheOptions::new().with_default_ttl(Duration::from_secs(60)),
            ),
        })
        .build()
        .unwrap();

    let cats = app.get::<CachedCatsService>().unwrap();
    let first = cats.find_one("1").unwrap();
    let second = cats.find_one("1").unwrap();

    assert_eq!(first, second);
}

#[derive(Debug)]
struct UsesNamedCacheModule {
    cache_module: CacheModule,
}

impl Module for UsesNamedCacheModule {
    fn name(&self) -> &'static str {
        "uses-named-cache"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.cache_module.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<CachedCatsService, _>(
            |module_ref: &ModuleRef| {
                Ok(CachedCatsService {
                    cache: module_ref.get_named::<Cache>("app-cache")?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct UsesGlobalCacheModule;

impl Module for UsesGlobalCacheModule {
    fn name(&self) -> &'static str {
        "uses-global-cache"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<CachedCatsService, _>(
            |module_ref: &ModuleRef| {
                Ok(CachedCatsService {
                    cache: module_ref.get::<Cache>()?,
                })
            },
        )])
    }
}

#[derive(Debug)]
struct MacroCachedController {
    shared_calls: Arc<AtomicUsize>,
    ttl_calls: Arc<AtomicUsize>,
}

#[a3s_boot::controller("/macro-cache")]
#[a3s_boot::cache_ttl(milliseconds = 10)]
impl MacroCachedController {
    #[a3s_boot::get("/shared", raw)]
    #[a3s_boot::cache_key("macro-shared")]
    async fn shared(&self, #[a3s_boot::request] request: BootRequest) -> Result<BootResponse> {
        let call = self.shared_calls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(BootResponse::text(format!(
            "{call}:{}",
            request.query_param("q").unwrap_or("missing")
        )))
    }

    #[a3s_boot::get("/ttl", raw)]
    async fn ttl(&self) -> Result<BootResponse> {
        let call = self.ttl_calls.fetch_add(1, Ordering::SeqCst) + 1;
        Ok(BootResponse::text(call.to_string()))
    }
}

#[derive(Debug)]
struct MacroCacheModule {
    controller: Arc<MacroCachedController>,
    cache: Cache,
}

impl Module for MacroCacheModule {
    fn name(&self) -> &'static str {
        "macro-cache"
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![Arc::clone(&self.controller)
            .controller()?
            .with_interceptor(CacheInterceptor::new(
                self.cache.clone(),
            ))])
    }
}

#[test]
fn cache_module_supports_named_and_global_exports() {
    let named = BootApplication::builder()
        .import(UsesNamedCacheModule {
            cache_module: CacheModule::in_memory("named-cache").named("app-cache"),
        })
        .build()
        .unwrap();
    assert!(named.get::<CachedCatsService>().is_ok());
    assert!(named.get_optional::<Cache>().unwrap().is_none());
    assert!(named.get_named::<Cache>("app-cache").is_ok());

    let global = BootApplication::builder()
        .import(CacheModule::in_memory("global-cache").global())
        .import(UsesGlobalCacheModule)
        .build()
        .unwrap();
    assert!(global.get::<CachedCatsService>().is_ok());
    assert!(global.get::<Cache>().is_ok());
}

#[tokio::test]
async fn cache_interceptor_serves_cached_get_responses_by_request_url() {
    let cache = Cache::in_memory();
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/cached", move |request: BootRequest| {
                let observed = Arc::clone(&observed);
                async move {
                    let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(BootResponse::text(format!(
                        "{call}:{}",
                        request.query_param("q").unwrap_or("missing")
                    )))
                }
            })
            .unwrap()
            .with_interceptor(CacheInterceptor::new(cache)),
        )
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/cached?q=one"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/cached?q=one"))
        .await
        .unwrap();
    let different_query = app
        .call(BootRequest::new(HttpMethod::Get, "/cached?q=two"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1:one");
    assert_eq!(second.body_text().unwrap(), "1:one");
    assert_eq!(different_query.body_text().unwrap(), "2:two");
    assert_eq!(calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cache_interceptor_supports_explicit_keys_ttl_and_opt_out() {
    let shared_key_calls = Arc::new(AtomicUsize::new(0));
    let ttl_calls = Arc::new(AtomicUsize::new(0));
    let uncached_calls = Arc::new(AtomicUsize::new(0));
    let shared_key_observed = Arc::clone(&shared_key_calls);
    let ttl_observed = Arc::clone(&ttl_calls);
    let uncached_observed = Arc::clone(&uncached_calls);
    let cache = Cache::in_memory();
    let app = BootApplication::builder()
        .route(
            RouteDefinition::get("/cache-key", move |request: BootRequest| {
                let observed = Arc::clone(&shared_key_observed);
                async move {
                    let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(BootResponse::text(format!(
                        "{call}:{}",
                        request.query_param("q").unwrap_or("missing")
                    )))
                }
            })
            .unwrap()
            .with_cache_key("shared-cache-key")
            .with_interceptor(CacheInterceptor::new(cache.clone())),
        )
        .route(
            RouteDefinition::get("/cache-ttl", move |_| {
                let observed = Arc::clone(&ttl_observed);
                async move {
                    let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(BootResponse::text(call.to_string()))
                }
            })
            .unwrap()
            .with_cache_ttl(Duration::from_millis(10))
            .with_interceptor(CacheInterceptor::new(cache.clone())),
        )
        .route(
            RouteDefinition::get("/cache-off", move |_| {
                let observed = Arc::clone(&uncached_observed);
                async move {
                    let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(BootResponse::text(call.to_string()))
                }
            })
            .unwrap()
            .without_cache()
            .with_interceptor(CacheInterceptor::new(cache)),
        )
        .build()
        .unwrap();

    let first_keyed = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-key?q=one"))
        .await
        .unwrap();
    let second_keyed = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-key?q=two"))
        .await
        .unwrap();
    assert_eq!(first_keyed.body_text().unwrap(), "1:one");
    assert_eq!(second_keyed.body_text().unwrap(), "1:one");
    assert_eq!(shared_key_calls.load(Ordering::SeqCst), 1);

    let first_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-ttl"))
        .await
        .unwrap();
    let second_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-ttl"))
        .await
        .unwrap();
    std::thread::sleep(Duration::from_millis(25));
    let expired_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-ttl"))
        .await
        .unwrap();
    assert_eq!(first_ttl.body_text().unwrap(), "1");
    assert_eq!(second_ttl.body_text().unwrap(), "1");
    assert_eq!(expired_ttl.body_text().unwrap(), "2");
    assert_eq!(ttl_calls.load(Ordering::SeqCst), 2);

    let first_uncached = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-off"))
        .await
        .unwrap();
    let second_uncached = app
        .call(BootRequest::new(HttpMethod::Get, "/cache-off"))
        .await
        .unwrap();
    assert_eq!(first_uncached.body_text().unwrap(), "1");
    assert_eq!(second_uncached.body_text().unwrap(), "2");
    assert_eq!(uncached_calls.load(Ordering::SeqCst), 2);
}

#[tokio::test]
async fn cache_interceptor_can_resolve_cache_from_provider_scope() {
    let calls = Arc::new(AtomicUsize::new(0));
    let observed = Arc::clone(&calls);
    let app = BootApplication::builder()
        .import(CacheModule::in_memory("cache").global())
        .route(
            RouteDefinition::get("/provider-cache", move |_| {
                let observed = Arc::clone(&observed);
                async move {
                    let call = observed.fetch_add(1, Ordering::SeqCst) + 1;
                    Ok(BootResponse::text(call.to_string()))
                }
            })
            .unwrap()
            .with_interceptor(CacheInterceptor::from_provider()),
        )
        .build()
        .unwrap();

    let first = app
        .call(BootRequest::new(HttpMethod::Get, "/provider-cache"))
        .await
        .unwrap();
    let second = app
        .call(BootRequest::new(HttpMethod::Get, "/provider-cache"))
        .await
        .unwrap();

    assert_eq!(first.body_text().unwrap(), "1");
    assert_eq!(second.body_text().unwrap(), "1");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn cache_key_and_ttl_macros_register_cache_metadata() {
    let shared_calls = Arc::new(AtomicUsize::new(0));
    let ttl_calls = Arc::new(AtomicUsize::new(0));
    let app = BootApplication::builder()
        .import(MacroCacheModule {
            controller: Arc::new(MacroCachedController {
                shared_calls: Arc::clone(&shared_calls),
                ttl_calls: Arc::clone(&ttl_calls),
            }),
            cache: Cache::in_memory(),
        })
        .build()
        .unwrap();

    let first_keyed = app
        .call(BootRequest::new(
            HttpMethod::Get,
            "/macro-cache/shared?q=one",
        ))
        .await
        .unwrap();
    let second_keyed = app
        .call(BootRequest::new(
            HttpMethod::Get,
            "/macro-cache/shared?q=two",
        ))
        .await
        .unwrap();

    assert_eq!(first_keyed.body_text().unwrap(), "1:one");
    assert_eq!(second_keyed.body_text().unwrap(), "1:one");
    assert_eq!(shared_calls.load(Ordering::SeqCst), 1);

    let first_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/macro-cache/ttl"))
        .await
        .unwrap();
    let second_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/macro-cache/ttl"))
        .await
        .unwrap();
    std::thread::sleep(Duration::from_millis(25));
    let expired_ttl = app
        .call(BootRequest::new(HttpMethod::Get, "/macro-cache/ttl"))
        .await
        .unwrap();

    assert_eq!(first_ttl.body_text().unwrap(), "1");
    assert_eq!(second_ttl.body_text().unwrap(), "1");
    assert_eq!(expired_ttl.body_text().unwrap(), "2");
    assert_eq!(ttl_calls.load(Ordering::SeqCst), 2);
}
