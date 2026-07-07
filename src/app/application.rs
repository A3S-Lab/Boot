use super::builder::BootApplicationBuilder;
use crate::versioning::ApiVersionCandidate;
use crate::{
    ApiVersioning, BootError, BootRequest, BootResponse, DiscoveryService, HttpAdapter, HttpMethod,
    MessagePatternDefinition, MessageTransport, Module, ModuleRef, OpenApiDocument, OpenApiInfo,
    Reflector, Result, RouteDefinition, TransportMessage, TransportReply,
    WebSocketGatewayDefinition,
};
use std::collections::BTreeMap;
use std::fmt;
use std::net::SocketAddr;
use std::sync::Arc;

#[derive(Clone)]
pub(crate) struct ModuleInstance {
    pub module: Arc<dyn Module>,
    pub module_ref: ModuleRef,
}

/// Resolved route and decoded path parameters for a method/path lookup.
pub struct RouteMatch<'a> {
    route: &'a RouteDefinition,
    params: BTreeMap<String, String>,
}

impl<'a> RouteMatch<'a> {
    pub fn route(&self) -> &'a RouteDefinition {
        self.route
    }

    pub fn params(&self) -> &BTreeMap<String, String> {
        &self.params
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn into_params(self) -> BTreeMap<String, String> {
        self.params
    }
}

impl fmt::Debug for RouteMatch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RouteMatch")
            .field("method", &self.route.method())
            .field("path", &self.route.path())
            .field("params", &self.params)
            .finish()
    }
}

/// Built application with a resolved module graph and framework-neutral routes.
#[derive(Clone)]
pub struct BootApplication {
    pub(crate) routes: Vec<RouteDefinition>,
    pub(crate) gateways: Vec<WebSocketGatewayDefinition>,
    pub(crate) message_patterns: Vec<MessagePatternDefinition>,
    pub(crate) modules: Vec<String>,
    pub(crate) module_ref: ModuleRef,
    pub(crate) module_instances: Vec<ModuleInstance>,
    pub(crate) api_versioning: Option<ApiVersioning>,
}

impl BootApplication {
    /// Create an application builder.
    pub fn builder() -> BootApplicationBuilder {
        BootApplicationBuilder::new()
    }

    /// Names of modules included in the application, in registration order.
    pub fn module_names(&self) -> &[String] {
        &self.modules
    }

