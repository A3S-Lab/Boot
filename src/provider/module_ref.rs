use super::{AnyProvider, ProviderDefinition, ProviderToken};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Runtime provider container. This is Boot's Rust equivalent of Nest's ModuleRef.
#[derive(Clone, Default)]
pub struct ModuleRef {
    providers: Arc<RwLock<BTreeMap<ProviderToken, Arc<AnyProvider>>>>,
    visible_scopes: Arc<RwLock<Vec<ModuleRef>>>,
}

impl fmt::Debug for ModuleRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let len = self
            .providers
            .read()
            .map(|providers| providers.len())
            .unwrap_or(0);
        let visible = self
            .visible_scopes
            .read()
            .map(|scopes| scopes.len())
            .unwrap_or(0);
        f.debug_struct("ModuleRef")
            .field("providers", &len)
            .field("visible_scopes", &visible)
            .finish()
    }
}

impl ModuleRef {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&self, definition: ProviderDefinition) -> Result<()> {
        let token = definition.token().clone();
        if self.contains_local(&token)? {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
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

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional_token::<T>(&ProviderToken::of::<T>())
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.get_optional_token::<T>(&ProviderToken::named(token))
    }

    pub fn contains(&self, token: &ProviderToken) -> Result<bool> {
        Ok(self.get_any(token)?.is_some())
    }

    pub fn contains_provider<T>(&self) -> Result<bool>
    where
        T: Send + Sync + 'static,
    {
        self.contains(&ProviderToken::of::<T>())
    }

    pub fn contains_named(&self, token: &str) -> Result<bool> {
        self.contains(&ProviderToken::named(token))
    }

    pub fn tokens(&self) -> Result<Vec<ProviderToken>> {
        let mut tokens = BTreeMap::new();
        self.collect_tokens(&mut tokens)?;
        Ok(tokens.into_keys().collect())
    }

    fn insert_any(&self, token: ProviderToken, value: Arc<AnyProvider>) -> Result<()> {
        let mut providers = self.write_providers()?;
        if providers.contains_key(&token) {
            return Err(BootError::DuplicateProvider(token.to_string()));
        }
        providers.insert(token, value);
        Ok(())
    }

    pub(crate) fn add_visible_scope(&self, module_ref: ModuleRef) -> Result<()> {
        self.write_visible_scopes()?.push(module_ref);
        Ok(())
    }

    pub(crate) fn export_from(&self, module_ref: &ModuleRef, token: &ProviderToken) -> Result<()> {
        let value = module_ref
            .get_any(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;
        self.insert_any(token.clone(), value)
    }

    pub(crate) fn local_tokens(&self) -> Result<Vec<ProviderToken>> {
        Ok(self.read_providers()?.keys().cloned().collect())
    }

    fn get_token<T>(&self, token: &ProviderToken) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        let value = self
            .get_any(token)?
            .ok_or_else(|| BootError::MissingProvider(token.to_string()))?;

        Arc::downcast::<T>(value).map_err(|_| BootError::ProviderTypeMismatch(token.to_string()))
    }

    fn get_optional_token<T>(&self, token: &ProviderToken) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        let value = self.get_any(token)?;
        match value {
            Some(value) => Arc::downcast::<T>(value)
                .map(Some)
                .map_err(|_| BootError::ProviderTypeMismatch(token.to_string())),
            None => Ok(None),
        }
    }

    fn get_any(&self, token: &ProviderToken) -> Result<Option<Arc<AnyProvider>>> {
        if let Some(value) = self.read_providers()?.get(token).cloned() {
            return Ok(Some(value));
        }

        for scope in self.visible_scopes()? {
            if let Some(value) = scope.get_any(token)? {
                return Ok(Some(value));
            }
        }

        Ok(None)
    }

    fn contains_local(&self, token: &ProviderToken) -> Result<bool> {
        Ok(self.read_providers()?.contains_key(token))
    }

    fn collect_tokens(&self, tokens: &mut BTreeMap<ProviderToken, ()>) -> Result<()> {
        for token in self.read_providers()?.keys() {
            tokens.insert(token.clone(), ());
        }
        for scope in self.visible_scopes()? {
            scope.collect_tokens(tokens)?;
        }
        Ok(())
    }

    fn visible_scopes(&self) -> Result<Vec<ModuleRef>> {
        Ok(self.read_visible_scopes()?.clone())
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

    fn read_visible_scopes(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<ModuleRef>>> {
        self.visible_scopes
            .read()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }

    fn write_visible_scopes(&self) -> Result<std::sync::RwLockWriteGuard<'_, Vec<ModuleRef>>> {
        self.visible_scopes
            .write()
            .map_err(|_| BootError::Internal("provider registry lock is poisoned".to_string()))
    }
}
