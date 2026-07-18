use super::{
    catch_errors, ExceptionFilter, ExecutionInterceptor, ExecutionInterceptorAdapter, Guard,
    Interceptor, Middleware, Pipe,
};
use crate::{
    BootErrorKind, ContextId, ModuleRef, ProviderToken, Result, TransportExceptionFilter,
    TransportGuard, TransportInterceptor, TransportPipe, ValidationOptions,
    WebSocketExceptionFilter, WebSocketGuard, WebSocketInterceptor, WebSocketPipe,
};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct PipelineComponent<T: ?Sized> {
    type_id: TypeId,
    source: PipelineComponentSource<T>,
}

type PipelineProviderResolver<T> = dyn Fn(&ModuleRef, &ContextId) -> Result<Arc<T>> + Send + Sync;

enum PipelineComponentSource<T: ?Sized> {
    Static(Arc<T>),
    Provider {
        owner: ModuleRef,
        resolver: Arc<PipelineProviderResolver<T>>,
    },
}

impl<T: ?Sized> Clone for PipelineComponent<T> {
    fn clone(&self) -> Self {
        Self {
            type_id: self.type_id,
            source: match &self.source {
                PipelineComponentSource::Static(inner) => {
                    PipelineComponentSource::Static(Arc::clone(inner))
                }
                PipelineComponentSource::Provider { owner, resolver } => {
                    PipelineComponentSource::Provider {
                        owner: owner.clone(),
                        resolver: Arc::clone(resolver),
                    }
                }
            },
        }
    }
}

impl<T: ?Sized> PipelineComponent<T> {
    fn from_static(type_id: TypeId, inner: Arc<T>) -> Self {
        Self {
            type_id,
            source: PipelineComponentSource::Static(inner),
        }
    }

    pub(crate) fn from_provider<R>(type_id: TypeId, owner: ModuleRef, resolver: R) -> Self
    where
        R: Fn(&ModuleRef, &ContextId) -> Result<Arc<T>> + Send + Sync + 'static,
    {
        Self {
            type_id,
            source: PipelineComponentSource::Provider {
                owner,
                resolver: Arc::new(resolver),
            },
        }
    }

    pub(crate) fn resolve(&self, context_id: &ContextId) -> Result<Arc<T>> {
        match &self.source {
            PipelineComponentSource::Static(inner) => Ok(Arc::clone(inner)),
            PipelineComponentSource::Provider { owner, resolver } => resolver(owner, context_id),
        }
    }

    fn type_id(&self) -> TypeId {
        self.type_id
    }
}

type ProviderEnhancerBinder = dyn Fn(ModuleRef) -> BoundProviderEnhancer + Send + Sync;

#[derive(Clone)]
pub(crate) struct ProviderEnhancerMarker {
    binder: Arc<ProviderEnhancerBinder>,
}

#[derive(Clone)]
pub(crate) enum BoundProviderEnhancer {
    Pipe(PipelineComponent<dyn Pipe>),
    Guard(PipelineComponent<dyn Guard>),
    Interceptor(PipelineComponent<dyn Interceptor>),
    Filter(PipelineComponent<dyn ExceptionFilter>),
    WebSocketPipe(PipelineComponent<dyn WebSocketPipe>),
    WebSocketGuard(PipelineComponent<dyn WebSocketGuard>),
    WebSocketInterceptor(PipelineComponent<dyn WebSocketInterceptor>),
    WebSocketFilter(PipelineComponent<dyn WebSocketExceptionFilter>),
    TransportPipe(PipelineComponent<dyn TransportPipe>),
    TransportGuard(PipelineComponent<dyn TransportGuard>),
    TransportInterceptor(PipelineComponent<dyn TransportInterceptor>),
    TransportFilter(PipelineComponent<dyn TransportExceptionFilter>),
}

