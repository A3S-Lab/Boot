use super::{AnyProvider, ModuleRef, ProviderToken};
use crate::{BootError, BoxFuture, Result};
use std::fmt;
use std::future::Future;
use std::sync::Arc;

type ProviderFactory = dyn Fn(&ModuleRef) -> Result<Arc<AnyProvider>> + Send + Sync;
type AsyncProviderFactory =
    dyn Fn(ModuleRef) -> BoxFuture<'static, Result<Arc<AnyProvider>>> + Send + Sync;
type ProviderModuleInitHook = dyn Fn(Arc<AnyProvider>, &ModuleRef) -> Result<()> + Send + Sync;
type ProviderApplicationHook =
    dyn Fn(Arc<AnyProvider>, ModuleRef) -> BoxFuture<'static, Result<()>> + Send + Sync;

/// Lifecycle hook for singleton providers that need synchronous module init work.
pub trait ProviderOnModuleInit: Send + Sync + 'static {
    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        Ok(())
    }
}

/// Lifecycle hook for singleton providers that need async startup work.
pub trait ProviderOnApplicationBootstrap: Send + Sync + 'static {
    fn on_application_bootstrap(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Lifecycle hook for singleton providers that need async shutdown cleanup.
pub trait ProviderOnApplicationShutdown: Send + Sync + 'static {
    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Builds an injectable value from the module provider graph.
pub trait FromModuleRef: Sized + Send + Sync + 'static {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self>;
}

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
    factory: ProviderFactoryKind,
    scope: ProviderScope,
    lifecycle: ProviderLifecycleHooks,
    alias_target: Option<ProviderToken>,
}

#[derive(Clone)]
enum ProviderFactoryKind {
    Sync(Arc<ProviderFactory>),
    Async(Arc<AsyncProviderFactory>),
}

#[derive(Clone, Default)]
pub(super) struct ProviderLifecycleHooks {
    on_module_init: Option<Arc<ProviderModuleInitHook>>,
    on_application_bootstrap: Option<Arc<ProviderApplicationHook>>,
    on_application_shutdown: Option<Arc<ProviderApplicationHook>>,
}

impl ProviderLifecycleHooks {
    pub(super) fn has_hooks(&self) -> bool {
        self.on_module_init.is_some()
            || self.on_application_bootstrap.is_some()
            || self.on_application_shutdown.is_some()
    }

    pub(super) fn on_module_init(&self) -> Option<&Arc<ProviderModuleInitHook>> {
        self.on_module_init.as_ref()
    }

    pub(super) fn on_application_bootstrap(&self) -> Option<&Arc<ProviderApplicationHook>> {
        self.on_application_bootstrap.as_ref()
    }

    pub(super) fn on_application_shutdown(&self) -> Option<&Arc<ProviderApplicationHook>> {
        self.on_application_shutdown.as_ref()
    }
}

impl fmt::Debug for ProviderDefinition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProviderDefinition")
            .field("token", &self.token)
            .field("scope", &self.scope)
            .field("async", &self.factory.is_async())
            .field("alias_target", &self.alias_target)
            .finish_non_exhaustive()
    }
}

