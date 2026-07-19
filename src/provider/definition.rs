use super::{AnyProvider, ModuleRef, ProviderToken};
use crate::pipeline::ProviderEnhancerMarker;
use crate::{
    BootError, BoxFuture, ExceptionFilter, Guard, Interceptor, Pipe, Result,
    TransportExceptionFilter, TransportGuard, TransportInterceptor, TransportPipe,
    WebSocketExceptionFilter, WebSocketGuard, WebSocketInterceptor, WebSocketPipe,
};
use std::fmt;
use std::future::Future;
use std::sync::Arc;

type ProviderFactory = dyn Fn(&ModuleRef) -> Result<Arc<AnyProvider>> + Send + Sync;
type AsyncProviderFactory =
    dyn Fn(ModuleRef) -> BoxFuture<'static, Result<Arc<AnyProvider>>> + Send + Sync;
type ProviderModuleInitHook = dyn Fn(Arc<AnyProvider>, &ModuleRef) -> Result<()> + Send + Sync;
type ProviderApplicationHook =
    dyn Fn(Arc<AnyProvider>, ModuleRef) -> BoxFuture<'static, Result<()>> + Send + Sync;
type ProviderShutdownHook = dyn Fn(Arc<AnyProvider>, ModuleRef, Option<String>) -> BoxFuture<'static, Result<()>>
    + Send
    + Sync;

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

/// Lifecycle hook for singleton providers that need async teardown before shutdown starts.
pub trait ProviderOnModuleDestroy: Send + Sync + 'static {
    fn on_module_destroy(
        &self,
        _module_ref: ModuleRef,
        _signal: Option<String>,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Lifecycle hook for singleton providers that need async work before listeners close.
pub trait ProviderBeforeApplicationShutdown: Send + Sync + 'static {
    fn before_application_shutdown(
        &self,
        _module_ref: ModuleRef,
        _signal: Option<String>,
    ) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

/// Lifecycle hook for singleton providers that need async shutdown cleanup.
pub trait ProviderOnApplicationShutdown: Send + Sync + 'static {
    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }

    fn on_application_shutdown_with_signal(
        &self,
        module_ref: ModuleRef,
        _signal: Option<String>,
    ) -> BoxFuture<'static, Result<()>> {
        self.on_application_shutdown(module_ref)
    }
}

/// Builds an injectable value from the module provider graph.
pub trait FromModuleRef: Sized + Send + Sync + 'static {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self>;

    /// Dependencies captured while constructing this provider.
    ///
    /// `#[injectable]` implements this automatically. Manual implementations
    /// can return `None` when the dependency graph is opaque.
    fn provider_dependencies() -> Option<Vec<ProviderDependency>> {
        None
    }
}

/// Lifetime strategy for provider resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderScope {
    Singleton,
    Request,
    Transient,
}

/// One provider dependency used to calculate contextual scope propagation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDependency {
    token: ProviderToken,
    optional: bool,
    lazy: bool,
}

impl ProviderDependency {
    /// Declare a required typed dependency.
    pub fn typed<T>() -> Self
    where
        T: Send + Sync + 'static,
    {
        Self::named(ProviderToken::of::<T>().as_str())
    }

    /// Declare a required named dependency.
    pub fn named(token: impl Into<String>) -> Self {
        Self {
            token: ProviderToken::named(token),
            optional: false,
            lazy: false,
        }
    }

    /// Mark this dependency as optional when its token is not visible.
    pub fn optional(mut self) -> Self {
        self.optional = true;
        self
    }

    /// Mark this as a lazy handle that does not participate in scope bubbling.
    pub fn lazy(mut self) -> Self {
        self.lazy = true;
        self
    }

    pub fn token(&self) -> &ProviderToken {
        &self.token
    }

    pub fn is_optional(&self) -> bool {
        self.optional
    }

    pub fn is_lazy(&self) -> bool {
        self.lazy
    }
}