macro_rules! provider_enhancer_marker {
    ($method:ident, $variant:ident, $provider_trait:path, $trait_object:ty) => {
        pub(crate) fn $method<T>(token: ProviderToken) -> Self
        where
            T: $provider_trait,
        {
            Self {
                binder: Arc::new(move |owner| {
                    let token = token.clone();
                    BoundProviderEnhancer::$variant(PipelineComponent::from_provider(
                        TypeId::of::<T>(),
                        owner,
                        move |module_ref, context_id| {
                            module_ref
                                .resolve_token_with_context::<T>(&token, context_id)
                                .map(|provider| provider as Arc<$trait_object>)
                        },
                    ))
                }),
            }
        }
    };
}

impl ProviderEnhancerMarker {
    provider_enhancer_marker!(pipe, Pipe, Pipe, dyn Pipe);
    provider_enhancer_marker!(guard, Guard, Guard, dyn Guard);
    provider_enhancer_marker!(interceptor, Interceptor, Interceptor, dyn Interceptor);
    provider_enhancer_marker!(filter, Filter, ExceptionFilter, dyn ExceptionFilter);
    provider_enhancer_marker!(
        websocket_pipe,
        WebSocketPipe,
        WebSocketPipe,
        dyn WebSocketPipe
    );
    provider_enhancer_marker!(
        websocket_guard,
        WebSocketGuard,
        WebSocketGuard,
        dyn WebSocketGuard
    );
    provider_enhancer_marker!(
        websocket_interceptor,
        WebSocketInterceptor,
        WebSocketInterceptor,
        dyn WebSocketInterceptor
    );
    provider_enhancer_marker!(
        websocket_filter,
        WebSocketFilter,
        WebSocketExceptionFilter,
        dyn WebSocketExceptionFilter
    );
    provider_enhancer_marker!(
        transport_pipe,
        TransportPipe,
        TransportPipe,
        dyn TransportPipe
    );
    provider_enhancer_marker!(
        transport_guard,
        TransportGuard,
        TransportGuard,
        dyn TransportGuard
    );
    provider_enhancer_marker!(
        transport_interceptor,
        TransportInterceptor,
        TransportInterceptor,
        dyn TransportInterceptor
    );
    provider_enhancer_marker!(
        transport_filter,
        TransportFilter,
        TransportExceptionFilter,
        dyn TransportExceptionFilter
    );

    pub(crate) fn bind(&self, owner: ModuleRef) -> BoundProviderEnhancer {
        (self.binder)(owner)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ProviderEnhancerComponents {
    pub http: PipelineComponents,
    pub websocket_pipes: Vec<PipelineComponent<dyn WebSocketPipe>>,
    pub websocket_guards: Vec<PipelineComponent<dyn WebSocketGuard>>,
    pub websocket_interceptors: Vec<PipelineComponent<dyn WebSocketInterceptor>>,
    pub websocket_filters: Vec<PipelineComponent<dyn WebSocketExceptionFilter>>,
    pub transport_pipes: Vec<PipelineComponent<dyn TransportPipe>>,
    pub transport_guards: Vec<PipelineComponent<dyn TransportGuard>>,
    pub transport_interceptors: Vec<PipelineComponent<dyn TransportInterceptor>>,
    pub transport_filters: Vec<PipelineComponent<dyn TransportExceptionFilter>>,
}

impl ProviderEnhancerComponents {
    pub(crate) fn push(&mut self, enhancer: BoundProviderEnhancer) {
        match enhancer {
            BoundProviderEnhancer::Pipe(component) => self.http.pipes.push(component),
            BoundProviderEnhancer::Guard(component) => self.http.guards.push(component),
            BoundProviderEnhancer::Interceptor(component) => {
                self.http.interceptors.push(component);
            }
            BoundProviderEnhancer::Filter(component) => self.http.filters.push(component),
            BoundProviderEnhancer::WebSocketPipe(component) => {
                self.websocket_pipes.push(component);
            }
            BoundProviderEnhancer::WebSocketGuard(component) => {
                self.websocket_guards.push(component);
            }
            BoundProviderEnhancer::WebSocketInterceptor(component) => {
                self.websocket_interceptors.push(component);
            }
            BoundProviderEnhancer::WebSocketFilter(component) => {
                self.websocket_filters.push(component);
            }
            BoundProviderEnhancer::TransportPipe(component) => {
                self.transport_pipes.push(component);
            }
            BoundProviderEnhancer::TransportGuard(component) => {
                self.transport_guards.push(component);
            }
            BoundProviderEnhancer::TransportInterceptor(component) => {
                self.transport_interceptors.push(component);
            }
            BoundProviderEnhancer::TransportFilter(component) => {
                self.transport_filters.push(component);
            }
        }
    }
}

impl PipelineComponent<dyn Pipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: Pipe,
    {
        Self::from_static(TypeId::of::<P>(), Arc::new(pipe))
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: Pipe,
        P: Pipe,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(pipe))
    }
}