impl ProviderFactoryKind {
    fn is_async(&self) -> bool {
        matches!(self, Self::Async(_))
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
            factory: ProviderFactoryKind::Sync(Arc::new(move |_| {
                Ok(Arc::clone(&factory_value) as Arc<AnyProvider>)
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: None,
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
            factory: ProviderFactoryKind::Sync(Arc::new(move |module_ref| {
                Ok(Arc::new(factory(module_ref)?) as Arc<AnyProvider>)
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: None,
        }
    }

    pub fn named_factory_arc<T, F>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(&ModuleRef) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self {
            token: ProviderToken::named(token),
            factory: ProviderFactoryKind::Sync(Arc::new(move |module_ref| {
                Ok(factory(module_ref)? as Arc<AnyProvider>)
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: None,
        }
    }

    pub fn async_factory<T, F, Fut>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(ModuleRef) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        Self::named_async_factory(ProviderToken::of::<T>().as_str(), factory)
    }

    pub fn async_factory_arc<T, F, Fut>(factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(ModuleRef) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<T>>> + Send + 'static,
    {
        Self::named_async_factory_arc(ProviderToken::of::<T>().as_str(), factory)
    }

    pub fn named_async_factory<T, F, Fut>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(ModuleRef) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<T>> + Send + 'static,
    {
        Self {
            token: ProviderToken::named(token),
            factory: ProviderFactoryKind::Async(Arc::new(move |module_ref| {
                let future = factory(module_ref);
                Box::pin(async move { Ok(Arc::new(future.await?) as Arc<AnyProvider>) })
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: None,
        }
    }

    pub fn named_async_factory_arc<T, F, Fut>(token: impl Into<String>, factory: F) -> Self
    where
        T: Send + Sync + 'static,
        F: Fn(ModuleRef) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<Arc<T>>> + Send + 'static,
    {
        Self {
            token: ProviderToken::named(token),
            factory: ProviderFactoryKind::Async(Arc::new(move |module_ref| {
                let future = factory(module_ref);
                Box::pin(async move { Ok(future.await? as Arc<AnyProvider>) })
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: None,
        }
    }

    pub fn injectable<T>() -> Self
    where
        T: FromModuleRef,
    {
        Self::factory::<T, _>(T::from_module_ref)
    }

    pub fn named_injectable<T>(token: impl Into<String>) -> Self
    where
        T: FromModuleRef,
    {
        Self::named_factory::<T, _>(token, T::from_module_ref)
    }

    pub fn alias<T>(target: ProviderToken) -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::named_alias(ProviderToken::of::<T>().as_str(), target)
    }

    pub fn named_alias(token: impl Into<String>, target: ProviderToken) -> Self {
        Self {
            token: ProviderToken::named(token),
            factory: ProviderFactoryKind::Sync(Arc::new(|_| {
                Err(BootError::Internal(
                    "provider aliases must be resolved through ModuleRef".to_string(),
                ))
            })),
            scope: ProviderScope::Singleton,
            lifecycle: ProviderLifecycleHooks::default(),
            alias_target: Some(target),
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

    pub fn transient_injectable<T>() -> Self
    where
        T: FromModuleRef,
    {
        Self::injectable::<T>().with_scope(ProviderScope::Transient)
    }

    pub fn named_transient_injectable<T>(token: impl Into<String>) -> Self
    where
        T: FromModuleRef,
    {
        Self::named_injectable::<T>(token).with_scope(ProviderScope::Transient)
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

    pub fn request_scoped_injectable<T>() -> Self
    where
        T: FromModuleRef,
    {
        Self::injectable::<T>().with_scope(ProviderScope::Request)
    }

    pub fn named_request_scoped_injectable<T>(token: impl Into<String>) -> Self
    where
        T: FromModuleRef,
    {
        Self::named_injectable::<T>(token).with_scope(ProviderScope::Request)
    }

    pub fn with_scope(mut self, scope: ProviderScope) -> Self {
        self.scope = scope;
        self
    }

    pub fn with_on_module_init<T>(mut self) -> Self
    where
        T: ProviderOnModuleInit,
    {
        self.lifecycle.on_module_init = Some(Arc::new(|provider, module_ref| {
            let provider = downcast_lifecycle_provider::<T>(provider)?;
            provider.on_module_init(module_ref)
        }));
        self
    }

    pub fn with_on_application_bootstrap<T>(mut self) -> Self
    where
        T: ProviderOnApplicationBootstrap,
    {
        self.lifecycle.on_application_bootstrap =
            Some(Arc::new(
                |provider, module_ref| match downcast_lifecycle_provider::<T>(provider) {
                    Ok(provider) => provider.on_application_bootstrap(module_ref),
                    Err(error) => Box::pin(async move { Err(error) }),
                },
            ));
        self
    }

    pub fn with_on_application_shutdown<T>(mut self) -> Self
    where
        T: ProviderOnApplicationShutdown,
    {
        self.lifecycle.on_application_shutdown =
            Some(Arc::new(
                |provider, module_ref| match downcast_lifecycle_provider::<T>(provider) {
                    Ok(provider) => provider.on_application_shutdown(module_ref),
                    Err(error) => Box::pin(async move { Err(error) }),
                },
            ));
        self
    }

    pub fn token(&self) -> &ProviderToken {
        &self.token
    }

    pub fn scope(&self) -> ProviderScope {
        self.scope
    }

    pub(super) fn is_alias(&self) -> bool {
        self.alias_target.is_some()
    }

    pub(super) fn is_async_factory(&self) -> bool {
        self.factory.is_async()
    }

    pub(super) fn alias_target(&self) -> Option<&ProviderToken> {
        self.alias_target.as_ref()
    }

    pub(super) fn lifecycle(&self) -> &ProviderLifecycleHooks {
        &self.lifecycle
    }

    pub(super) fn build(&self, module_ref: &ModuleRef) -> Result<Arc<AnyProvider>> {
        match &self.factory {
            ProviderFactoryKind::Sync(factory) => factory(module_ref),
            ProviderFactoryKind::Async(_) => Err(BootError::Internal(format!(
                "async provider factory requires async application build: {}",
                self.token
            ))),
        }
    }

    pub(super) async fn build_async(&self, module_ref: ModuleRef) -> Result<Arc<AnyProvider>> {
        match &self.factory {
            ProviderFactoryKind::Sync(factory) => factory(&module_ref),
            ProviderFactoryKind::Async(factory) => factory(module_ref).await,
        }
    }
}

fn downcast_lifecycle_provider<T>(provider: Arc<AnyProvider>) -> Result<Arc<T>>
where
    T: Send + Sync + 'static,
{
    Arc::downcast::<T>(provider)
        .map_err(|_| BootError::ProviderTypeMismatch(std::any::type_name::<T>().to_string()))
}
