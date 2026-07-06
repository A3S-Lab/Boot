use super::{AnyProvider, ProviderDefinition, ProviderToken};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Runtime provider container. This is Boot's Rust equivalent of Nest's ModuleRef.
#[derive(Clone, Default)]
pub struct ModuleRef {
    providers: Arc<RwLock<BTreeMap<ProviderToken, Arc<AnyProvider>>>>,
}

impl fmt::Debug for ModuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self
            .providers
            .read()
            .map(|providers| providers.len())
            .unwrap_or(0);
        f.debug_struct("ModuleRef")
            .field("providers", &len)
            .finish()
    }
}

impl ModuleRef {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, definition: ProviderDefinition) -> Result<()> {
        let token = definition.token().clone();
        let value = definition.build(self)?;
        self.insert_any(token, value)
    }

    pub fn insert<T>(&self, value: T) -> Result<()>
    where
        T: Send + Sync + 'static,
    {
        self.insert_arc(Arc::new(value))
    }

    pub fn insert_arc<T>(&self, value: Arc<T>) -> Result<()>
    where
        T: Send + Sync + 'static,
    {
        self.insert_any(ProviderToken::of::<T>(), value as Arc<AnyProvider>)
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_token::<T>(&ProviderToken::of::<T>())
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.get_token::<T>(&ProviderToken::named(token))
    }

    pub fn contains(&self, token: &ProviderToken) -> Result<bool> {
        Ok(self.read_providers()?.contains_key(token))
    }

    pub fn tokens(&self) -> Result<Vec<ProviderToken>> {
        Ok(self.read_providers()?.keys().cloned().collect())
    }

    fn insert_any(&self, token: ProviderToken, value: Arc<AnyProvider>) -> Result<()> {
        let mut providers = self.write_providers()?;
        if providers.contains_key(&token) {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
        providers.insert(token, value);
        Ok(())
    }

    fn get_token<T>(&self, token: &ProviderToken) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let value = self
            .read_providers()?
            .get(token)
            .cloned()
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;

        Arc::downcast::<T>(value).map_err(|_| BootError::ProviderTypeMismatch(token.to_string()))
    }

    fn read_providers(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, BTreeMap<ProviderToken, Arc<AnyProvider>>>> {
        self.providers
            .read()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }

    fn write_providers(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<ProviderToken, Arc<AnyProvider>>>> {
        self.providers
            .write()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }
}
