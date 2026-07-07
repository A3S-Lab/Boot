use super::{AnyProvider, ModuleRef, ProviderToken};
use crate::Result;
use std::fmt;
use std::sync::Arc;

type ProviderFactory = dyn Fn(&ModuleRef) -> Result<Arc<AnyProvider>> + Send + Sync;

/// Lifetime strategy for provider resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderScope {
    Singleton,
    Request,
    Transient,
}

/// A provider registration, similar to a Nest provider entry.
#[derive(Clone)]
pub struct ProviderDefinition {
    token: ProviderToken,
    factory: Arc<ProviderFactory>,
    scope: ProviderScope,
}

impl fmt::Debug for ProviderDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderDefinition")
            .field("token", &self.token)
            .field("scope", &self.scope)
            .finish_non_exhaustive()
    }
}

impl ProviderDefinition {
    pub fn singleton<T>(value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::from_arc(Arc::new(value))
    }

    pub fn named_singleton<T>(token: impl Into<String>, value: T) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::named_from_arc(token, Arc::new(value))
    }

    pub fn from_arc<T>(value: Arc<T>) -> Self
    where
        T: Send + Sync + 'static,
    {
        let token = ProviderToken::of::<T>();
        Self::named_from_arc(token.as_str(), value)
    }

    pub fn named_from_arc<T>(token: impl Into<String>, value: Arc<T>) -> Self
    where
        T: Send + Sync + 'static,
    {
        let token = ProviderToken::named(token);
        let factory_value = Arc::clone(&value);
        Self {
            token,
            factory: Arc::new(move |_| Ok(Arc::clone(&factory_value) as Arc<AnyProvider>)),
            scope: ProviderScope::Singleton,
        }
    }

    pub fn factory<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self::named_factory(ProviderToken::of::<T>().as_str(), factory)
    }

    pub fn factory_arc<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self::named_factory_arc(ProviderToken::of::<T>().as_str(), factory)
    }

    pub fn named_factory<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self {
            token: ProviderToken::named(token),
            factory: Arc::new(move |module_ref| {
                Ok(Arc::new(factory(module_ref)?) as Arc<AnyProvider>)
            }),
            scope: ProviderScope::Singleton,
        }
    }

    pub fn named_factory_arc<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self {
            token: ProviderToken::named(token),
            factory: Arc::new(move |module_ref| Ok(factory(module_ref)? as Arc<AnyProvider>)),
            scope: ProviderScope::Singleton,
        }
    }

    pub fn transient<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self::factory::<T, _>(factory).with_scope(ProviderScope::Transient)
    }

    pub fn transient_arc<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self::factory_arc::<T, _>(factory).with_scope(ProviderScope::Transient)
    }

    pub fn named_transient<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self::named_factory::<T, _>(token, factory).with_scope(ProviderScope::Transient)
    }

    pub fn named_transient_arc<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self::named_factory_arc::<T, _>(token, factory).with_scope(ProviderScope::Transient)
    }

    pub fn request_scoped<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self::factory::<T, _>(factory).with_scope(ProviderScope::Request)
    }

    pub fn request_scoped_arc<T, F>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self::factory_arc::<T, _>(factory).with_scope(ProviderScope::Request)
    }

    pub fn named_request_scoped<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<T> + Send + Sync + 'static,
    {
        Self::named_factory::<T, _>(token, factory).with_scope(ProviderScope::Request)
    }

    pub fn named_request_scoped_arc<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self::named_factory_arc::<T, _>(token, factory).with_scope(ProviderScope::Request)
    }

    pub fn with_scope(mut self, scope: ProviderScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn token(&self) -> &ProviderToken {
        &self.token
    }

    pub fn scope(&self) -> ProviderScope {
        self.scope
    }

    pub(super) fn build(&self, module_ref: &ModuleRef) -> Result<Arc<AnyProvider>> {
        (self.factory)(module_ref)
    }
}