impl PipelineComponent<dyn Guard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: Guard,
    {
        Self::from_static(TypeId::of::<G>(), Arc::new(guard))
    }

    pub(crate) fn from_arc(guard: Arc<dyn Guard>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn Guard>>(), guard)
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: Guard,
        G: Guard,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(guard))
    }
}

impl PipelineComponent<dyn Interceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: Interceptor,
    {
        Self::from_static(TypeId::of::<I>(), Arc::new(interceptor))
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: Interceptor,
        I: Interceptor,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(interceptor))
    }
}

impl PipelineComponent<dyn ExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        Self::from_static(TypeId::of::<F>(), Arc::new(filter))
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: ExceptionFilter,
        F: ExceptionFilter,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(filter))
    }
}

impl PipelineComponent<dyn WebSocketPipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: WebSocketPipe,
    {
        Self::from_static(TypeId::of::<P>(), Arc::new(pipe))
    }

    pub(crate) fn from_arc(pipe: Arc<dyn WebSocketPipe>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn WebSocketPipe>>(), pipe)
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: WebSocketPipe,
        P: WebSocketPipe,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(pipe))
    }
}

impl PipelineComponent<dyn WebSocketGuard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: WebSocketGuard,
    {
        Self::from_static(TypeId::of::<G>(), Arc::new(guard))
    }

    pub(crate) fn from_arc(guard: Arc<dyn WebSocketGuard>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn WebSocketGuard>>(), guard)
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: WebSocketGuard,
        G: WebSocketGuard,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(guard))
    }
}

impl PipelineComponent<dyn WebSocketInterceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: WebSocketInterceptor,
    {
        Self::from_static(TypeId::of::<I>(), Arc::new(interceptor))
    }

    pub(crate) fn from_arc(interceptor: Arc<dyn WebSocketInterceptor>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn WebSocketInterceptor>>(), interceptor)
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: WebSocketInterceptor,
        I: WebSocketInterceptor,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(interceptor))
    }
}

impl PipelineComponent<dyn WebSocketExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: WebSocketExceptionFilter,
    {
        Self::from_static(TypeId::of::<F>(), Arc::new(filter))
    }

    pub(crate) fn from_arc(filter: Arc<dyn WebSocketExceptionFilter>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn WebSocketExceptionFilter>>(), filter)
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: WebSocketExceptionFilter,
        F: WebSocketExceptionFilter,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(filter))
    }
}

impl PipelineComponent<dyn TransportPipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: TransportPipe,
    {
        Self::from_static(TypeId::of::<P>(), Arc::new(pipe))
    }

    pub(crate) fn from_arc(pipe: Arc<dyn TransportPipe>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn TransportPipe>>(), pipe)
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: TransportPipe,
        P: TransportPipe,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(pipe))
    }
}

impl PipelineComponent<dyn TransportGuard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: TransportGuard,
    {
        Self::from_static(TypeId::of::<G>(), Arc::new(guard))
    }

    pub(crate) fn from_arc(guard: Arc<dyn TransportGuard>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn TransportGuard>>(), guard)
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: TransportGuard,
        G: TransportGuard,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(guard))
    }
}

