use super::application::BootApplication;
use super::registration::{ModuleRegistrationSink, ModuleRegistry};
use crate::pipeline::PipelineComponents;
use crate::{
    ApiVersioning, BootError, BootResponse, ExceptionFilter, Guard, Interceptor,
    MessagePatternDefinition, Middleware, Module, ModuleRef, OpenApiDocument, OpenApiInfo, Pipe,
    Result, RouteDefinition, WebSocketGatewayDefinition,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Builder for a [`BootApplication`].
#[derive(Default)]
pub struct BootApplicationBuilder {
    modules: Vec<Arc<dyn Module>>,
    routes: Vec<RouteDefinition>,
    gateways: Vec<WebSocketGatewayDefinition>,
    message_patterns: Vec<MessagePatternDefinition>,
    global_pipeline: PipelineComponents,
    global_prefix: Option<String>,
    api_versioning: Option<ApiVersioning>,
    openapi_routes: Vec<(String, OpenApiInfo)>,
}

impl BootApplicationBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Import a root module.
    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.modules.push(Arc::new(module));
        self
    }

    /// Import a shared root module.
    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.modules.push(module);
        self
    }

    /// Add a framework-neutral route directly to the application shell.
    pub fn route(mut self, route: RouteDefinition) -> Self {
        self.routes.push(route);
        self
    }

    /// Add a framework-neutral WebSocket gateway directly to the application shell.
    pub fn gateway(mut self, gateway: WebSocketGatewayDefinition) -> Self {
        self.gateways.push(gateway);
        self
    }

    /// Add a framework-neutral microservice message pattern directly to the application shell.
    pub fn message_pattern(mut self, pattern: MessagePatternDefinition) -> Self {
        self.message_patterns.push(pattern);
        self
    }

    /// Serve a generated OpenAPI document at the given path.
    pub fn serve_openapi(mut self, path: impl Into<String>, info: OpenApiInfo) -> Self {
        self.openapi_routes.push((path.into(), info));
        self
    }

    /// Prefix every route in the built application, for example `/api/v1`.
    pub fn global_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.global_prefix = Some(prefix.into());
        self
    }

    /// Enable adapter-neutral API version matching for HTTP routes.
    pub fn enable_api_versioning(mut self, versioning: ApiVersioning) -> Self {
        self.api_versioning = Some(versioning);
        self
    }

    /// Add application-wide middleware, similar to Nest middleware.
    pub fn use_global_middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.global_pipeline.push_middleware(middleware);
        self
    }

    /// Add an application-wide pipe, similar to Nest's global pipes.
    pub fn use_global_pipe<P>(mut self, pipe: P) -> Self
    where
        P: Pipe,
    {
        self.global_pipeline.push_pipe(pipe);
        self
    }

    /// Add an application-wide guard, similar to Nest's global guards.
    pub fn use_global_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.global_pipeline.push_guard(guard);
        self
    }

    /// Add an application-wide interceptor, similar to Nest's global interceptors.
    pub fn use_global_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        self.global_pipeline.push_interceptor(interceptor);
        self
    }

    /// Add an application-wide exception filter, similar to Nest's global filters.
    pub fn use_global_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        self.global_pipeline.push_filter(filter);
        self
    }

    /// Enable DTO validation for routes that carry validation metadata.
    pub fn use_global_validation(mut self) -> Self {
        self.global_pipeline.enable_validation();
        self
    }

    /// Resolve module imports, providers, controllers, and routes.
    pub fn build(self) -> Result<BootApplication> {
        let module_ref = ModuleRef::new();
        let global_ref = ModuleRef::new();
        module_ref.add_visible_scope(global_ref.clone())?;
        let mut registry = ModuleRegistry::new(global_ref);
        let mut modules = Vec::new();
        let mut module_instances = Vec::new();
        let mut routes = self
            .routes
            .into_iter()
            .map(|route| route.with_pipeline_prefix(&self.global_pipeline))
            .collect::<Vec<_>>();
        let mut gateways = self.gateways;
        let mut message_patterns = self.message_patterns;

        {
            let mut sink = ModuleRegistrationSink {
                modules: &mut modules,
                module_instances: &mut module_instances,
                routes: &mut routes,
                gateways: &mut gateways,
                message_patterns: &mut message_patterns,
            };

            for module in &self.modules {
                let registered = registry.register_module(
                    Arc::clone(module),
                    &self.global_pipeline,
                    &mut sink,
                )?;
                module_ref.add_visible_scope(registered.module_ref)?;
            }
        }

        let mut routes = apply_global_prefix(routes, self.global_prefix.as_deref())?;
        let gateways = apply_global_gateway_prefix(gateways, self.global_prefix.as_deref())?;
        let documented_routes = routes.clone();

        for (path, info) in self.openapi_routes {
            let document = OpenApiDocument::from_routes(info, &documented_routes);
            let route =
                openapi_json_route(path, document)?.with_pipeline_prefix(&self.global_pipeline);
            let route = match self.global_prefix.as_deref() {
                Some(prefix) => route.with_path_prefix(prefix)?,
                None => route,
            };
            routes.push(route);
        }

        validate_unique_routes(&routes, self.api_versioning.as_ref())?;
        validate_unique_gateways(&gateways)?;
        validate_unique_message_patterns(&message_patterns)?;
        validate_gateway_route_conflicts(&routes, &gateways)?;

        Ok(BootApplication {
            routes,
            gateways,
            message_patterns,
            modules,
            module_ref,
            module_instances,
            api_versioning: self.api_versioning,
        })
    }
}

