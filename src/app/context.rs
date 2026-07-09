use super::{BootApplication, BootApplicationHandle};
use crate::{ModuleRef, Result};
use std::sync::Arc;

/// Managed application context for provider-only hosts and workers.
pub struct BootApplicationContext {
    pub(crate) handle: BootApplicationHandle,
}

impl BootApplicationContext {
    pub fn app(&self) -> &BootApplication {
        self.handle.app()
    }

    pub fn module_ref(&self) -> &ModuleRef {
        self.handle.module_ref()
    }

    pub fn into_app(self) -> BootApplication {
        self.handle.into_app()
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_named::<T>(token)
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_optional::<T>()
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_optional_named::<T>(token)
    }

    pub fn is_initialized(&self) -> bool {
        self.handle.is_initialized()
    }

    pub async fn init(&mut self) -> Result<()> {
        self.handle.init().await
    }

    pub async fn close(&mut self) -> Result<()> {
        self.handle.close().await
    }

    pub async fn close_with_signal(&mut self, signal: impl Into<String>) -> Result<()> {
        self.handle.close_with_signal(signal).await
    }
}