impl PipelineComponent<dyn TransportInterceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        Self::from_static(TypeId::of::<I>(), Arc::new(interceptor))
    }

    pub(crate) fn from_arc(interceptor: Arc<dyn TransportInterceptor>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn TransportInterceptor>>(), interceptor)
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: TransportInterceptor,
        I: TransportInterceptor,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(interceptor))
    }
}

impl PipelineComponent<dyn TransportExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: TransportExceptionFilter,
    {
        Self::from_static(TypeId::of::<F>(), Arc::new(filter))
    }

    pub(crate) fn from_arc(filter: Arc<dyn TransportExceptionFilter>) -> Self {
        Self::from_static(TypeId::of::<Arc<dyn TransportExceptionFilter>>(), filter)
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: TransportExceptionFilter,
        F: TransportExceptionFilter,
    {
        Self::from_static(TypeId::of::<T>(), Arc::new(filter))
    }
}

#[derive(Clone, Default)]
pub(crate) struct PipelineComponents {
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub pipes: Vec<PipelineComponent<dyn Pipe>>,
    pub guards: Vec<PipelineComponent<dyn Guard>>,
    pub interceptors: Vec<PipelineComponent<dyn Interceptor>>,
    pub filters: Vec<PipelineComponent<dyn ExceptionFilter>>,
    pub validation_enabled: bool,
    pub validation_options: ValidationOptions,
}

impl PipelineComponents {
    pub(crate) fn append(&mut self, components: &Self) {
        self.middleware
            .extend(components.middleware.iter().cloned());
        self.pipes.extend(components.pipes.iter().cloned());
        self.guards.extend(components.guards.iter().cloned());
        self.interceptors
            .extend(components.interceptors.iter().cloned());
        self.filters.extend(components.filters.iter().cloned());
        self.validation_enabled = self.validation_enabled || components.validation_enabled;
        self.validation_options = self.validation_options.merge(components.validation_options);
    }

    pub fn push_middleware<M>(&mut self, middleware: M)
    where
        M: Middleware,
    {
        self.middleware.push(Arc::new(middleware));
    }

    pub fn push_middleware_arc(&mut self, middleware: Arc<dyn Middleware>) {
        self.middleware.push(middleware);
    }

    pub fn push_pipe<P>(&mut self, pipe: P)
    where
        P: Pipe,
    {
        self.pipes.push(PipelineComponent::<dyn Pipe>::new(pipe));
    }

    pub fn push_guard<G>(&mut self, guard: G)
    where
        G: Guard,
    {
        self.guards.push(PipelineComponent::<dyn Guard>::new(guard));
    }

    pub fn push_guard_arc(&mut self, guard: Arc<dyn Guard>) {
        self.guards
            .push(PipelineComponent::<dyn Guard>::from_arc(guard));
    }

    pub fn push_interceptor<I>(&mut self, interceptor: I)
    where
        I: Interceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn Interceptor>::new(interceptor));
    }

    pub fn push_execution_interceptor_arc(&mut self, interceptor: Arc<dyn ExecutionInterceptor>) {
        self.push_interceptor(ExecutionInterceptorAdapter::new(interceptor));
    }

    pub fn push_filter<F>(&mut self, filter: F)
    where
        F: ExceptionFilter,
    {
        self.filters
            .push(PipelineComponent::<dyn ExceptionFilter>::new(filter));
    }

    pub fn push_catch_filter<I, F>(&mut self, kinds: I, filter: F)
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: ExceptionFilter,
    {
        self.push_filter(catch_errors(kinds, filter));
    }

    pub fn enable_validation(&mut self) {
        self.validation_enabled = true;
    }

    pub fn enable_validation_with_options(&mut self, options: ValidationOptions) {
        self.validation_enabled = true;
        self.validation_options = self.validation_options.merge(options);
    }
}