/// A provider registration, similar to a Nest provider entry.
#[derive(Clone)]
pub struct ProviderDefinition {
    token: ProviderToken,
    factory: ProviderFactoryKind,
    scope: ProviderScope,
    lifecycle: ProviderLifecycleHooks,
    alias_target: Option<ProviderToken>,
    dependencies: Option<Arc<[ProviderDependency]>>,
    enhancers: Arc<[ProviderEnhancerMarker]>,
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
    on_module_destroy: Option<Arc<ProviderShutdownHook>>,
    before_application_shutdown: Option<Arc<ProviderShutdownHook>>,
    on_application_shutdown: Option<Arc<ProviderShutdownHook>>,
}

impl ProviderLifecycleHooks {
    pub(super) fn has_hooks(&self) -> bool {
        self.on_module_init.is_some()
            || self.on_application_bootstrap.is_some()
            || self.on_module_destroy.is_some()
            || self.before_application_shutdown.is_some()
            || self.on_application_shutdown.is_some()
    }

    pub(super) fn on_module_init(&self) -> Option<&Arc<ProviderModuleInitHook>> {
        self.on_module_init.as_ref()
    }

    pub(super) fn on_application_bootstrap(&self) -> Option<&Arc<ProviderApplicationHook>> {
        self.on_application_bootstrap.as_ref()
    }

    pub(super) fn on_module_destroy(&self) -> Option<&Arc<ProviderShutdownHook>> {
        self.on_module_destroy.as_ref()
    }

    pub(super) fn before_application_shutdown(&self) -> Option<&Arc<ProviderShutdownHook>> {
        self.before_application_shutdown.as_ref()
    }

