use super::cache::ProviderCache;
use crate::{BootError, BootRequest, Result};
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Weak};

static NEXT_CONTEXT_ID: AtomicU64 = AtomicU64::new(1);

/// Identity and provider cache for one dependency-injection resolution context.
#[derive(Clone)]
pub struct ContextId {
    id: u64,
    state: ContextIdStateRef,
}

#[derive(Clone)]
enum ContextIdStateRef {
    Strong(Arc<ContextIdState>),
    Weak(Weak<ContextIdState>),
}

struct ContextIdState {
    cache: ProviderCache,
}

impl ContextId {
    fn new(id: u64) -> Self {
        Self {
            id,
            state: ContextIdStateRef::Strong(Arc::new(ContextIdState {
                cache: ProviderCache::new(),
            })),
        }
    }

    /// Numeric identity useful for diagnostics and correlation.
    pub fn id(&self) -> u64 {
        self.id
    }

    pub(crate) fn cache(&self) -> Result<ProviderCache> {
        let state = match &self.state {
            ContextIdStateRef::Strong(state) => Arc::clone(state),
            ContextIdStateRef::Weak(state) => state.upgrade().ok_or_else(|| {
                BootError::Internal(format!(
                    "dependency-injection context {} is no longer active",
                    self.id
                ))
            })?,
        };
        Ok(state.cache.clone())
    }

    pub(crate) fn downgrade(&self) -> Self {
        let state = match &self.state {
            ContextIdStateRef::Strong(state) => ContextIdStateRef::Weak(Arc::downgrade(state)),
            ContextIdStateRef::Weak(state) => ContextIdStateRef::Weak(Weak::clone(state)),
        };
        Self { id: self.id, state }
    }
}

impl fmt::Debug for ContextId {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("ContextId")
            .field("id", &self.id())
            .finish_non_exhaustive()
    }
}

impl PartialEq for ContextId {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

impl Eq for ContextId {}

impl PartialOrd for ContextId {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for ContextId {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.id().cmp(&other.id())
    }
}

impl Hash for ContextId {
    fn hash<H>(&self, state: &mut H)
    where
        H: Hasher,
    {
        self.id().hash(state);
    }
}

/// Creates and discovers Nest-style dependency-injection context identities.
#[derive(Debug, Clone, Copy, Default)]
pub struct ContextIdFactory;

impl ContextIdFactory {
    /// Create an isolated dependency-injection context.
    pub fn create() -> ContextId {
        ContextId::new(NEXT_CONTEXT_ID.fetch_add(1, Ordering::Relaxed))
    }

    /// Return the context already attached to a request, or create a fresh one.
    pub fn get_by_request(request: &BootRequest) -> ContextId {
        request.context_id().cloned().unwrap_or_else(Self::create)
    }
}