#[derive(Default)]
pub(crate) struct PipelineOverrides {
    pipes: HashMap<TypeId, PipelineComponent<dyn Pipe>>,
    guards: HashMap<TypeId, PipelineComponent<dyn Guard>>,
    interceptors: HashMap<TypeId, PipelineComponent<dyn Interceptor>>,
    filters: HashMap<TypeId, PipelineComponent<dyn ExceptionFilter>>,
    websocket_pipes: HashMap<TypeId, PipelineComponent<dyn WebSocketPipe>>,
    websocket_guards: HashMap<TypeId, PipelineComponent<dyn WebSocketGuard>>,
    websocket_interceptors: HashMap<TypeId, PipelineComponent<dyn WebSocketInterceptor>>,
    websocket_filters: HashMap<TypeId, PipelineComponent<dyn WebSocketExceptionFilter>>,
    transport_pipes: HashMap<TypeId, PipelineComponent<dyn TransportPipe>>,
    transport_guards: HashMap<TypeId, PipelineComponent<dyn TransportGuard>>,
    transport_interceptors: HashMap<TypeId, PipelineComponent<dyn TransportInterceptor>>,
    transport_filters: HashMap<TypeId, PipelineComponent<dyn TransportExceptionFilter>>,
}

impl PipelineOverrides {
    pub(crate) fn is_empty(&self) -> bool {
        self.pipes.is_empty()
            && self.guards.is_empty()
            && self.interceptors.is_empty()
            && self.filters.is_empty()
            && self.websocket_pipes.is_empty()
            && self.websocket_guards.is_empty()
            && self.websocket_interceptors.is_empty()
            && self.websocket_filters.is_empty()
            && self.transport_pipes.is_empty()
            && self.transport_guards.is_empty()
            && self.transport_interceptors.is_empty()
            && self.transport_filters.is_empty()
    }

