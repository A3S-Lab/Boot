use crate::{BootError, Module, ProviderDefinition, ProviderToken, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Cache configuration shared by cache modules and cache instances.
#[derive(Debug, Clone, Copy, Default)]
pub struct CacheOptions {
    pub default_ttl: Option<Duration>,
}

impl CacheOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_default_ttl(mut self, ttl: Duration) -> Self {
        self.default_ttl = Some(ttl);
        self
    }
}

/// Storage backend used by [`Cache`].
pub trait CacheStore: Send + Sync + 'static {
    fn get(&self, key: &str) -> Result<Option<Value>>;

    fn set(&self, key: String, value: Value, ttl: Option<Duration>) -> Result<()>;

    fn remove(&self, key: &str) -> Result<bool>;

    fn clear(&self) -> Result<()>;
}

/// Typed cache facade exposed as a provider by [`CacheModule`].
#[derive(Clone)]
pub struct Cache {
    store: Arc<dyn CacheStore>,
    options: CacheOptions,
}

impl fmt::Debug for Cache {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Cache")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Cache {
    pub fn new<S>(store: S) -> Self
    where
        S: CacheStore,
    {
        Self::from_store_arc(Arc::new(store))
    }

    pub fn from_store_arc(store: Arc<dyn CacheStore>) -> Self {
        Self {
            store,
            options: CacheOptions::default(),
        }
    }

    pub fn in_memory() -> Self {
        Self::new(InMemoryCacheStore::new())
    }

    pub fn with_options(mut self, options: CacheOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_default_ttl(self, ttl: Duration) -> Self {
        self.with_options(CacheOptions::new().with_default_ttl(ttl))
    }

    pub fn get<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.get_value(key)? else {
            return Ok(None);
        };
        serde_json::from_value(value).map(Some).map_err(|error| {
            BootError::Internal(format!("invalid cached value for {key}: {error}"))
        })
    }

    pub fn get_value(&self, key: &str) -> Result<Option<Value>> {
        self.store.get(key)
    }

    pub fn set<T>(&self, key: impl Into<String>, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        self.set_with_ttl(key, value, self.options.default_ttl)
    }

    pub fn set_with_ttl<T>(
        &self,
        key: impl Into<String>,
        value: &T,
        ttl: Option<Duration>,
    ) -> Result<()>
    where
        T: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!("failed to serialize cache value: {error}"))
        })?;
        self.store.set(key, value, ttl)
    }

    pub fn get_or_insert_with<T, F>(&self, key: impl Into<String>, factory: F) -> Result<T>
    where
        T: DeserializeOwned + Serialize,
        F: FnOnce() -> Result<T>,
    {
        self.get_or_insert_with_ttl(key, self.options.default_ttl, factory)
    }

    pub fn get_or_insert_with_ttl<T, F>(
        &self,
        key: impl Into<String>,
        ttl: Option<Duration>,
        factory: F,
    ) -> Result<T>
    where
        T: DeserializeOwned + Serialize,
        F: FnOnce() -> Result<T>,
    {
        let key = key.into();
        if let Some(value) = self.get::<T>(&key)? {
            return Ok(value);
        }

        let value = factory()?;
        self.set_with_ttl(&key, &value, ttl)?;
        Ok(value)
    }

    pub fn remove(&self, key: &str) -> Result<bool> {
        self.store.remove(key)
    }

    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }
}

/// In-memory cache store suitable for tests and single-process services.
#[derive(Debug, Clone, Default)]
pub struct InMemoryCacheStore {
    entries: Arc<RwLock<BTreeMap<String, CacheEntry>>>,
}

impl InMemoryCacheStore {
    pub fn new() -> Self {
        Self::default()
    }
}

impl CacheStore for InMemoryCacheStore {
    fn get(&self, key: &str) -> Result<Option<Value>> {
        let mut entries = self.write_entries()?;
        let Some(entry) = entries.get(key) else {
            return Ok(None);
        };

        if entry.is_expired() {
            entries.remove(key);
            return Ok(None);
        }

        Ok(Some(entry.value.clone()))
    }

    fn set(&self, key: String, value: Value, ttl: Option<Duration>) -> Result<()> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        self.write_entries()?
            .insert(key, CacheEntry { value, expires_at });
        Ok(())
    }

    fn remove(&self, key: &str) -> Result<bool> {
        Ok(self.write_entries()?.remove(key).is_some())
    }

    fn clear(&self) -> Result<()> {
        self.write_entries()?.clear();
        Ok(())
    }
}

impl InMemoryCacheStore {
    fn write_entries(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, CacheEntry>>> {
        self.entries
            .write()
            .map_err(|_| BootError::Internal("cache store lock is poisoned".to_string()))
    }
}

#[derive(Debug, Clone)]
struct CacheEntry {
    value: Value,
    expires_at: Option<Instant>,
}

impl CacheEntry {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

/// Module that registers and exports a [`Cache`] provider.
#[derive(Clone)]
pub struct CacheModule {
    name: &'static str,
    token: ProviderToken,
    cache: Arc<Cache>,
    global: bool,
}

impl fmt::Debug for CacheModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl CacheModule {
    pub fn in_memory(name: &'static str) -> Self {
        Self::from_cache(name, Cache::in_memory())
    }

    pub fn in_memory_with_options(name: &'static str, options: CacheOptions) -> Self {
        Self::from_cache(name, Cache::in_memory().with_options(options))
    }

    pub fn from_cache(name: &'static str, cache: Cache) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Cache>(),
            cache: Arc::new(cache),
            global: false,
        }
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for CacheModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.cache),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}
