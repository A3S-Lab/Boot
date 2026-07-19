use super::{
    ExecutionTransportGuard, ExecutionTransportInterceptor, IntoTransportReply, MessagePatternKind,
    TransportContext, TransportGuard, TransportInterceptor, TransportMessage, TransportPipe,
    TransportReply,
};
use crate::pipeline::{PipelineComponent, PipelineOverrides, ProviderEnhancerComponents};
use crate::{
    catch_errors, validate_json_value_with_options, BootError, BootErrorKind, BootRequest,
    BoxFuture, CallHandler, ContextId, ContextIdFactory, ExecutionInterceptor, Guard, HttpMethod,
    ModuleRef, Result, TransportExceptionFilter, Validate, ValidationOptions, ValidationSchema,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::{Arc, Mutex};

type TransportHandlerFuture = BoxFuture<'static, Result<Option<TransportReply>>>;
type MessageValidator =
    Arc<dyn Fn(TransportMessage, ValidationOptions) -> Result<TransportMessage> + Send + Sync>;

trait TransportMessageHandler: Send + Sync + 'static {
    fn call(&self, message: TransportMessage) -> TransportHandlerFuture;
}

struct TransportHandlerAdapter<H> {
    handler: H,
}

impl<H, Fut, R> TransportMessageHandler for TransportHandlerAdapter<H>
where
    H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoTransportReply + Send + 'static,
{
    fn call(&self, message: TransportMessage) -> TransportHandlerFuture {
        let future = (self.handler)(message);
        Box::pin(async move { Ok(future.await?.into_transport_reply()) })
    }
}

type ScopedTransportHandlerFactory =
    dyn Fn(&ModuleRef) -> Result<Arc<dyn TransportMessageHandler>> + Send + Sync;

#[derive(Clone)]
enum TransportHandlerDefinition {
    Static(Arc<dyn TransportMessageHandler>),
    Scoped(Arc<ScopedTransportHandlerFactory>),
}

impl TransportHandlerDefinition {
    fn resolve(
        &self,
        module_ref: Option<&ModuleRef>,
        pattern: &str,
    ) -> Result<Arc<dyn TransportMessageHandler>> {
        match self {
            Self::Static(handler) => Ok(Arc::clone(handler)),
            Self::Scoped(factory) => {
                let module_ref = module_ref.ok_or_else(|| {
                    BootError::Internal(format!(
                        "scoped transport message pattern `{pattern}` requires a declaring or default module context"
                    ))
                })?;
                factory(module_ref)
            }
        }
    }

    fn is_scoped(&self) -> bool {
        matches!(self, Self::Scoped(_))
    }
}

#[derive(Default)]
struct DispatchHandlerCache {
    handler: Mutex<Option<Arc<dyn TransportMessageHandler>>>,
}

impl DispatchHandlerCache {
    fn resolve(
        &self,
        definition: &TransportHandlerDefinition,
        module_ref: Option<&ModuleRef>,
        pattern: &str,
    ) -> Result<Arc<dyn TransportMessageHandler>> {
        let mut cached = self
            .handler
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(handler) = cached.as_ref() {
            return Ok(Arc::clone(handler));
        }

        let handler = definition.resolve(module_ref, pattern)?;
        *cached = Some(Arc::clone(&handler));
        Ok(handler)
    }
}

#[derive(Clone)]
struct MessageDispatchState {
    context_id: ContextId,
    module_ref: Option<ModuleRef>,
    handler: Arc<DispatchHandlerCache>,
}

impl MessageDispatchState {
    fn new(module_ref: Option<&ModuleRef>) -> Self {
        let context_id = ContextIdFactory::create();
        let module_ref = module_ref.map(|module_ref| module_ref.context_scope(&context_id));
        Self {
            context_id,
            module_ref,
            handler: Arc::new(DispatchHandlerCache::default()),
        }
    }

    fn request(&self) -> BootRequest {
        let request = BootRequest::new(HttpMethod::Post, "/__transport");
        match &self.module_ref {
            Some(module_ref) => request.with_module_ref(module_ref.clone()),
            None => request,
        }
    }
}