    /// Routes exposed by the application.
    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }

    /// API versioning configuration used for HTTP route matching, when enabled.
    pub fn api_versioning(&self) -> Option<&ApiVersioning> {
        self.api_versioning.as_ref()
    }

    /// WebSocket gateways exposed by the application.
    pub fn gateways(&self) -> &[WebSocketGatewayDefinition] {
        &self.gateways
    }

    /// Microservice message patterns exposed by the application.
    pub fn message_patterns(&self) -> &[MessagePatternDefinition] {
        &self.message_patterns
    }

    /// Build a discovery snapshot for modules, routes, gateways, and message patterns.
    pub fn discovery(&self) -> Result<DiscoveryService> {
        DiscoveryService::from_app(self)
    }

    /// Build a discovery snapshot and metadata reflector.
    pub fn reflector(&self) -> Result<Reflector> {
        Reflector::from_app(self)
    }

    /// Generate an OpenAPI 3 document from the resolved route table.
    pub fn openapi(&self, info: OpenApiInfo) -> OpenApiDocument {
        OpenApiDocument::from_routes(info, &self.routes)
    }

    /// Route registered for a method and the most specific route shape matching a path.
    pub fn route_for(&self, method: HttpMethod, path: &str) -> Option<&RouteDefinition> {
        let candidates = self.path_candidates(path);
        self.route_for_candidates(method, &candidates)
            .map(|matched| matched.route)
    }

    /// Route and decoded path parameters for a method and path, when one is registered.
    pub fn route_match(&self, method: HttpMethod, path: &str) -> Result<Option<RouteMatch<'_>>> {
        let candidates = self.path_candidates(path);
        let Some(matched) = self.route_for_candidates(method, &candidates) else {
            return Ok(None);
        };
        let Some(params) = matched.route.path_params(&matched.path)? else {
            return Ok(None);
        };
        Ok(Some(RouteMatch {
            route: matched.route,
            params,
        }))
    }

    pub fn gateway_for(&self, path: &str) -> Option<&WebSocketGatewayDefinition> {
        self.gateways
            .iter()
            .find(|gateway| gateway.matches_path(path))
    }

    pub fn message_pattern_for(&self, pattern: &str) -> Option<&MessagePatternDefinition> {
        self.message_patterns
            .iter()
            .find(|definition| definition.pattern() == pattern)
    }

    /// HTTP methods registered for the most specific route shape matching a path.
    pub fn allowed_methods(&self, path: &str) -> Vec<HttpMethod> {
        let candidates = self.path_candidates(path);
        let Some((candidate_index, best_specificity)) =
            best_path_specificity(&self.routes, &candidates, self.api_versioning.as_ref())
        else {
            return Vec::new();
        };
        let candidate = &candidates[candidate_index];

        let mut methods = Vec::new();
        for route in &self.routes {
            if !route_matches_candidate(
                route,
                candidate,
                &best_specificity,
                self.api_versioning.as_ref(),
            ) {
                continue;
            }

            if !methods.contains(&route.method()) {
                methods.push(route.method());
            }
        }
        methods
    }

    /// Comma-separated HTTP methods for an Allow header on the most specific matching path.
    pub fn allowed_methods_header(&self, path: &str) -> Option<String> {
        let methods = self.allowed_methods(path);
        if methods.is_empty() {
            return None;
        }

        Some(
            methods
                .into_iter()
                .map(|method| method.as_str())
                .collect::<Vec<_>>()
                .join(","),
        )
    }

    /// Provider container available to hosts and controllers.
    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    /// Resolve a typed provider from the application container.
    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get::<T>()
    }

    /// Resolve a named provider from the application container.
    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_named::<T>(token)
    }

    /// Resolve a typed provider when it is present.
    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_optional::<T>()
    }

    /// Resolve a named provider when it is present.
    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get_optional_named::<T>(token)
    }

    /// Dispatch a framework-neutral request through the resolved route table.
    pub async fn call(&self, request: BootRequest) -> Result<BootResponse> {
        let candidates = self.request_candidates(&request);
        let Some((candidate_index, best_specificity)) =
            best_path_specificity(&self.routes, &candidates, self.api_versioning.as_ref())
        else {
            return Err(BootError::NotFound(format!(
                "{} {}",
                request.method.as_str(),
                request.path
            )));
        };
        let candidate = &candidates[candidate_index];

        if let Some(route) = matching_route_with_specificity(
            &self.routes,
            candidate,
            &best_specificity,
            self.api_versioning.as_ref(),
            |route| route.method() == request.method,
        ) {
            let request = request.with_matched_path(candidate.path.clone());
            return route.call(request).await;
        }

        if let Some(route) = matching_route_with_specificity(
            &self.routes,
            candidate,
            &best_specificity,
            self.api_versioning.as_ref(),
            |_| true,
        ) {
            return route.call(request).await;
        }

        Err(BootError::NotFound(format!(
            "{} {}",
            request.method.as_str(),
            request.path
        )))
    }

    /// Dispatch a request and convert unhandled errors into Boot HTTP responses.
    pub async fn handle(&self, request: BootRequest) -> BootResponse {
        match self.call(request).await {
            Ok(response) => response,
            Err(error) => BootResponse::from_error(&error),
        }
    }

    /// Dispatch a microservice transport message through the resolved pattern table.
    pub async fn dispatch_message(
        &self,
        message: TransportMessage,
    ) -> Result<Option<TransportReply>> {
        let pattern = message.pattern.clone();
        let Some(definition) = self.message_pattern_for(&pattern) else {
            return Err(BootError::NotFound(format!("message pattern {pattern}")));
        };
        definition.dispatch(message).await
    }

    /// Dispatch an event-only transport message and ignore any handler reply.
    pub async fn emit_message(&self, message: TransportMessage) -> Result<()> {
        self.dispatch_message(message).await.map(|_| ())
    }

    /// Run async startup hooks before serving, when the host needs them.
    pub async fn bootstrap(&self) -> Result<()> {
        for instance in &self.module_instances {
            instance
                .module
                .on_application_bootstrap(instance.module_ref.clone())
                .await?;
        }
        Ok(())
    }

    /// Run async shutdown hooks in reverse registration order.
    pub async fn shutdown(&self) -> Result<()> {
        for instance in self.module_instances.iter().rev() {
            instance
                .module
                .on_application_shutdown(instance.module_ref.clone())
                .await?;
        }
        Ok(())
    }

    /// Build this application through a concrete HTTP adapter.
    pub fn into_adapter<A>(self, adapter: &A) -> Result<A::Output>
    where
        A: HttpAdapter,
    {
        adapter.build(self)
    }

    /// Build this application through a concrete message transport.
    pub fn into_message_transport<T>(self, transport: &T) -> Result<T::Output>
    where
        T: MessageTransport,
    {
        transport.build(self)
    }

    /// Serve this application through a concrete HTTP adapter.
    pub async fn serve_with<A>(self, adapter: &A, addr: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        let app_for_shutdown = self.clone();
        if let Err(error) = self.bootstrap().await {
            let _ = app_for_shutdown.shutdown().await;
            return Err(error);
        }
        let serve_result = adapter.serve(self, addr).await;
        let shutdown_result = app_for_shutdown.shutdown().await;

        match (serve_result, shutdown_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    fn request_candidates(&self, request: &BootRequest) -> Vec<ApiVersionCandidate> {
        match self.api_versioning.as_ref() {
            Some(versioning) => versioning.request_candidates(request),
            None => vec![ApiVersionCandidate {
                path: request.path.clone(),
                version: None,
            }],
        }
    }

    fn path_candidates(&self, path: &str) -> Vec<ApiVersionCandidate> {
        match self.api_versioning.as_ref() {
            Some(versioning) => versioning.path_candidates(path),
            None => vec![ApiVersionCandidate {
                path: path.to_string(),
                version: None,
            }],
        }
    }

    fn route_for_candidates<'a>(
        &'a self,
        method: HttpMethod,
        candidates: &[ApiVersionCandidate],
    ) -> Option<MatchedRoute<'a>> {
        let (candidate_index, best_specificity) =
            best_path_specificity(&self.routes, candidates, self.api_versioning.as_ref())?;
        let candidate = &candidates[candidate_index];
        let route = matching_route_with_specificity(
            &self.routes,
            candidate,
            &best_specificity,
            self.api_versioning.as_ref(),
            |route| route.method() == method,
        )?;

        Some(MatchedRoute {
            route,
            path: candidate.path.clone(),
        })
    }
}

