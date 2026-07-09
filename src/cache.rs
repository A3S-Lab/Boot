use crate::{
    BootError, BootResponse, BoxFuture, ExecutionContext, HttpMethod, Interceptor, Module,
    ProviderDefinition, ProviderToken, Result,
};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub const CACHE_KEY_METADATA: &str = "cache.key";
pub const CACHE_TTL_METADATA: &str = "cache.ttl_ms";
pub const CACHE_DISABLED_METADATA: &str = "cache.disabled";

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

/// Nest-style response cache interceptor for HTTP GET routes.
#[derive(Clone)]
pub struct CacheInterceptor {
    source: CacheInterceptorSource,
}

impl fmt::Debug for CacheInterceptor {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CacheInterceptor")
            .field("source", &self.source)
            .finish()
    }
}

#[derive(Clone)]
enum CacheInterceptorSource {
    Cache(Arc<Cache>),
    Provider,
    NamedProvider(String),
}

impl fmt::Debug for CacheInterceptorSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cache(_) => f.write_str("Cache"),
            Self::Provider => f.write_str("Provider"),
            Self::NamedProvider(token) => f.debug_tuple("NamedProvider").field(token).finish(),
        }
    }
}

impl CacheInterceptor {
    pub fn new(cache: Cache) -> Self {
        Self::from_cache_arc(Arc::new(cache))
    }

    pub fn from_cache_arc(cache: Arc<Cache>) -> Self {
        Self {
            source: CacheInterceptorSource::Cache(cache),
        }
    }

    pub fn from_provider() -> Self {
        Self {
            source: CacheInterceptorSource::Provider,
        }
    }

    pub fn from_named_provider(token: impl Into<String>) -> Self {
        Self {
            source: CacheInterceptorSource::NamedProvider(token.into()),
        }
    }

    fn cache(&self, context: &ExecutionContext) -> Result<Arc<Cache>> {
        match &self.source {
            CacheInterceptorSource::Cache(cache) => Ok(Arc::clone(cache)),
            CacheInterceptorSource::Provider => context.request.get::<Cache>(),
            CacheInterceptorSource::NamedProvider(token) => {
                context.request.get_named::<Cache>(token)
            }
        }
    }
}

impl Interceptor for CacheInterceptor {
    fn short_circuit(
        &self,
        context: ExecutionContext,
    ) -> BoxFuture<'static, Result<Option<BootResponse>>> {
        let cache = self.cache(&context);
        Box::pin(async move {
            let Some(key) = cache_key_for_context(&context)? else {
                return Ok(None);
            };
            let Some(response) = cache?.get::<CachedHttpResponse>(&key)? else {
                return Ok(None);
            };
            Ok(Some(response.into_response()))
        })
    }

    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let cache = self.cache(&context);
        Box::pin(async move {
            let Some(key) = cache_key_for_context(&context)? else {
                return Ok(response);
            };
            if !is_cacheable_response(&response) {
                return Ok(response);
            }

            let cached = CachedHttpResponse::from_response(&response);
            let cache = cache?;
            if let Some(ttl) = cache_ttl_for_context(&context)? {
                cache.set_with_ttl(key, &cached, Some(ttl))?;
            } else {
                cache.set(key, &cached)?;
            }
            Ok(response)
        })
    }
}

fn cache_key_for_context(context: &ExecutionContext) -> Result<Option<String>> {
    if context.protocol != crate::ExecutionProtocol::Http || context.method != HttpMethod::Get {
        return Ok(None);
    }
    if context
        .metadata_as::<bool>(CACHE_DISABLED_METADATA)?
        .unwrap_or(false)
    {
        return Ok(None);
    }
    if let Some(key) = context.metadata_as::<String>(CACHE_KEY_METADATA)? {
        return Ok(Some(key));
    }

    let query = context
        .request
        .query_string()
        .map(|query| format!("?{query}"))
        .unwrap_or_default();
    Ok(Some(format!(
        "{}:{}{}",
        context.method.as_str(),
        context.request_path,
        query
    )))
}

fn cache_ttl_for_context(context: &ExecutionContext) -> Result<Option<Duration>> {
    Ok(context
        .metadata_as::<u64>(CACHE_TTL_METADATA)?
        .map(Duration::from_millis))
}

fn is_cacheable_response(response: &BootResponse) -> bool {
    (200..300).contains(&response.status()) && !response.is_streaming()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct CachedHttpResponse {
    status: u16,
    headers: BTreeMap<String, String>,
    appended_headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl CachedHttpResponse {
    fn from_response(response: &BootResponse) -> Self {
        Self {
            status: response.status,
            headers: response.headers.clone(),
            appended_headers: response.appended_headers.clone(),
            body: response.body.clone(),
        }
    }

    fn into_response(self) -> BootResponse {
        let mut response = BootResponse::new(self.status, self.body).with_headers(self.headers);
        for (name, value) in self.appended_headers {
            response = response.append_header(name, value);
        }
        response
    }
}
