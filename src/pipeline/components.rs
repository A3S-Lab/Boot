use super::{
    catch_errors, ExceptionFilter, ExecutionInterceptor, ExecutionInterceptorAdapter, Guard,
    Interceptor, Middleware, Pipe,
};
use crate::{
    BootErrorKind, TransportExceptionFilter, TransportGuard, TransportInterceptor, TransportPipe,
    ValidationOptions, WebSocketExceptionFilter, WebSocketGuard, WebSocketInterceptor,
    WebSocketPipe,
};
use std::any::TypeId;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) struct PipelineComponent<T: ?Sized> {
    type_id: TypeId,
    inner: Arc<T>,
}

impl<T: ?Sized> Clone for PipelineComponent<T> {
    fn clone(&self) -> Self {
        Self {
            type_id: self.type_id,
            inner: Arc::clone(&self.inner),
        }
    }
}

impl<T: ?Sized> PipelineComponent<T> {
    pub(crate) fn inner(&self) -> &T {
        self.inner.as_ref()
    }

    fn type_id(&self) -> TypeId {
        self.type_id
    }
}

impl PipelineComponent<dyn Pipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: Pipe,
    {
        Self {
            type_id: TypeId::of::<P>(),
            inner: Arc::new(pipe),
        }
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: Pipe,
        P: Pipe,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(pipe),
        }
    }
}

impl PipelineComponent<dyn Guard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: Guard,
    {
        Self {
            type_id: TypeId::of::<G>(),
            inner: Arc::new(guard),
        }
    }

    pub(crate) fn from_arc(guard: Arc<dyn Guard>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn Guard>>(),
            inner: guard,
        }
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: Guard,
        G: Guard,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(guard),
        }
    }
}

impl PipelineComponent<dyn Interceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: Interceptor,
    {
        Self {
            type_id: TypeId::of::<I>(),
            inner: Arc::new(interceptor),
        }
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: Interceptor,
        I: Interceptor,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(interceptor),
        }
    }
}

impl PipelineComponent<dyn ExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<F>(),
            inner: Arc::new(filter),
        }
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: ExceptionFilter,
        F: ExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(filter),
        }
    }
}

impl PipelineComponent<dyn WebSocketPipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: WebSocketPipe,
    {
        Self {
            type_id: TypeId::of::<P>(),
            inner: Arc::new(pipe),
        }
    }

    pub(crate) fn from_arc(pipe: Arc<dyn WebSocketPipe>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn WebSocketPipe>>(),
            inner: pipe,
        }
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: WebSocketPipe,
        P: WebSocketPipe,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(pipe),
        }
    }
}

impl PipelineComponent<dyn WebSocketGuard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: WebSocketGuard,
    {
        Self {
            type_id: TypeId::of::<G>(),
            inner: Arc::new(guard),
        }
    }

    pub(crate) fn from_arc(guard: Arc<dyn WebSocketGuard>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn WebSocketGuard>>(),
            inner: guard,
        }
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: WebSocketGuard,
        G: WebSocketGuard,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(guard),
        }
    }
}

impl PipelineComponent<dyn WebSocketInterceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: WebSocketInterceptor,
    {
        Self {
            type_id: TypeId::of::<I>(),
            inner: Arc::new(interceptor),
        }
    }

    pub(crate) fn from_arc(interceptor: Arc<dyn WebSocketInterceptor>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn WebSocketInterceptor>>(),
            inner: interceptor,
        }
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: WebSocketInterceptor,
        I: WebSocketInterceptor,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(interceptor),
        }
    }
}

impl PipelineComponent<dyn WebSocketExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: WebSocketExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<F>(),
            inner: Arc::new(filter),
        }
    }

    pub(crate) fn from_arc(filter: Arc<dyn WebSocketExceptionFilter>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn WebSocketExceptionFilter>>(),
            inner: filter,
        }
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: WebSocketExceptionFilter,
        F: WebSocketExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(filter),
        }
    }
}

impl PipelineComponent<dyn TransportPipe> {
    pub(crate) fn new<P>(pipe: P) -> Self
    where
        P: TransportPipe,
    {
        Self {
            type_id: TypeId::of::<P>(),
            inner: Arc::new(pipe),
        }
    }

    pub(crate) fn from_arc(pipe: Arc<dyn TransportPipe>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn TransportPipe>>(),
            inner: pipe,
        }
    }

    fn replacement<T, P>(pipe: P) -> Self
    where
        T: TransportPipe,
        P: TransportPipe,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(pipe),
        }
    }
}

impl PipelineComponent<dyn TransportGuard> {
    pub(crate) fn new<G>(guard: G) -> Self
    where
        G: TransportGuard,
    {
        Self {
            type_id: TypeId::of::<G>(),
            inner: Arc::new(guard),
        }
    }

    pub(crate) fn from_arc(guard: Arc<dyn TransportGuard>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn TransportGuard>>(),
            inner: guard,
        }
    }

    fn replacement<T, G>(guard: G) -> Self
    where
        T: TransportGuard,
        G: TransportGuard,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(guard),
        }
    }
}

impl PipelineComponent<dyn TransportInterceptor> {
    pub(crate) fn new<I>(interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        Self {
            type_id: TypeId::of::<I>(),
            inner: Arc::new(interceptor),
        }
    }

    pub(crate) fn from_arc(interceptor: Arc<dyn TransportInterceptor>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn TransportInterceptor>>(),
            inner: interceptor,
        }
    }

    fn replacement<T, I>(interceptor: I) -> Self
    where
        T: TransportInterceptor,
        I: TransportInterceptor,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(interceptor),
        }
    }
}

impl PipelineComponent<dyn TransportExceptionFilter> {
    pub(crate) fn new<F>(filter: F) -> Self
    where
        F: TransportExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<F>(),
            inner: Arc::new(filter),
        }
    }

    pub(crate) fn from_arc(filter: Arc<dyn TransportExceptionFilter>) -> Self {
        Self {
            type_id: TypeId::of::<Arc<dyn TransportExceptionFilter>>(),
            inner: filter,
        }
    }

    fn replacement<T, F>(filter: F) -> Self
    where
        T: TransportExceptionFilter,
        F: TransportExceptionFilter,
    {
        Self {
            type_id: TypeId::of::<T>(),
            inner: Arc::new(filter),
        }
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
