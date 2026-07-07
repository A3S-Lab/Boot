use super::builder::BootApplicationBuilder;
use crate::{
    BootError, BootRequest, BootResponse, HttpAdapter, HttpMethod, MessagePatternDefinition,
    MessageTransport, Module, ModuleRef, OpenApiDocument, OpenApiInfo, Result, RouteDefinition,
    TransportMessage, TransportReply, WebSocketGatewayDefinition,
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

    /// WebSocket gateways exposed by the application.
    pub fn gateways(&self) -> &[WebSocketGatewayDefinition] {
        &self.gateways
    }

    /// Microservice message patterns exposed by the application.
    pub fn message_patterns(&self) -> &[MessagePatternDefinition] {
        &self.message_patterns
    }

    /// Generate an OpenAPI 3 document from the resolved route table.
    pub fn openapi(&self, info: OpenApiInfo) -> OpenApiDocument {
        OpenApiDocument::from_routes(info, &self.routes)
    }

    /// Route registered for a method and the most specific route shape matching a path.
    pub fn route_for(&self, method: HttpMethod, path: &str) -> Option<&RouteDefinition> {
        let best_specificity = best_path_specificity(&self.routes, path)?;
        self.routes.iter().find(|route| {
            route.matches_path_shape(path)
                && route.path_specificity().as_slice() == best_specificity.as_slice()
                && route.method() == method
        })
    }

    /// Route and decoded path parameters for a method and path, when one is registered.
    pub fn route_match(&self, method: HttpMethod, path: &str) -> Result<Option<RouteMatch<'_>>> {
        let Some(route) = self.route_for(method, path) else {
            return Ok(None);
        };
        let Some(params) = route.path_params(path)? else {
            return Ok(None);
        };
        Ok(Some(RouteMatch { route, params }))
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
        let Some(best_specificity) = best_path_specificity(&self.routes, path) else {
            return Vec::new();
        };

        let mut methods = Vec::new();
        for route in matching_routes_with_specificity(&self.routes, path, &best_specificity) {
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
        let path = request.path.clone();
        let Some(best_specificity) = best_path_specificity(&self.routes, &path) else {
            return Err(BootError::NotFound(format!(
                "{} {}",
                request.method.as_str(),
                request.path
            )));
        };

        if let Some(route) =
            matching_route_with_specificity(&self.routes, &path, &best_specificity, |route| {
                route.method() == request.method
            })
        {
            return route.call(request).await;
        }

        if let Some(route) =
            matching_route_with_specificity(&self.routes, &path, &best_specificity, |_| true)
        {
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
}

fn best_path_specificity(routes: &[RouteDefinition], path: &str) -> Option<Vec<u8>> {
    let mut best_specificity = None;

    for route in routes {
        if route.matches_path_shape(path) {
            let specificity = route.path_specificity();
            if best_specificity
                .as_ref()
                .map(|best| specificity > *best)
                .unwrap_or(true)
            {
                best_specificity = Some(specificity);
            }
        }
    }

    best_specificity
}

fn matching_routes_with_specificity<'a>(
    routes: &'a [RouteDefinition],
    path: &'a str,
    specificity: &'a [u8],
) -> impl Iterator<Item = &'a RouteDefinition> + 'a {
    routes.iter().filter(move |route| {
        route.matches_path_shape(path) && route.path_specificity().as_slice() == specificity
    })
}

fn matching_route_with_specificity<'a, P>(
    routes: &'a [RouteDefinition],
    path: &'a str,
    specificity: &'a [u8],
    mut predicate: P,
) -> Option<&'a RouteDefinition>
where
    P: FnMut(&RouteDefinition) -> bool,
{
    matching_routes_with_specificity(routes, path, specificity).find(|route| predicate(route))
}