    pub(crate) fn override_pipe<T, P>(&mut self, pipe: P)
    where
        T: Pipe,
        P: Pipe,
    {
        self.pipes.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn Pipe>::replacement::<T, P>(pipe),
        );
    }

    pub(crate) fn override_guard<T, G>(&mut self, guard: G)
    where
        T: Guard,
        G: Guard,
    {
        self.guards.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn Guard>::replacement::<T, G>(guard),
        );
    }

    pub(crate) fn override_interceptor<T, I>(&mut self, interceptor: I)
    where
        T: Interceptor,
        I: Interceptor,
    {
        self.interceptors.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn Interceptor>::replacement::<T, I>(interceptor),
        );
    }

    pub(crate) fn override_filter<T, F>(&mut self, filter: F)
    where
        T: ExceptionFilter,
        F: ExceptionFilter,
    {
        self.filters.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn ExceptionFilter>::replacement::<T, F>(filter),
        );
    }

    pub(crate) fn apply_to_pipes(&self, pipes: &mut [PipelineComponent<dyn Pipe>]) {
        apply(&self.pipes, pipes);
    }

    pub(crate) fn apply_to_guards(&self, guards: &mut [PipelineComponent<dyn Guard>]) {
        apply(&self.guards, guards);
    }

    pub(crate) fn apply_to_interceptors(
        &self,
        interceptors: &mut [PipelineComponent<dyn Interceptor>],
    ) {
        apply(&self.interceptors, interceptors);
    }

    pub(crate) fn apply_to_filters(&self, filters: &mut [PipelineComponent<dyn ExceptionFilter>]) {
        apply(&self.filters, filters);
    }

    pub(crate) fn override_websocket_pipe<T, P>(&mut self, pipe: P)
    where
        T: WebSocketPipe,
        P: WebSocketPipe,
    {
        self.websocket_pipes.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn WebSocketPipe>::replacement::<T, P>(pipe),
        );
    }

    pub(crate) fn override_websocket_guard<T, G>(&mut self, guard: G)
    where
        T: WebSocketGuard,
        G: WebSocketGuard,
    {
        self.websocket_guards.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn WebSocketGuard>::replacement::<T, G>(guard),
        );
    }

    pub(crate) fn override_websocket_interceptor<T, I>(&mut self, interceptor: I)
    where
        T: WebSocketInterceptor,
        I: WebSocketInterceptor,
    {
        self.websocket_interceptors.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn WebSocketInterceptor>::replacement::<T, I>(interceptor),
        );
    }

    pub(crate) fn override_websocket_filter<T, F>(&mut self, filter: F)
    where
        T: WebSocketExceptionFilter,
        F: WebSocketExceptionFilter,
    {
        self.websocket_filters.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn WebSocketExceptionFilter>::replacement::<T, F>(filter),
        );
    }

    pub(crate) fn apply_to_websocket_pipes(
        &self,
        pipes: &mut [PipelineComponent<dyn WebSocketPipe>],
    ) {
        apply(&self.websocket_pipes, pipes);
    }

    pub(crate) fn apply_to_websocket_guards(
        &self,
        guards: &mut [PipelineComponent<dyn WebSocketGuard>],
    ) {
        apply(&self.websocket_guards, guards);
    }

    pub(crate) fn apply_to_websocket_interceptors(
        &self,
        interceptors: &mut [PipelineComponent<dyn WebSocketInterceptor>],
    ) {
        apply(&self.websocket_interceptors, interceptors);
    }

    pub(crate) fn apply_to_websocket_filters(
        &self,
        filters: &mut [PipelineComponent<dyn WebSocketExceptionFilter>],
    ) {
        apply(&self.websocket_filters, filters);
    }

    pub(crate) fn override_transport_pipe<T, P>(&mut self, pipe: P)
    where
        T: TransportPipe,
        P: TransportPipe,
    {
        self.transport_pipes.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn TransportPipe>::replacement::<T, P>(pipe),
        );
    }

    pub(crate) fn override_transport_guard<T, G>(&mut self, guard: G)
    where
        T: TransportGuard,
        G: TransportGuard,
    {
        self.transport_guards.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn TransportGuard>::replacement::<T, G>(guard),
        );
    }

    pub(crate) fn override_transport_interceptor<T, I>(&mut self, interceptor: I)
    where
        T: TransportInterceptor,
        I: TransportInterceptor,
    {
        self.transport_interceptors.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn TransportInterceptor>::replacement::<T, I>(interceptor),
        );
    }

    pub(crate) fn override_transport_filter<T, F>(&mut self, filter: F)
    where
        T: TransportExceptionFilter,
        F: TransportExceptionFilter,
    {
        self.transport_filters.insert(
            TypeId::of::<T>(),
            PipelineComponent::<dyn TransportExceptionFilter>::replacement::<T, F>(filter),
        );
    }

    pub(crate) fn apply_to_transport_pipes(
        &self,
        pipes: &mut [PipelineComponent<dyn TransportPipe>],
    ) {
        apply(&self.transport_pipes, pipes);
    }

    pub(crate) fn apply_to_transport_guards(
        &self,
        guards: &mut [PipelineComponent<dyn TransportGuard>],
    ) {
        apply(&self.transport_guards, guards);
    }

    pub(crate) fn apply_to_transport_interceptors(
        &self,
        interceptors: &mut [PipelineComponent<dyn TransportInterceptor>],
    ) {
        apply(&self.transport_interceptors, interceptors);
    }

    pub(crate) fn apply_to_transport_filters(
        &self,
        filters: &mut [PipelineComponent<dyn TransportExceptionFilter>],
    ) {
        apply(&self.transport_filters, filters);
    }
}

fn apply<T: ?Sized>(
    overrides: &HashMap<TypeId, PipelineComponent<T>>,
    components: &mut [PipelineComponent<T>],
) {
    for component in components {
        if let Some(replacement) = overrides.get(&component.type_id()) {
            *component = replacement.clone();
        }
    }
}
