use super::AnyProvider;
use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

pub(crate) type ProviderCache = Arc<RwLock<BTreeMap<ProviderCacheKey, Arc<AnyProvider>>>>;

static NEXT_PROVIDER_CACHE_KEY: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ProviderCacheKey(u64);

impl ProviderCacheKey {
    pub(crate) fn next() -> Self {
        Self(NEXT_PROVIDER_CACHE_KEY.fetch_add(1, Ordering::Relaxed))
    }
}

pub(crate) fn new_provider_cache() -> ProviderCache {
    Arc::new(RwLock::new(BTreeMap::new()))
}
