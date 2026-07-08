use super::application::BootApplication;
use super::registration::{ModuleRegistrationSink, ModuleRegistry};
use crate::pipeline::PipelineComponents;
use crate::{
    ApiVersioning, BootError, BootResponse, ExceptionFilter, Guard, Interceptor,
    MessagePatternDefinition, Middleware, Module, ModuleRef, OpenApiDocument, OpenApiInfo, Pipe,
    ProviderDefinition, ProviderToken, Result, RouteDefinition, SerializationInterceptor,
    WebSocketGatewayDefinition,
};
#[cfg(feature = "compression")]
use crate::{CompressionInterceptor, CompressionOptions};
#[cfg(feature = "security")]
use crate::{
    CorsMiddleware, CorsOptions, CorsPreflightRoute, CorsResponseInterceptor, CsrfGuard,
    CsrfOptions, RateLimitGuard, RateLimitOptions, SecurityHeadersInterceptor,
    SecurityHeadersOptions,
};
#[cfg(feature = "session")]
use crate::{SessionCookieInterceptor, SessionManager, SessionMiddleware, SessionModule};
use std::collections::{BTreeMap, BTreeSet};
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
    provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
    #[cfg(feature = "security")]
    cors_preflight: Option<CorsPreflightRoute>,
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

    /// Add the default JSON response serialization interceptor.
    pub fn use_global_serialization(mut self) -> Self {
        self.global_pipeline
            .push_interceptor(SerializationInterceptor::new());
        self
    }

    /// Add gzip response compression for clients that send `Accept-Encoding: gzip`.
    #[cfg(feature = "compression")]
    pub fn use_global_compression(mut self, options: CompressionOptions) -> Self {
        self.global_pipeline
            .push_interceptor(CompressionInterceptor::with_options(options));
        self
    }

    /// Add CORS handling for preflight and normal responses.
    #[cfg(feature = "security")]
    pub fn use_global_cors(mut self, options: CorsOptions) -> Self {
        self.global_pipeline
            .push_middleware(CorsMiddleware::with_options(options.clone()));
        self.global_pipeline
            .push_interceptor(CorsResponseInterceptor::with_options(options.clone()));
        self.cors_preflight = Some(CorsPreflightRoute::with_options(options));
        self
    }

    /// Add common security response headers, similar to a small Helmet setup.
    #[cfg(feature = "security")]
    pub fn use_global_security_headers(mut self, options: SecurityHeadersOptions) -> Self {
        self.global_pipeline
            .push_interceptor(SecurityHeadersInterceptor::with_options(options));
        self
    }

    /// Add an application-wide CSRF guard for unsafe HTTP methods.
    #[cfg(feature = "security")]
    pub fn use_global_csrf(mut self, options: CsrfOptions) -> Self {
        self.global_pipeline
            .push_guard(CsrfGuard::with_options(options));
        self
    }

    /// Add an in-memory application-wide rate limit guard.
    #[cfg(feature = "security")]
    pub fn use_global_rate_limit(mut self, options: RateLimitOptions) -> Self {
        self.global_pipeline
            .push_guard(RateLimitGuard::with_options(options));
        self
    }

    /// Add global session middleware and cookie persistence.
    #[cfg(feature = "session")]
    pub fn use_global_sessions(mut self, manager: SessionManager) -> Self {
        self.global_pipeline
            .push_middleware(SessionMiddleware::new(manager.clone()));
        self.global_pipeline
            .push_interceptor(SessionCookieInterceptor::new(manager));
        self
    }

    /// Import a session module and apply its middleware/interceptor globally.
    #[cfg(feature = "session")]
    pub fn use_global_session_module(mut self, module: SessionModule) -> Self {
        let manager = module.manager();
        self = self.use_global_sessions(manager);
        self.modules.push(Arc::new(module));
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

    /// Replace matching module providers before modules build controllers.
    ///
    /// This is primarily used by testing utilities to swap provider
    /// implementations while preserving the application module graph.
    pub fn override_provider(mut self, provider: ProviderDefinition) -> Self {
        self.provider_overrides
            .insert(provider.token().clone(), provider);
        self
    }

    /// Resolve module imports, providers, controllers, and routes.
    pub fn build(self) -> Result<BootApplication> {
        let module_ref = ModuleRef::new();
        let global_ref = ModuleRef::new();
        module_ref.add_visible_scope(global_ref.clone())?;
        let mut registry = ModuleRegistry::new(global_ref, self.provider_overrides);
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

        #[cfg(feature = "security")]
        if let Some(cors_preflight) = &self.cors_preflight {
            add_cors_preflight_routes(&mut routes, cors_preflight, &self.global_pipeline)?;
        }

        routes = routes
            .into_iter()
            .map(|route| route.with_default_module_ref(module_ref.clone()))
            .collect();

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

#[cfg(feature = "security")]
fn add_cors_preflight_routes(
    routes: &mut Vec<RouteDefinition>,
    cors_preflight: &CorsPreflightRoute,
    pipeline: &PipelineComponents,
) -> Result<()> {
    let existing_options = routes
        .iter()
        .filter(|route| route.method() == crate::HttpMethod::Options)
        .map(RouteDefinition::path_shape)
        .collect::<BTreeSet<_>>();
    let mut generated = BTreeSet::new();
    let source_routes = routes.clone();

    for route in source_routes {
        if route.method() == crate::HttpMethod::Options {
            continue;
        }

        let path_shape = route.path_shape();
        if existing_options.contains(&path_shape) || !generated.insert(path_shape) {
            continue;
        }

        let preflight = RouteDefinition::options(route.path().to_string(), cors_preflight.clone())?
            .hide_from_openapi()
            .with_pipeline_prefix(pipeline);
        routes.push(preflight);
    }

    Ok(())
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
                || existing.host_shape_key() != route.host_shape_key()
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

            let host = route
                .host()
                .map(|host| format!(" host {host}"))
                .unwrap_or_default();
            return Err(BootError::DuplicateRoute(format!(
                "{} {}{} version {}",
                route.method().as_str(),
                route.path(),
                host,
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