struct MatchedRoute<'a> {
    route: &'a RouteDefinition,
    path: String,
}

fn best_path_specificity(
    routes: &[RouteDefinition],
    candidates: &[ApiVersionCandidate],
    versioning: Option<&ApiVersioning>,
) -> Option<(usize, Vec<u8>)> {
    let mut best = None;

    for (candidate_index, candidate) in candidates.iter().enumerate() {
        for route in routes {
            if route_matches_path_and_version(route, candidate, versioning) {
                let specificity = route.path_specificity();
                if best
                    .as_ref()
                    .map(|(_, best_specificity)| specificity > *best_specificity)
                    .unwrap_or(true)
                {
                    best = Some((candidate_index, specificity));
                }
            }
        }
    }

    best
}

fn matching_route_with_specificity<'a, P>(
    routes: &'a [RouteDefinition],
    candidate: &ApiVersionCandidate,
    specificity: &[u8],
    versioning: Option<&ApiVersioning>,
    mut predicate: P,
) -> Option<&'a RouteDefinition>
where
    P: FnMut(&RouteDefinition) -> bool,
{
    routes.iter().find(|route| {
        route_matches_candidate(route, candidate, specificity, versioning) && predicate(route)
    })
}

fn route_matches_candidate(
    route: &RouteDefinition,
    candidate: &ApiVersionCandidate,
    specificity: &[u8],
    versioning: Option<&ApiVersioning>,
) -> bool {
    route_matches_path_and_version(route, candidate, versioning)
        && route.path_specificity().as_slice() == specificity
}

fn route_matches_path_and_version(
    route: &RouteDefinition,
    candidate: &ApiVersionCandidate,
    versioning: Option<&ApiVersioning>,
) -> bool {
    if !route.matches_path_shape(&candidate.path) {
        return false;
    }

    match versioning {
        Some(versioning) => route
            .versioning()
            .matches(candidate.version.as_deref(), versioning.default_version()),
        None => true,
    }
}
