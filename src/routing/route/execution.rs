use super::definition::RouteDefinition;
use crate::routing::path::{join_paths, route_shape_key, route_specificity};
use crate::{
    BootError, BootRequest, BootResponse, CallHandler, ContextId, ContextIdFactory,
    ExecutionContext, MiddlewareOutcome, Result,
};
use std::sync::{Arc, Mutex};

#[derive(Clone)]
struct PipelineErrorContext {
    context: Arc<Mutex<ExecutionContext>>,
}

impl PipelineErrorContext {
    fn new(context: ExecutionContext) -> Self {
        Self {
            context: Arc::new(Mutex::new(context)),
        }
    }

    fn replace(&self, context: ExecutionContext) {
        *self
            .context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = context;
    }

    fn snapshot(&self) -> ExecutionContext {
        self.context
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }
}

impl RouteDefinition {
    pub async fn call(&self, mut request: BootRequest) -> Result<BootResponse> {
        let context_id = ContextIdFactory::create();
        request = self.attach_request_context(request, &context_id);
        if !self.method.matches(request.method) {
            let message = format!("{} {}", request.method.as_str(), request.path);
            return self
                .handle_error(
                    self.execution_context(request),
                    BootError::MethodNotAllowed(message),
                    &context_id,
                )
                .await;
        }

        let params = match self.path_params(&request.path) {
            Ok(Some(params)) => params,
            Ok(None) => {
                let message = format!("{} {}", request.method.as_str(), request.path);
                return self
                    .handle_error(
                        self.execution_context(request),
                        BootError::NotFound(message),
                        &context_id,
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .handle_error(self.execution_context(request), error, &context_id)
                    .await;
            }
        };
        request = request.with_path_params(params);
        let host_params = match self.host_params(request.host()) {
            Ok(Some(params)) => params,
            Ok(None) => {
                let message = format!("{} {}", request.method.as_str(), request.path);
                return self
                    .handle_error(
                        self.execution_context(request),
                        BootError::NotFound(message),
                        &context_id,
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .handle_error(self.execution_context(request), error, &context_id)
                    .await;
            }
        };
        request = request.with_host_params(host_params);
        #[cfg(feature = "request-context")]
        {
            let context = crate::RequestContext::from_route_request(
                &request,
                self.path.clone(),
                self.module_name.clone(),
                self.controller_prefix.clone(),
                self.metadata.clone(),
            );
            return crate::RequestContext::scope(context, self.call_pipeline(request, context_id))
                .await;
        }

        #[cfg(not(feature = "request-context"))]
        {
            self.call_pipeline(request, context_id).await
        }
    }

    async fn call_pipeline(
        &self,
        mut request: BootRequest,
        context_id: ContextId,
    ) -> Result<BootResponse> {
        for middleware in &self.middleware {
            let context_request = request.clone();
            request = match middleware.handle(request).await {
                Ok(MiddlewareOutcome::Continue(request)) => {
                    self.attach_request_context(request, &context_id)
                }
                Ok(MiddlewareOutcome::Respond(response)) => return Ok(response),
                Err(error) => {
                    return self
                        .handle_error(self.execution_context(context_request), error, &context_id)
                        .await;
                }
            };
        }

        let context = self.execution_context(request.clone());

        for guard in &self.guards {
            let guard = match guard.resolve(&context_id) {
                Ok(guard) => guard,
                Err(error) => {
                    return self.handle_error(context.clone(), error, &context_id).await;
                }
            };
            let can_activate = match guard.can_activate(context.clone()).await {
                Ok(can_activate) => can_activate,
                Err(error) => {
                    return self.handle_error(context.clone(), error, &context_id).await;
                }
            };

            if !can_activate {
                let message = format!("{} {}", context.method.as_str(), context.request_path);
                return self
                    .handle_error(context, BootError::Forbidden(message), &context_id)
                    .await;
            }
        }

        let mut resolved_interceptors = Vec::with_capacity(self.interceptors.len());
        for interceptor in &self.interceptors {
            match interceptor.resolve(&context_id) {
                Ok(interceptor) => resolved_interceptors.push(interceptor),
                Err(error) => {
                    return self.handle_error(context.clone(), error, &context_id).await;
                }
            }
        }

        let error_context = PipelineErrorContext::new(context.clone());
        let terminal_context = context.clone();
        let terminal_error_context = error_context.clone();
        let handler_context_id = context_id.clone();
        let mut next = CallHandler::from_fn(move || {
            terminal_error_context.replace(terminal_context.clone());
            self.call_handler_pipeline(
                request.clone(),
                terminal_error_context.clone(),
                handler_context_id.clone(),
            )
        });
        for interceptor in resolved_interceptors.iter().rev() {
            let interceptor_context = context.clone();
            let success_context = context.clone();
            let interceptor_error_context = error_context.clone();
            let downstream = next.clone();
            next = CallHandler::from_fn(move || {
                interceptor_error_context.replace(interceptor_context.clone());
                let future = interceptor.intercept(interceptor_context.clone(), downstream.clone());
                let success_context = success_context.clone();
                let interceptor_error_context = interceptor_error_context.clone();
                async move {
                    let result = future.await;
                    if result.is_ok() {
                        interceptor_error_context.replace(success_context);
                    }
                    result
                }
            });
        }

        match next.handle().await {
            Ok(response) => Ok(response),
            Err(error) => {
                self.handle_error(error_context.snapshot(), error, &context_id)
                    .await
            }
        }
    }

    async fn call_handler_pipeline(
        &self,
        mut request: BootRequest,
        error_context: PipelineErrorContext,
        context_id: ContextId,
    ) -> Result<BootResponse> {
        for pipe in &self.pipes {
            let context_request = request.clone();
            let pipe = pipe.resolve(&context_id)?;
            request = match pipe.transform(request).await {
                Ok(request) => self.attach_request_context(request, &context_id),
                Err(error) => {
                    error_context.replace(self.execution_context(context_request));
                    return Err(error);
                }
            };
        }

        if self.validation_enabled {
            for validator in &self.validators {
                let context_request = request.clone();
                request = match validator(request, self.validation_options) {
                    Ok(request) => request,
                    Err(error) => {
                        error_context.replace(self.execution_context(context_request));
                        return Err(error);
                    }
                };
            }
        }

        self.handler.call(request).await
    }

    /// Dispatch a request through this route and convert unhandled errors into Boot HTTP responses.
    pub async fn handle(&self, request: BootRequest) -> BootResponse {
        match self.call(request).await {
            Ok(response) => response,
            Err(error) => BootResponse::from_error(&error),
        }
    }

    pub(crate) fn matches_path_shape(&self, path: &str) -> bool {
        self.matches_path(path)
    }

    pub(crate) fn path_shape_key(&self) -> String {
        route_shape_key(&self.path)
    }

    pub(crate) fn path_specificity(&self) -> Vec<u8> {
        route_specificity(&self.path)
    }

    fn execution_context(&self, request: BootRequest) -> ExecutionContext {
        ExecutionContext::new(
            request,
            self.path.clone(),
            self.module_name.clone(),
            self.controller_prefix.clone(),
            self.serialization.clone(),
            self.metadata.clone(),
        )
    }

    pub(crate) fn with_prefix(mut self, prefix: &str) -> Result<Self> {
        self.path = join_paths(prefix, &self.path)?;
        self.controller_prefix = Some(prefix.trim_end_matches('/').to_string());
        Ok(self)
    }

    pub(crate) fn with_path_prefix(mut self, prefix: &str) -> Result<Self> {
        self.path = join_paths(prefix, &self.path)?;
        Ok(self)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }

    pub(crate) fn with_module_ref(mut self, module_ref: crate::ModuleRef) -> Self {
        self.module_ref = Some(module_ref);
        self
    }

    pub(crate) fn with_default_module_ref(mut self, module_ref: crate::ModuleRef) -> Self {
        if self.module_ref.is_none() {
            self.module_ref = Some(module_ref);
        }
        self
    }

    async fn handle_error(
        &self,
        context: ExecutionContext,
        error: BootError,
        context_id: &ContextId,
    ) -> Result<BootResponse> {
        for filter in self.filters.iter().rev() {
            let filter = filter.resolve(context_id)?;
            if let Some(response) = filter
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(response);
            }
        }
        Err(error)
    }

    fn attach_request_context(&self, request: BootRequest, context_id: &ContextId) -> BootRequest {
        match &self.module_ref {
            Some(module_ref) => request.with_module_ref(module_ref.context_scope(context_id)),
            None => request,
        }
    }
}