fn apply_global_gateway_prefix(
    gateways: Vec<WebSocketGatewayDefinition>,
    prefix: Option<&str>,
) -> Result<Vec<WebSocketGatewayDefinition>> {
    match prefix {
        Some(prefix) => gateways
            .into_iter()
            .map(|gateway| gateway.with_path_prefix(prefix))
            .collect(),
        None => Ok(gateways),
    }
}

fn openapi_json_route(path: String, document: OpenApiDocument) -> Result<RouteDefinition> {
    RouteDefinition::get(path, move |_| {
        let document = document.clone();
        async move { BootResponse::json(&document) }
    })
    .map(RouteDefinition::hide_from_openapi)
}

fn apply_global_prefix(
    routes: Vec<RouteDefinition>,
    prefix: Option<&str>,
) -> Result<Vec<RouteDefinition>> {
    match prefix {
        Some(prefix) => routes
            .into_iter()
            .map(|route| route.with_path_prefix(prefix))
            .collect(),
        None => Ok(routes),
    }
}

fn validate_unique_routes(
    routes: &[RouteDefinition],
    versioning: Option<&ApiVersioning>,
) -> Result<()> {
    for (index, route) in routes.iter().enumerate() {
        for existing in routes.iter().take(index) {
            if existing.method() != route.method()
                || existing.path_shape_key() != route.path_shape_key()
            {
                continue;
            }

            let duplicate = match versioning {
                Some(versioning) => existing
                    .versioning()
                    .overlaps(route.versioning(), versioning.default_version()),
                None => true,
            };
            if !duplicate {
                continue;
            }

            return Err(BootError::DuplicateRoute(format!(
                "{} {} version {}",
                route.method().as_str(),
                route.path(),
                route.versioning()
            )));
        }
    }

    Ok(())
}

fn validate_unique_gateways(gateways: &[WebSocketGatewayDefinition]) -> Result<()> {
    let mut seen = BTreeSet::new();

    for gateway in gateways {
        if !seen.insert(gateway.path_shape()) {
            return Err(BootError::DuplicateRoute(format!("WS {}", gateway.path())));
        }
    }

    Ok(())
}

fn validate_gateway_route_conflicts(
    routes: &[RouteDefinition],
    gateways: &[WebSocketGatewayDefinition],
) -> Result<()> {
    let get_routes = routes
        .iter()
        .filter(|route| route.method() == crate::HttpMethod::Get)
        .map(RouteDefinition::path_shape)
        .collect::<BTreeSet<_>>();

    for gateway in gateways {
        if get_routes.contains(&gateway.path_shape()) {
            return Err(BootError::DuplicateRoute(format!("GET {}", gateway.path())));
        }
    }

    Ok(())
}

fn validate_unique_message_patterns(patterns: &[MessagePatternDefinition]) -> Result<()> {
    let mut seen = BTreeSet::new();

    for pattern in patterns {
        if !seen.insert(pattern.pattern()) {
            return Err(BootError::DuplicateRoute(format!(
                "message pattern {}",
                pattern.pattern()
            )));
        }
    }

    Ok(())
}
