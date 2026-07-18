use super::application::BootApplication;
use super::lazy::LazyModuleLoader;
use super::registration::{ModuleRegistrationSink, ModuleRegistry};
use crate::pipeline::PipelineComponents;
#[cfg(feature = "auth")]
use crate::AuthGuard;
use crate::{
    catch_errors, ApiVersioning, BootError, BootErrorKind, BootResponse, ExceptionFilter,
    ExecutionInterceptor, Guard, Interceptor, MessagePatternDefinition, Middleware,
    MiddlewareRoute, Module, ModuleRef, OpenApiDocument, OpenApiInfo, Pipe, ProviderDefinition,
    ProviderToken, Result, RouteDefinition, SerializationInterceptor, TransportExceptionFilter,
    TransportGuard, TransportInterceptor, TransportPipe, ValidationOptions,
    WebSocketExceptionFilter, WebSocketGatewayDefinition, WebSocketGuard, WebSocketInterceptor,
    WebSocketPipe,
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
    global_execution_guards: Vec<Arc<dyn Guard>>,
    global_execution_interceptors: Vec<Arc<dyn ExecutionInterceptor>>,
    global_websocket_guards: Vec<Arc<dyn WebSocketGuard>>,
    global_websocket_interceptors: Vec<Arc<dyn WebSocketInterceptor>>,
    global_websocket_pipes: Vec<Arc<dyn WebSocketPipe>>,
    global_websocket_filters: Vec<Arc<dyn WebSocketExceptionFilter>>,
    global_transport_guards: Vec<Arc<dyn TransportGuard>>,
    global_transport_interceptors: Vec<Arc<dyn TransportInterceptor>>,
    global_transport_pipes: Vec<Arc<dyn TransportPipe>>,
    global_transport_filters: Vec<Arc<dyn TransportExceptionFilter>>,
    global_prefix: Option<String>,
    global_prefix_exclusions: Vec<MiddlewareRoute>,
    api_versioning: Option<ApiVersioning>,
    openapi_routes: Vec<(String, OpenApiInfo)>,
    openapi_ui_routes: Vec<OpenApiUiRoute>,
    provider_overrides: BTreeMap<ProviderToken, ProviderDefinition>,
    module_overrides: BTreeMap<String, Arc<dyn Module>>,
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

    /// Serve a generated OpenAPI document and a Swagger UI page for it.
    pub fn serve_openapi_ui(
        mut self,
        path: impl Into<String>,
        document_path: impl Into<String>,
        info: OpenApiInfo,
    ) -> Self {
        self.openapi_ui_routes.push(OpenApiUiRoute {
            path: path.into(),
            document_path: document_path.into(),
            info,
        });
        self
    }

    /// Prefix every route in the built application, for example `/api/v1`.
    pub fn global_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.global_prefix = Some(prefix.into());
        self
    }

    /// Exclude selected HTTP routes from the application-wide global prefix.
    pub fn exclude_global_prefix<I>(mut self, routes: I) -> Self
    where
        I: IntoIterator<Item = MiddlewareRoute>,
    {
        self.global_prefix_exclusions.extend(routes);
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

    /// Add an application-wide WebSocket pipe.
    pub fn use_global_websocket_pipe<P>(mut self, pipe: P) -> Self
    where
        P: WebSocketPipe,
    {
        self.global_websocket_pipes.push(Arc::new(pipe));
        self
    }

    /// Add an application-wide WebSocket guard.
    pub fn use_global_websocket_guard<G>(mut self, guard: G) -> Self
    where
        G: WebSocketGuard,
    {
        self.global_websocket_guards.push(Arc::new(guard));
        self
    }

    /// Add an application-wide WebSocket interceptor.
    pub fn use_global_websocket_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: WebSocketInterceptor,
    {
        self.global_websocket_interceptors
            .push(Arc::new(interceptor));
        self
    }

    /// Add an application-wide transport pipe.
    pub fn use_global_transport_pipe<P>(mut self, pipe: P) -> Self
    where
        P: TransportPipe,
    {
        self.global_transport_pipes.push(Arc::new(pipe));
        self
    }

    /// Add an application-wide transport guard.
    pub fn use_global_transport_guard<G>(mut self, guard: G) -> Self
    where
        G: TransportGuard,
    {
        self.global_transport_guards.push(Arc::new(guard));
        self
    }

    /// Add an application-wide transport interceptor.
    pub fn use_global_transport_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: TransportInterceptor,
    {
        self.global_transport_interceptors
            .push(Arc::new(interceptor));
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

    /// Add an application-wide protocol-neutral guard.
    pub fn use_global_execution_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        let guard: Arc<dyn Guard> = Arc::new(guard);
        self.global_pipeline.push_guard_arc(Arc::clone(&guard));
        self.global_execution_guards.push(guard);
        self
    }

    /// Add a provider-backed authentication guard using the default auth strategy.
    #[cfg(feature = "auth")]
    pub fn use_global_auth(mut self) -> Self {
        self.global_pipeline.push_guard(AuthGuard::new());
        self
    }

    /// Add a provider-backed authentication guard using a named auth strategy.
    #[cfg(feature = "auth")]
    pub fn use_global_auth_strategy(mut self, strategy: impl Into<String>) -> Self {
        self.global_pipeline
            .push_guard(AuthGuard::new().strategy(strategy));
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

    /// Add an application-wide protocol-neutral interceptor.
    pub fn use_global_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        let interceptor: Arc<dyn ExecutionInterceptor> = Arc::new(interceptor);
        self.global_pipeline
            .push_execution_interceptor_arc(Arc::clone(&interceptor));
        self.global_execution_interceptors.push(interceptor);
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
        self.modules.push(Arc::new(module.global()));
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

    /// Add an application-wide exception filter for selected error kinds.
    pub fn use_global_catch_filter<I, F>(mut self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: ExceptionFilter,
    {
        self.global_pipeline.push_catch_filter(kinds, filter);
        self
    }

    /// Add an application-wide WebSocket exception filter.
    pub fn use_global_websocket_filter<F>(mut self, filter: F) -> Self
    where
        F: WebSocketExceptionFilter,
    {
        self.global_websocket_filters.push(Arc::new(filter));
        self
    }

    /// Add an application-wide WebSocket exception filter for selected error kinds.
    pub fn use_global_websocket_catch_filter<I, F>(mut self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: WebSocketExceptionFilter,
    {
        self.global_websocket_filters
            .push(Arc::new(catch_errors(kinds, filter)));
        self
    }

    /// Add an application-wide transport exception filter.
    pub fn use_global_transport_filter<F>(mut self, filter: F) -> Self
    where
        F: TransportExceptionFilter,
    {
        self.global_transport_filters.push(Arc::new(filter));
        self
    }

    /// Add an application-wide transport exception filter for selected error kinds.
    pub fn use_global_transport_catch_filter<I, F>(mut self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: TransportExceptionFilter,
    {
        self.global_transport_filters
            .push(Arc::new(catch_errors(kinds, filter)));
        self
    }

    /// Enable DTO validation for routes that carry validation metadata.
    pub fn use_global_validation(mut self) -> Self {
        self.global_pipeline.enable_validation();
        self
    }

    /// Enable DTO validation with Nest-style validation options.
    pub fn use_global_validation_options(mut self, options: ValidationOptions) -> Self {
        self.global_pipeline.enable_validation_with_options(options);
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

    /// Replace a module by name before the application graph is registered.
    ///
    /// This is primarily used by testing utilities to mirror Nest's
    /// `overrideModule(...).useModule(...)` workflow. The replacement module is
    /// registered wherever a module with the target name appears in the import
    /// graph.
    pub fn override_module<M>(mut self, target_name: impl Into<String>, module: M) -> Self
    where
        M: Module,
    {
        self.module_overrides
            .insert(target_name.into(), Arc::new(module));
        self
    }

    /// Replace a shared module by name before the application graph is registered.
    pub fn override_module_arc(
        mut self,
        target_name: impl Into<String>,
        module: Arc<dyn Module>,
    ) -> Self {
        self.module_overrides.insert(target_name.into(), module);
        self
    }

    /// Resolve module imports, providers, controllers, and routes.
    pub fn build(self) -> Result<BootApplication> {
        let module_ref = ModuleRef::new();
        let global_ref = ModuleRef::new();
        let lazy_module_loader = LazyModuleLoader::new(global_ref.clone());
        global_ref.insert_arc(Arc::new(lazy_module_loader.clone()))?;
        module_ref.add_visible_scope(global_ref.clone())?;
        let mut registry =
            ModuleRegistry::new(global_ref, self.provider_overrides, self.module_overrides);
        let mut modules = Vec::new();
        let mut module_instances = Vec::new();
        let mut routes = self.routes;
        let mut gateways = self.gateways;
        let mut message_patterns = self.message_patterns;

        for module in &self.modules {
            let registered = registry.register_module(Arc::clone(module))?;
            module_ref.add_visible_scope(registered.module_ref)?;
        }
        let provider_enhancers = registry.provider_enhancers();
        let mut effective_global_pipeline = self.global_pipeline.clone();
        effective_global_pipeline.append(&provider_enhancers.http);
        routes = routes
            .into_iter()
            .map(|route| route.with_pipeline_prefix(&effective_global_pipeline))
            .collect();
        {
            let mut sink = ModuleRegistrationSink {
                modules: &mut modules,
                module_instances: &mut module_instances,
                routes: &mut routes,
                gateways: &mut gateways,
                message_patterns: &mut message_patterns,
            };
            registry.finalize(&effective_global_pipeline, &mut sink)?;
        }
        for (name, registered) in registry.registered_modules() {
            lazy_module_loader.seed_module(name, registered.module_ref, registered.exports)?;
        }

        let mut routes = apply_global_prefix(
            routes,
            self.global_prefix.as_deref(),
            &self.global_prefix_exclusions,
        )?;
        let gateways = apply_global_gateway_prefix(gateways, self.global_prefix.as_deref())?
            .into_iter()
            .map(|gateway| {
                gateway
                    .with_provider_enhancer_prefix(&provider_enhancers)
                    .with_guard_prefix(&self.global_websocket_guards)
                    .with_interceptor_prefix(&self.global_websocket_interceptors)
                    .with_execution_pipeline_prefix(
                        &self.global_execution_guards,
                        &self.global_execution_interceptors,
                    )
                    .with_pipe_prefix(&self.global_websocket_pipes)
                    .with_filter_prefix(&self.global_websocket_filters)
                    .with_validation_prefix(
                        self.global_pipeline.validation_enabled,
                        self.global_pipeline.validation_options,
                    )
                    .with_default_module_ref(module_ref.clone())
            })
            .collect::<Vec<_>>();
        let message_patterns = message_patterns
            .into_iter()
            .map(|pattern| {
                pattern
                    .with_provider_enhancer_prefix(&provider_enhancers)
                    .with_guard_prefix(&self.global_transport_guards)
                    .with_interceptor_prefix(&self.global_transport_interceptors)
                    .with_execution_pipeline_prefix(
                        &self.global_execution_guards,
                        &self.global_execution_interceptors,
                    )
                    .with_pipe_prefix(&self.global_transport_pipes)
                    .with_filter_prefix(&self.global_transport_filters)
                    .with_validation_prefix(
                        self.global_pipeline.validation_enabled,
                        self.global_pipeline.validation_options,
                    )
                    .with_default_module_ref(module_ref.clone())
            })
            .collect::<Vec<_>>();
        let documented_routes = routes.clone();

        for (path, info) in self.openapi_routes {
            let document = OpenApiDocument::from_routes(info, &documented_routes);
            let route = openapi_json_route(path, document)?
                .with_pipeline_prefix(&effective_global_pipeline);
            let route = apply_global_prefix_to_route(
                route,
                self.global_prefix.as_deref(),
                &self.global_prefix_exclusions,
            )?;
            routes.push(route);
        }
        for ui in self.openapi_ui_routes {
            add_openapi_ui_routes(
                &mut routes,
                ui,
                &documented_routes,
                self.global_prefix.as_deref(),
                &self.global_prefix_exclusions,
                &effective_global_pipeline,
            )?;
        }

        #[cfg(feature = "security")]
        if let Some(cors_preflight) = &self.cors_preflight {
            add_cors_preflight_routes(&mut routes, cors_preflight, &effective_global_pipeline)?;
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
            global_filters: effective_global_pipeline.filters.clone(),
        })
    }

    /// Resolve modules with async provider factories, then build the application.
    pub async fn build_async(self) -> Result<BootApplication> {
        let module_ref = ModuleRef::new();
        let global_ref = ModuleRef::new();
        let lazy_module_loader = LazyModuleLoader::new(global_ref.clone());
        global_ref.insert_arc(Arc::new(lazy_module_loader.clone()))?;
        module_ref.add_visible_scope(global_ref.clone())?;
        let mut registry =
            ModuleRegistry::new(global_ref, self.provider_overrides, self.module_overrides);
        let mut modules = Vec::new();
        let mut module_instances = Vec::new();
        let mut routes = self.routes;
        let mut gateways = self.gateways;
        let mut message_patterns = self.message_patterns;

        for module in &self.modules {
            let registered = registry.register_module_async(Arc::clone(module)).await?;
            module_ref.add_visible_scope(registered.module_ref)?;
        }
        let provider_enhancers = registry.provider_enhancers();
        let mut effective_global_pipeline = self.global_pipeline.clone();
        effective_global_pipeline.append(&provider_enhancers.http);
        routes = routes
            .into_iter()
            .map(|route| route.with_pipeline_prefix(&effective_global_pipeline))
            .collect();
        {
            let mut sink = ModuleRegistrationSink {
                modules: &mut modules,
                module_instances: &mut module_instances,
                routes: &mut routes,
                gateways: &mut gateways,
                message_patterns: &mut message_patterns,
            };
            registry
                .finalize_async(&effective_global_pipeline, &mut sink)
                .await?;
        }
        for (name, registered) in registry.registered_modules() {
            lazy_module_loader.seed_module(name, registered.module_ref, registered.exports)?;
        }

        let mut routes = apply_global_prefix(
            routes,
            self.global_prefix.as_deref(),
            &self.global_prefix_exclusions,
        )?;
        let gateways = apply_global_gateway_prefix(gateways, self.global_prefix.as_deref())?
            .into_iter()
            .map(|gateway| {
                gateway
                    .with_provider_enhancer_prefix(&provider_enhancers)
                    .with_guard_prefix(&self.global_websocket_guards)
                    .with_interceptor_prefix(&self.global_websocket_interceptors)
                    .with_execution_pipeline_prefix(
                        &self.global_execution_guards,
                        &self.global_execution_interceptors,
                    )
                    .with_pipe_prefix(&self.global_websocket_pipes)
                    .with_filter_prefix(&self.global_websocket_filters)
                    .with_validation_prefix(
                        self.global_pipeline.validation_enabled,
                        self.global_pipeline.validation_options,
                    )
                    .with_default_module_ref(module_ref.clone())
            })
            .collect::<Vec<_>>();
        let message_patterns = message_patterns
            .into_iter()
            .map(|pattern| {
                pattern
                    .with_provider_enhancer_prefix(&provider_enhancers)
                    .with_guard_prefix(&self.global_transport_guards)
                    .with_interceptor_prefix(&self.global_transport_interceptors)
                    .with_execution_pipeline_prefix(
                        &self.global_execution_guards,
                        &self.global_execution_interceptors,
                    )
                    .with_pipe_prefix(&self.global_transport_pipes)
                    .with_filter_prefix(&self.global_transport_filters)
                    .with_validation_prefix(
                        self.global_pipeline.validation_enabled,
                        self.global_pipeline.validation_options,
                    )
                    .with_default_module_ref(module_ref.clone())
            })
            .collect::<Vec<_>>();
        let documented_routes = routes.clone();

        for (path, info) in self.openapi_routes {
            let document = OpenApiDocument::from_routes(info, &documented_routes);
            let route = openapi_json_route(path, document)?
                .with_pipeline_prefix(&effective_global_pipeline);
            let route = apply_global_prefix_to_route(
                route,
                self.global_prefix.as_deref(),
                &self.global_prefix_exclusions,
            )?;
            routes.push(route);
        }
        for ui in self.openapi_ui_routes {
            add_openapi_ui_routes(
                &mut routes,
                ui,
                &documented_routes,
                self.global_prefix.as_deref(),
                &self.global_prefix_exclusions,
                &effective_global_pipeline,
            )?;
        }

        #[cfg(feature = "security")]
        if let Some(cors_preflight) = &self.cors_preflight {
            add_cors_preflight_routes(&mut routes, cors_preflight, &effective_global_pipeline)?;
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
            global_filters: effective_global_pipeline.filters.clone(),
        })
    }
}

#[derive(Clone)]
struct OpenApiUiRoute {
    path: String,
    document_path: String,
    info: OpenApiInfo,
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

fn add_openapi_ui_routes(
    routes: &mut Vec<RouteDefinition>,
    ui: OpenApiUiRoute,
    documented_routes: &[RouteDefinition],
    global_prefix: Option<&str>,
    global_prefix_exclusions: &[MiddlewareRoute],
    pipeline: &PipelineComponents,
) -> Result<()> {
    let document = OpenApiDocument::from_routes(ui.info.clone(), documented_routes);
    let document_path = ui.document_path;
    let ui_path = ui.path;
    let json_route = openapi_json_route(document_path, document)?.with_pipeline_prefix(pipeline);
    let json_route =
        apply_global_prefix_to_route(json_route, global_prefix, global_prefix_exclusions)?;
    let public_document_path = json_route.path().to_string();
    let ui_route =
        openapi_ui_route(ui_path, public_document_path, ui.info)?.with_pipeline_prefix(pipeline);
    let ui_route = apply_global_prefix_to_route(ui_route, global_prefix, global_prefix_exclusions)?;
    routes.push(json_route);
    routes.push(ui_route);
    Ok(())
}

fn openapi_ui_route(
    path: String,
    document_path: String,
    info: OpenApiInfo,
) -> Result<RouteDefinition> {
    RouteDefinition::get(path, move |_| {
        let document_path = document_path.clone();
        let title = info.title.clone();
        async move { Ok(openapi_ui_response(&title, &document_path)) }
    })
    .map(RouteDefinition::hide_from_openapi)
}

fn openapi_ui_response(title: &str, document_path: &str) -> BootResponse {
    BootResponse::text_with_status(200, openapi_ui_html(title, document_path))
        .with_header("content-type", "text/html; charset=utf-8")
}

fn openapi_ui_html(title: &str, document_path: &str) -> String {
    format!(
        r##"<!doctype html>
<html lang="en">
<head>
  <meta charset="utf-8">
  <title>{title}</title>
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <link rel="stylesheet" href="https://unpkg.com/swagger-ui-dist@5/swagger-ui.css">
</head>
<body>
  <div id="swagger-ui"></div>
  <script src="https://unpkg.com/swagger-ui-dist@5/swagger-ui-bundle.js"></script>
  <script>
    window.ui = SwaggerUIBundle({{
      url: {document_path:?},
      dom_id: "#swagger-ui",
      deepLinking: true,
      presets: [SwaggerUIBundle.presets.apis],
      layout: "BaseLayout"
    }});
  </script>
</body>
</html>"##,
        title = escape_html(title),
        document_path = document_path
    )
}

fn escape_html(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
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
    exclusions: &[MiddlewareRoute],
) -> Result<Vec<RouteDefinition>> {
    routes
        .into_iter()
        .map(|route| apply_global_prefix_to_route(route, prefix, exclusions))
        .collect()
}

fn apply_global_prefix_to_route(
    route: RouteDefinition,
    prefix: Option<&str>,
    exclusions: &[MiddlewareRoute],
) -> Result<RouteDefinition> {
    let Some(prefix) = prefix else {
        return Ok(route);
    };
    if global_prefix_excludes(&route, exclusions) {
        return Ok(route);
    }
    route.with_path_prefix(prefix)
}

fn global_prefix_excludes(route: &RouteDefinition, exclusions: &[MiddlewareRoute]) -> bool {
    exclusions
        .iter()
        .any(|exclusion| exclusion.matches(route.method(), &[route.path().to_string()]))
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
        .filter(|route| route.method().matches(crate::HttpMethod::Get))
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
