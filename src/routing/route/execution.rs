use super::definition::RouteDefinition;
use crate::routing::path::{join_paths, route_shape_key, route_specificity};
use crate::{BootError, BootRequest, BootResponse, ExecutionContext, MiddlewareOutcome, Result};

impl RouteDefinition {
    pub async fn call(&self, mut request: BootRequest) -> Result<BootResponse> {
        if request.method != self.method {
            let message = format!("{} {}", request.method.as_str(), request.path);
            return self
                .handle_error(
                    self.execution_context(request),
                    BootError::MethodNotAllowed(message),
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
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .handle_error(self.execution_context(request), error)
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
                    )
                    .await;
            }
            Err(error) => {
                return self
                    .handle_error(self.execution_context(request), error)
                    .await;
            }
        };
        request = request.with_host_params(host_params);
        if let Some(module_ref) = &self.module_ref {
            request = request.with_module_ref(module_ref.request_scope());
        }

        for middleware in &self.middleware {
            let context_request = request.clone();
            request = match middleware.handle(request).await {
                Ok(MiddlewareOutcome::Continue(request)) => request,
                Ok(MiddlewareOutcome::Respond(response)) => return Ok(response),
                Err(error) => {
                    return self
                        .handle_error(self.execution_context(context_request), error)
                        .await;
                }
            };
        }

        for pipe in &self.pipes {
            let context_request = request.clone();
            request = match pipe.transform(request).await {
                Ok(request) => request,
                Err(error) => {
                    return self
                        .handle_error(self.execution_context(context_request), error)
                        .await;
                }
            };
        }

        if self.validation_enabled {
            for validator in &self.validators {
                if let Err(error) = validator(&request) {
                    return self
                        .handle_error(self.execution_context(request.clone()), error)
                        .await;
                }
            }
        }

        let context = self.execution_context(request.clone());

        for guard in &self.guards {
            let can_activate = match guard.can_activate(context.clone()).await {
                Ok(can_activate) => can_activate,
                Err(error) => return self.handle_error(context.clone(), error).await,
            };

            if !can_activate {
                let message = format!("{} {}", context.method.as_str(), context.request_path);
                return self
                    .handle_error(context, BootError::Forbidden(message))
                    .await;
            }
        }

        for interceptor in &self.interceptors {
            if let Err(error) = interceptor.before(context.clone()).await {
                return self.handle_error(context.clone(), error).await;
            }
        }

        let mut response = match self.handler.call(request).await {
            Ok(response) => response,
            Err(error) => return self.handle_error(context, error).await,
        };

        for interceptor in self.interceptors.iter().rev() {
            response = match interceptor.after(context.clone(), response).await {
                Ok(response) => response,
                Err(error) => return self.handle_error(context.clone(), error).await,
            };
        }

        Ok(response)
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
    ) -> Result<BootResponse> {
        for filter in self.filters.iter().rev() {
            if let Some(response) = filter
                .catch(context.clone(), error.clone_for_filter())
                .await?
            {
                return Ok(response);
            }
        }
        Err(error)
    }
}
