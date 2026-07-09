use crate::{ModuleRef, Result};
use std::sync::Arc;

/// Context passed to CQRS handlers.
#[derive(Debug, Clone)]
pub struct CqrsContext {
    module_ref: ModuleRef,
}

impl CqrsContext {
    pub fn new(module_ref: ModuleRef) -> Self {
        Self { module_ref }
    }

    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_named::<T>(token)
    }
}