/// Framework-neutral message pattern handler definition.
#[derive(Clone)]
pub struct MessagePatternDefinition {
    pattern: String,
    kind: MessagePatternKind,
    handler: TransportHandlerDefinition,
    pipes: Vec<PipelineComponent<dyn TransportPipe>>,
    guards: Vec<PipelineComponent<dyn TransportGuard>>,
    interceptors: Vec<PipelineComponent<dyn TransportInterceptor>>,
    filters: Vec<PipelineComponent<dyn TransportExceptionFilter>>,
    validators: Vec<MessageValidator>,
    validation_enabled: bool,
    validation_disabled: bool,
    validation_options: ValidationOptions,
    metadata: BTreeMap<String, Value>,
    module_name: Option<String>,
    module_ref: Option<ModuleRef>,
}

impl MessagePatternDefinition {
    pub fn request<H, Fut, R>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        Self::new(pattern, MessagePatternKind::RequestResponse, handler)
    }

    /// Build a request-response pattern whose handler is created from the
    /// current message dispatch's dependency-injection scope.
    pub fn request_scoped<F, H, Fut, R>(pattern: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        Self::new_scoped(pattern, MessagePatternKind::RequestResponse, factory)
    }

    pub fn event<H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::new(pattern, MessagePatternKind::Event, handler)
    }

    /// Build an event pattern whose handler is created from the current
    /// message dispatch's dependency-injection scope.
    pub fn event_scoped<F, H, Fut>(pattern: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::new_scoped(pattern, MessagePatternKind::Event, factory)
    }

    pub fn request_json<T, H, Fut, R>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::request(pattern, move |message: TransportMessage| {
            let payload = message.data_as::<T>();
            let future = payload.map(&handler);
            async move {
                let response = future?.await?;
                TransportReply::json(&response)
            }
        })
    }

    pub fn request_validated_json<T, H, Fut, R>(
        pattern: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::request(pattern, move |message: TransportMessage| {
            let payload = message.validated_data::<T>();
            let future = payload.map(&handler);
            async move {
                let response = future?.await?;
                TransportReply::json(&response)
            }
        })
    }

    pub fn event_json<T, H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::event(pattern, move |message: TransportMessage| {
            let payload = message.data_as::<T>();
            let future = payload.map(&handler);
            async move { future?.await }
        })
    }

    pub fn event_validated_json<T, H, Fut>(pattern: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self::event(pattern, move |message: TransportMessage| {
            let payload = message.validated_data::<T>();
            let future = payload.map(&handler);
            async move { future?.await }
        })
    }

    fn new<H, Fut, R>(
        pattern: impl Into<String>,
        kind: MessagePatternKind,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        Self::from_handler(
            pattern,
            kind,
            TransportHandlerDefinition::Static(Arc::new(TransportHandlerAdapter { handler })),
        )
    }

    fn new_scoped<F, H, Fut, R>(
        pattern: impl Into<String>,
        kind: MessagePatternKind,
        factory: F,
    ) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: Fn(TransportMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: IntoTransportReply + Send + 'static,
    {
        let factory = move |module_ref: &ModuleRef| {
            let handler = factory(module_ref)?;
            Ok(Arc::new(TransportHandlerAdapter { handler }) as Arc<dyn TransportMessageHandler>)
        };
        Self::from_handler(
            pattern,
            kind,
            TransportHandlerDefinition::Scoped(Arc::new(factory)),
        )
    }

    fn from_handler(
        pattern: impl Into<String>,
        kind: MessagePatternKind,
        handler: TransportHandlerDefinition,
    ) -> Result<Self> {
        let pattern = pattern.into();
        validate_pattern(&pattern)?;
        Ok(Self {
            pattern,
            kind,
            handler,
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            validation_options: ValidationOptions::default(),
            metadata: BTreeMap::new(),
            module_name: None,
            module_ref: None,
        })
    }

    pub fn pattern(&self) -> &str {
        &self.pattern
    }

    pub fn kind(&self) -> MessagePatternKind {
        self.kind
    }

    /// Return whether this pattern constructs its handler from each dispatch scope.
    pub fn is_scoped(&self) -> bool {
        self.handler.is_scoped()
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn with_metadata<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize message pattern metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: TransportPipe,
    {
        self.pipes
            .push(PipelineComponent::<dyn TransportPipe>::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: TransportGuard,
    {
        self.guards
            .push(PipelineComponent::<dyn TransportGuard>::new(guard));
        self
    }

    pub fn with_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards
            .push(PipelineComponent::<dyn TransportGuard>::new(
                ExecutionTransportGuard { inner: guard },
            ));
        self
    }

    pub(crate) fn with_execution_pipeline_prefix(
        mut self,
        guards: &[Arc<dyn Guard>],
        interceptors: &[Arc<dyn ExecutionInterceptor>],
    ) -> Self {
        self.guards = prepend_execution_guards(guards, self.guards);
        self.interceptors = prepend_execution_interceptors(interceptors, self.interceptors);
        self
    }

    pub(crate) fn with_guard_prefix(mut self, guards: &[Arc<dyn TransportGuard>]) -> Self {
        let mut merged = guards
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportGuard>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.guards);
        self.guards = merged;
        self
    }

    pub(crate) fn with_interceptor_prefix(
        mut self,
        interceptors: &[Arc<dyn TransportInterceptor>],
    ) -> Self {
        let mut merged = interceptors
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportInterceptor>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.interceptors);
        self.interceptors = merged;
        self
    }

    pub(crate) fn with_pipe_prefix(mut self, pipes: &[Arc<dyn TransportPipe>]) -> Self {
        let mut merged = pipes
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportPipe>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.pipes);
        self.pipes = merged;
        self
    }

    pub(crate) fn with_filter_prefix(
        mut self,
        filters: &[Arc<dyn TransportExceptionFilter>],
    ) -> Self {
        let mut merged = filters
            .iter()
            .cloned()
            .map(PipelineComponent::<dyn TransportExceptionFilter>::from_arc)
            .collect::<Vec<_>>();
        merged.extend(self.filters);
        self.filters = merged;
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn TransportInterceptor>::new(
                interceptor,
            ));
        self
    }

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn TransportInterceptor>::new(
                ExecutionTransportInterceptor { inner: interceptor },
            ));
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: TransportExceptionFilter,
    {
        self.filters
            .push(PipelineComponent::<dyn TransportExceptionFilter>::new(
                filter,
            ));
        self
    }

    pub fn with_catch_filter<I, F>(self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: TransportExceptionFilter,
    {
        self.with_filter(catch_errors(kinds, filter))
    }

    pub(crate) fn with_pipeline_overrides(mut self, overrides: &PipelineOverrides) -> Self {
        overrides.apply_to_transport_pipes(&mut self.pipes);
        overrides.apply_to_transport_guards(&mut self.guards);
        overrides.apply_to_transport_interceptors(&mut self.interceptors);
        overrides.apply_to_transport_filters(&mut self.filters);
        self
    }

    pub(crate) fn with_provider_enhancer_prefix(
        mut self,
        enhancers: &ProviderEnhancerComponents,
    ) -> Self {
        let mut pipes = enhancers.transport_pipes.clone();
        pipes.extend(self.pipes);
        self.pipes = pipes;

        let mut guards = enhancers.transport_guards.clone();
        guards.extend(self.guards);
        self.guards = guards;

        let mut interceptors = enhancers.transport_interceptors.clone();
        interceptors.extend(self.interceptors);
        self.interceptors = interceptors;

        let mut filters = enhancers.transport_filters.clone();
        filters.extend(self.filters);
        self.filters = filters;
        self
    }

    pub fn with_validation(mut self) -> Self {
        self.validation_enabled = true;
        self.validation_disabled = false;
        self
    }

    pub fn with_validation_options(mut self, options: ValidationOptions) -> Self {
        self.validation_enabled = true;
        self.validation_disabled = false;
        self.validation_options = self.validation_options.merge(options);
        self
    }

    pub fn without_validation(mut self) -> Self {
        self.validation_enabled = false;
        self.validation_disabled = true;
        self
    }

    pub(crate) fn with_validation_prefix(
        mut self,
        validation_enabled: bool,
        validation_options: ValidationOptions,
    ) -> Self {
        if !self.validation_disabled {
            self.validation_enabled = validation_enabled || self.validation_enabled;
            self.validation_options = validation_options.merge(self.validation_options);
        }
        self
    }

    pub fn with_payload_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(Arc::new(|message, _| {
            message.validated_data::<T>().map(|_| message)
        }));
        self.with_validation()
    }

    pub fn with_payload_validation_options<T>(mut self, options: ValidationOptions) -> Self
    where
        T: DeserializeOwned + Serialize + Validate + ValidationSchema + 'static,
    {
        self.validators
            .push(Arc::new(move |mut message, inherited_options| {
                let options = inherited_options.merge(options);
                let data = validate_json_value_with_options::<T>(
                    message.data.clone(),
                    options,
                    "message property",
                )?;
                if options.transform || options.whitelist {
                    message.data = data;
                }
                Ok(message)
            }));
        self.with_validation()
    }

    pub async fn dispatch(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let state = MessageDispatchState::new(self.module_ref.as_ref());
        let context = TransportContext::new(self, &message.pattern, state.request());
        match self
            .dispatch_pipeline(message, context.clone(), state.clone())
            .await
        {
            Ok(reply) => Ok(reply),
            Err(error) => self.handle_error(context, error, &state.context_id).await,
        }
    }

    async fn dispatch_pipeline(
        &self,
        message: TransportMessage,
        context: TransportContext,
        state: MessageDispatchState,
    ) -> Result<Option<TransportReply>> {
        if message.pattern != self.pattern {
            return Err(BootError::NotFound(format!(
                "message pattern {}",
                message.pattern
            )));
        }

        for guard in &self.guards {
            let can_activate = guard
                .resolve(&state.context_id)?
                .can_activate(context.clone())
                .await?;
            if !can_activate {
                return Err(BootError::Forbidden(format!(
                    "message pattern {}",
                    message.pattern
                )));
            }
        }

        let reply = self
            .dispatch_interceptor_chain(0, context, message, state)
            .await?;
        Ok(if self.kind == MessagePatternKind::Event {
            None
        } else {
            reply
        })
    }

    fn dispatch_interceptor_chain<'a>(
        &'a self,
        index: usize,
        context: TransportContext,
        message: TransportMessage,
        state: MessageDispatchState,
    ) -> BoxFuture<'a, Result<Option<TransportReply>>> {
        Box::pin(async move {
            let Some(interceptor) = self.interceptors.get(index) else {
                return self.dispatch_handler_pipeline(message, state).await;
            };
            let interceptor = interceptor.resolve(&state.context_id)?;

            let next_context = context.clone();
            let next_message = message.clone();
            let next_state = state.clone();
            let next = CallHandler::from_fn(move || {
                self.dispatch_interceptor_chain(
                    index + 1,
                    next_context.clone(),
                    next_message.clone(),
                    next_state.clone(),
                )
            });
            interceptor.intercept(context, next).await
        })
    }

    async fn dispatch_handler_pipeline(
        &self,
        mut message: TransportMessage,
        state: MessageDispatchState,
    ) -> Result<Option<TransportReply>> {
        for pipe in &self.pipes {
            message = pipe.resolve(&state.context_id)?.transform(message).await?;
        }

        if self.validation_enabled {
            for validator in &self.validators {
                message = validator(message, self.validation_options)?;
            }
        }

        let handler = state.handler.resolve(
            &self.handler,
            state.module_ref.as_ref(),
            self.pattern.as_str(),
        )?;
        let mut reply = handler.call(message).await?;
        if self.kind == MessagePatternKind::Event {
            reply = None;
        }
        Ok(reply)
    }

    async fn handle_error(
        &self,
        context: TransportContext,
        error: BootError,
        context_id: &ContextId,
    ) -> Result<Option<TransportReply>> {
        for filter in self.filters.iter().rev() {
            let filter = filter.resolve(context_id)?;
            if let Some(response) = filter
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(if self.kind == MessagePatternKind::Event {
                    None
                } else {
                    response.into_reply()
                });
            }
        }
        Err(error)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }

    pub(crate) fn with_module_ref(mut self, module_ref: ModuleRef) -> Self {
        self.module_ref = Some(module_ref);
        self
    }

    pub(crate) fn with_default_module_ref(mut self, module_ref: ModuleRef) -> Self {
        if self.module_ref.is_none() {
            self.module_ref = Some(module_ref);
        }
        self
    }
}

fn prepend_execution_guards(
    prefix: &[Arc<dyn Guard>],
    values: Vec<PipelineComponent<dyn TransportGuard>>,
) -> Vec<PipelineComponent<dyn TransportGuard>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|guard| {
            PipelineComponent::<dyn TransportGuard>::new(ExecutionTransportGuard { inner: guard })
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

fn prepend_execution_interceptors(
    prefix: &[Arc<dyn ExecutionInterceptor>],
    values: Vec<PipelineComponent<dyn TransportInterceptor>>,
) -> Vec<PipelineComponent<dyn TransportInterceptor>> {
    let mut merged = prefix
        .iter()
        .cloned()
        .map(|interceptor| {
            PipelineComponent::<dyn TransportInterceptor>::new(ExecutionTransportInterceptor {
                inner: interceptor,
            })
        })
        .collect::<Vec<_>>();
    merged.extend(values);
    merged
}

fn validate_pattern(pattern: &str) -> Result<()> {
    if pattern.trim().is_empty() {
        return Err(BootError::BadRequest(
            "message pattern cannot be empty".to_string(),
        ));
    }
    Ok(())
}
