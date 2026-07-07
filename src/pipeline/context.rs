use crate::{BootRequest, HttpMethod, SerializationOptions};

/// Request context visible to guards, interceptors, pipes, and filters.
#[derive(Debug, Clone)]
pub struct ExecutionContext {
    pub method: HttpMethod,
    pub request_path: String,
    pub route_path: String,
    pub module_name: Option<String>,
    pub controller_prefix: Option<String>,
    pub serialization: SerializationOptions,
    pub request: BootRequest,
}

impl ExecutionContext {
    pub(crate) fn new(
        request: BootRequest,
        route_path: String,
        module_name: Option<String>,
        controller_prefix: Option<String>,
        serialization: SerializationOptions,
    ) -> Self {
        Self {
            method: request.method,
            request_path: request.path.clone(),
            route_path,
            module_name,
            controller_prefix,
            serialization,
            request,
        }
    }
}
