use super::definition::RouteDefinition;
use crate::routing::path::{extract_path_params, join_paths};
use crate::{BootError, BootRequest, BootResponse, ExecutionContext, Result};

impl RouteDefinition {
    pub async fn call(&self, mut request: BootRequest) -> Result<BootResponse> {
        let params = extract_path_params(&self.path, &request.path);
        request = request.with_path_params(params);

        for pipe in &self.pipes {
            request = pipe.transform(request).await?;
        }

        let context = ExecutionContext::new(
            request.clone(),
            self.path.clone(),
            self.module_name.clone(),
            self.controller_prefix.clone(),
        );

        for guard in &self.guards {
            if !guard.can_activate(context.clone()).await? {
                return self
                    .handle_error(
                        context,
                        BootError::Forbidden(format!("{} {}", self.method.as_str(), self.path)),
                    )
                    .await;
            }
        }

        for interceptor in &self.interceptors {
            interceptor.before(context.clone()).await?;
        }

        let mut response = match self.handler.call(request).await {
            Ok(response) => response,
            Err(error) => return self.handle_error(context, error).await,
        };

        for interceptor in self.interceptors.iter().rev() {
            response = interceptor.after(context.clone(), response).await?;
        }

        Ok(response)
    }

    pub(crate) fn with_prefix(mut self, prefix: &str) -> Result<Self> {
        self.path = join_paths(prefix, &self.path)?;
        self.controller_prefix = Some(prefix.trim_end_matches('/').to_string());
        Ok(self)
    }

    pub(crate) fn with_module_name(mut self, module_name: &str) -> Self {
        self.module_name = Some(module_name.to_string());
        self
    }

    async fn handle_error(
        &self,
        context: ExecutionContext,
        error: BootError,
    ) -> Result<BootResponse> {
        let mut current_error = error;
        for filter in self.filters.iter().rev() {
            match filter.catch(context.clone(), current_error).await? {
                Some(response) => return Ok(response),
                None => {
                    current_error =
                        BootError::Internal("exception filter returned no response".to_string());
                }
            }
        }
        Err(current_error)
    }
}
