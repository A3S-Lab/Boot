#![cfg(feature = "cache")]

use a3s_boot::{
    BootApplication, Cache, CacheModule, CacheOptions, Module, ModuleRef, ProviderDefinition,
    Result,
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
