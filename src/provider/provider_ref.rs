use super::{ContextId, ModuleRef, ProviderToken};
use crate::Result;
use std::fmt;
use std::marker::PhantomData;
use std::sync::Arc;

/// Lazy provider handle for forward-reference-style dependencies.
#[derive(Clone)]
pub struct ProviderRef<T> {
    module_ref: ModuleRef,
    token: ProviderToken,
    _marker: PhantomData<fn() -> T>,
}

impl<T> fmt::Debug for ProviderRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderRef")
            .field("token", &self.token)
            .finish_non_exhaustive()
    }
}

impl<T> ProviderRef<T>
where
    T: Send + Sync + 'static,
{
    pub fn new(module_ref: ModuleRef, token: ProviderToken) -> Self {
        Self {
            module_ref: module_ref.without_resolution_stack(),
            token,
            _marker: PhantomData,
        }
    }

    pub fn token(&self) -> &ProviderToken {
        &self.token
    }

    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    pub fn get(&self) -> Result<Arc<T>> {
        self.module_ref.get_token(&self.token)
    }

    pub fn get_optional(&self) -> Result<Option<Arc<T>>> {
        self.module_ref.get_optional_token(&self.token)
    }

    pub fn resolve(&self) -> Result<Arc<T>> {
        self.module_ref.resolve_token(&self.token)
    }

    pub fn resolve_with_context(&self, context_id: &ContextId) -> Result<Arc<T>> {
        self.module_ref
            .resolve_token_with_context(&self.token, context_id)
    }

    pub fn resolve_optional(&self) -> Result<Option<Arc<T>>> {
        self.module_ref.resolve_optional_token(&self.token)
    }

    pub fn resolve_optional_with_context(&self, context_id: &ContextId) -> Result<Option<Arc<T>>> {
        self.module_ref
            .resolve_optional_token_with_context(&self.token, context_id)
    }
}