    pub(super) fn on_application_shutdown(&self) -> Option<&Arc<ProviderShutdownHook>> {
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
            .field("enhancers", &self.enhancers.len())
            .field(
                "dependencies",
                &self
                    .dependencies
                    .as_ref()
                    .map(|dependencies| dependencies.len()),
            )
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
            dependencies: Some(Arc::from([])),
            enhancers: Arc::from([]),
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
            dependencies: None,
            enhancers: Arc::from([]),
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
            dependencies: None,
            enhancers: Arc::from([]),
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
            dependencies: None,
            enhancers: Arc::from([]),
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
            dependencies: None,
            enhancers: Arc::from([]),
        }
    }

    pub fn injectable<T>() -> Self
    where
        T: FromModuleRef,
    {
        let definition = Self::factory::<T, _>(T::from_module_ref);
        match T::provider_dependencies() {
            Some(dependencies) => definition.with_dependencies(dependencies),
            None => definition,
        }
    }

    pub fn named_injectable<T>(token: impl Into<String>) -> Self
    where
        T: FromModuleRef,
    {
        let definition = Self::named_factory::<T, _>(token, T::from_module_ref);
        match T::provider_dependencies() {
            Some(dependencies) => definition.with_dependencies(dependencies),
            None => definition,
        }
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
            alias_target: Some(target.clone()),
            dependencies: Some(Arc::from([ProviderDependency {
                token: target,
                optional: false,
                lazy: false,
            }])),
            enhancers: Arc::from([]),
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

    /// Register an injectable provider as an application-wide HTTP guard.
    pub fn app_guard<T>() -> Self
    where
        T: FromModuleRef + Guard,
    {
        Self::injectable::<T>().with_app_guard::<T>()
    }

    /// Register an injectable provider as an application-wide HTTP pipe.
    pub fn app_pipe<T>() -> Self
    where
        T: FromModuleRef + Pipe,
    {
        Self::injectable::<T>().with_app_pipe::<T>()
    }

    /// Register an injectable provider as an application-wide HTTP interceptor.
    pub fn app_interceptor<T>() -> Self
    where
        T: FromModuleRef + Interceptor,
    {
        Self::injectable::<T>().with_app_interceptor::<T>()
    }

    /// Register an injectable provider as an application-wide HTTP exception filter.
    pub fn app_filter<T>() -> Self
    where
        T: FromModuleRef + ExceptionFilter,
    {
        Self::injectable::<T>().with_app_filter::<T>()
    }

    /// Register an injectable provider as an application-wide WebSocket guard.
    pub fn app_websocket_guard<T>() -> Self
    where
        T: FromModuleRef + WebSocketGuard,
    {
        Self::injectable::<T>().with_app_websocket_guard::<T>()
    }

    /// Register an injectable provider as an application-wide WebSocket pipe.
    pub fn app_websocket_pipe<T>() -> Self
    where
        T: FromModuleRef + WebSocketPipe,
    {
        Self::injectable::<T>().with_app_websocket_pipe::<T>()
    }

    /// Register an injectable provider as an application-wide WebSocket interceptor.
    pub fn app_websocket_interceptor<T>() -> Self
    where
        T: FromModuleRef + WebSocketInterceptor,
    {
        Self::injectable::<T>().with_app_websocket_interceptor::<T>()
    }

    /// Register an injectable provider as an application-wide WebSocket exception filter.
    pub fn app_websocket_filter<T>() -> Self
    where
        T: FromModuleRef + WebSocketExceptionFilter,
    {
        Self::injectable::<T>().with_app_websocket_filter::<T>()
    }

    /// Register an injectable provider as an application-wide transport guard.
    pub fn app_transport_guard<T>() -> Self
    where
        T: FromModuleRef + TransportGuard,
    {
        Self::injectable::<T>().with_app_transport_guard::<T>()
    }

    /// Register an injectable provider as an application-wide transport pipe.
    pub fn app_transport_pipe<T>() -> Self
    where
        T: FromModuleRef + TransportPipe,
    {
        Self::injectable::<T>().with_app_transport_pipe::<T>()
    }

    /// Register an injectable provider as an application-wide transport interceptor.
    pub fn app_transport_interceptor<T>() -> Self
    where
        T: FromModuleRef + TransportInterceptor,
    {
        Self::injectable::<T>().with_app_transport_interceptor::<T>()
    }

    /// Register an injectable provider as an application-wide transport exception filter.
    pub fn app_transport_filter<T>() -> Self
    where
        T: FromModuleRef + TransportExceptionFilter,
    {
        Self::injectable::<T>().with_app_transport_filter::<T>()
    }

    /// Mark this provider as an application-wide HTTP guard.
    pub fn with_app_guard<T>(self) -> Self
    where
        T: Guard,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::guard::<T>(token))
    }

    /// Mark this provider as an application-wide HTTP pipe.
    pub fn with_app_pipe<T>(self) -> Self
    where
        T: Pipe,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::pipe::<T>(token))
    }

    /// Mark this provider as an application-wide HTTP interceptor.
    pub fn with_app_interceptor<T>(self) -> Self
    where
        T: Interceptor,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::interceptor::<T>(token))
    }

    /// Mark this provider as an application-wide HTTP exception filter.
    pub fn with_app_filter<T>(self) -> Self
    where
        T: ExceptionFilter,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::filter::<T>(token))
    }

    /// Mark this provider as an application-wide WebSocket guard.
    pub fn with_app_websocket_guard<T>(self) -> Self
    where
        T: WebSocketGuard,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::websocket_guard::<T>(token))
    }

    /// Mark this provider as an application-wide WebSocket pipe.
    pub fn with_app_websocket_pipe<T>(self) -> Self
    where
        T: WebSocketPipe,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::websocket_pipe::<T>(token))
    }

    /// Mark this provider as an application-wide WebSocket interceptor.
    pub fn with_app_websocket_interceptor<T>(self) -> Self
    where
        T: WebSocketInterceptor,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::websocket_interceptor::<T>(token))
    }

    /// Mark this provider as an application-wide WebSocket exception filter.
    pub fn with_app_websocket_filter<T>(self) -> Self
    where
        T: WebSocketExceptionFilter,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::websocket_filter::<T>(token))
    }

    /// Mark this provider as an application-wide transport guard.
    pub fn with_app_transport_guard<T>(self) -> Self
    where
        T: TransportGuard,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::transport_guard::<T>(token))
    }

    /// Mark this provider as an application-wide transport pipe.
    pub fn with_app_transport_pipe<T>(self) -> Self
    where
        T: TransportPipe,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::transport_pipe::<T>(token))
    }

    /// Mark this provider as an application-wide transport interceptor.
    pub fn with_app_transport_interceptor<T>(self) -> Self
    where
        T: TransportInterceptor,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::transport_interceptor::<T>(token))
    }

    /// Mark this provider as an application-wide transport exception filter.
    pub fn with_app_transport_filter<T>(self) -> Self
    where
        T: TransportExceptionFilter,
    {
        let token = self.token.clone();
        self.with_enhancer(ProviderEnhancerMarker::transport_filter::<T>(token))
    }

    pub fn with_scope(mut self, scope: ProviderScope) -> Self {
        self.scope = scope;
        self
    }

    fn with_enhancer(mut self, enhancer: ProviderEnhancerMarker) -> Self {
        let mut enhancers = self.enhancers.to_vec();
        enhancers.push(enhancer);
        self.enhancers = enhancers.into();
        self
    }

    /// Declare the dependencies captured by this provider factory.
    pub fn with_dependencies<I>(mut self, dependencies: I) -> Self
    where
        I: IntoIterator<Item = ProviderDependency>,
    {
        self.dependencies = Some(dependencies.into_iter().collect::<Vec<_>>().into());
        self
    }

    /// Add one required typed dependency to this provider factory.
    pub fn depends_on<T>(self) -> Self
    where
        T: Send + Sync + 'static,
    {
        self.with_dependency(ProviderDependency::typed::<T>())
    }

    /// Add one required named dependency to this provider factory.
    pub fn depends_on_named(self, token: impl Into<String>) -> Self {
        self.with_dependency(ProviderDependency::named(token))
    }

    /// Add one dependency while retaining previously declared metadata.
    pub fn with_dependency(mut self, dependency: ProviderDependency) -> Self {
        let mut dependencies = self
            .dependencies
            .as_deref()
            .map(<[ProviderDependency]>::to_vec)
            .unwrap_or_default();
        dependencies.push(dependency);
        self.dependencies = Some(dependencies.into());
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

    pub fn with_on_module_destroy<T>(mut self) -> Self
    where
        T: ProviderOnModuleDestroy,
    {
        self.lifecycle.on_module_destroy =
            Some(Arc::new(
                |provider, module_ref, signal| match downcast_lifecycle_provider::<T>(provider) {
                    Ok(provider) => provider.on_module_destroy(module_ref, signal),
                    Err(error) => Box::pin(async move { Err(error) }),
                },
            ));
        self
    }

    pub fn with_before_application_shutdown<T>(mut self) -> Self
    where
        T: ProviderBeforeApplicationShutdown,
    {
        self.lifecycle.before_application_shutdown = Some(Arc::new(
            |provider, module_ref, signal| match downcast_lifecycle_provider::<T>(provider) {
                Ok(provider) => provider.before_application_shutdown(module_ref, signal),
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
                |provider, module_ref, signal| match downcast_lifecycle_provider::<T>(provider) {
                    Ok(provider) => {
                        provider.on_application_shutdown_with_signal(module_ref, signal)
                    }
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

    /// Declared dependency metadata, or `None` for an opaque factory.
    pub fn dependencies(&self) -> Option<&[ProviderDependency]> {
        self.dependencies.as_deref()
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

    pub(crate) fn enhancer_markers(&self) -> &[ProviderEnhancerMarker] {
        &self.enhancers
    }

    pub(crate) fn with_enhancers_from(mut self, source: &Self) -> Self {
        self.enhancers = Arc::clone(&source.enhancers);
        self
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
