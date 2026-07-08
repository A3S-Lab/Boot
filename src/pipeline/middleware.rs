use crate::routing::path::{match_path_shape, validate_route_path};
use crate::{BootError, BootRequest, BootResponse, BoxFuture, HttpMethod, Result, RouteDefinition};
use std::future::Future;
use std::sync::Arc;

/// Result of running a middleware.
pub enum MiddlewareOutcome {
    Continue(BootRequest),
    Respond(BootResponse),
}

impl MiddlewareOutcome {
    pub fn next(request: BootRequest) -> Self {
        Self::Continue(request)
    }

    pub fn response(response: BootResponse) -> Self {
        Self::Respond(response)
    }
}

/// Request middleware that runs before guards, interceptors, pipes, and handlers.
pub trait Middleware: Send + Sync + 'static {
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>>;
}

impl<F, Fut> Middleware for F
where
    F: Fn(BootRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<MiddlewareOutcome>> + Send + 'static,
{
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        Box::pin(self(request))
    }
}

/// Route selector used by [`MiddlewareConsumer`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MiddlewareRoute {
    method: Option<HttpMethod>,
    path: String,
}

impl MiddlewareRoute {
    pub fn any(path: impl Into<String>) -> Result<Self> {
        Self::new(None, path)
    }

    pub fn all(path: impl Into<String>) -> Result<Self> {
        Self::any(path)
    }

    pub fn method(method: HttpMethod, path: impl Into<String>) -> Result<Self> {
        Self::new(Some(method), path)
    }

    pub fn get(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Get, path)
    }

    pub fn post(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Post, path)
    }

    pub fn put(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Put, path)
    }

    pub fn patch(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Patch, path)
    }

    pub fn delete(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Delete, path)
    }

    pub fn options(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Options, path)
    }

    pub fn head(path: impl Into<String>) -> Result<Self> {
        Self::method(HttpMethod::Head, path)
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn http_method(&self) -> Option<HttpMethod> {
        self.method
    }

    fn new(method: Option<HttpMethod>, path: impl Into<String>) -> Result<Self> {
        let path = path.into();
        validate_route_path(&path)?;
        Ok(Self {
            method: method.filter(|method| !method.is_wildcard()),
            path,
        })
    }

    fn matches(&self, route_method: HttpMethod, path_candidates: &[String]) -> bool {
        self.matches_method(route_method)
            && path_candidates
                .iter()
                .any(|path| match_path_shape(&self.path, path))
    }

    fn matches_method(&self, route_method: HttpMethod) -> bool {
        match self.method {
            Some(method) => method == route_method || route_method.is_wildcard(),
            None => true,
        }
    }
}

/// Nest-style route-scoped middleware configuration.
#[derive(Clone, Default)]
pub struct MiddlewareConsumer {
    entries: Vec<MiddlewareConsumerEntry>,
}

impl MiddlewareConsumer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn apply<M>(&mut self, middleware: M) -> MiddlewareConsumerBuilder<'_>
    where
        M: Middleware,
    {
        self.apply_arc(Arc::new(middleware))
    }

    pub fn apply_arc(&mut self, middleware: Arc<dyn Middleware>) -> MiddlewareConsumerBuilder<'_> {
        MiddlewareConsumerBuilder {
            consumer: self,
            middleware: vec![middleware],
            excluded: Vec::new(),
        }
    }

    pub fn extend(&mut self, other: MiddlewareConsumer) {
        self.entries.extend(other.entries);
    }

    pub(crate) fn apply_to_route(
        &self,
        route: RouteDefinition,
        path_candidates: &[String],
    ) -> RouteDefinition {
        let mut middleware = Vec::new();
        for entry in &self.entries {
            if entry.matches(route.method(), path_candidates) {
                middleware.extend(entry.middleware.iter().cloned());
            }
        }

        if middleware.is_empty() {
            route
        } else {
            route.with_middleware_prefix_arc(&middleware)
        }
    }
}

pub struct MiddlewareConsumerBuilder<'a> {
    consumer: &'a mut MiddlewareConsumer,
    middleware: Vec<Arc<dyn Middleware>>,
    excluded: Vec<MiddlewareRoute>,
}

impl MiddlewareConsumerBuilder<'_> {
    pub fn and<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.middleware.push(Arc::new(middleware));
        self
    }

    pub fn and_arc(mut self, middleware: Arc<dyn Middleware>) -> Self {
        self.middleware.push(middleware);
        self
    }

    pub fn exclude<I>(mut self, routes: I) -> Self
    where
        I: IntoIterator<Item = MiddlewareRoute>,
    {
        self.excluded.extend(routes);
        self
    }

    pub fn exclude_route(mut self, route: MiddlewareRoute) -> Self {
        self.excluded.push(route);
        self
    }

    pub fn for_route(self, route: MiddlewareRoute) -> Result<()> {
        self.for_routes([route])
    }

    pub fn for_routes<I>(self, routes: I) -> Result<()>
    where
        I: IntoIterator<Item = MiddlewareRoute>,
    {
        let routes = routes.into_iter().collect::<Vec<_>>();
        if routes.is_empty() {
            return Err(BootError::BadRequest(
                "middleware consumer requires at least one route".to_string(),
            ));
        }
        self.register(routes);
        Ok(())
    }

    pub fn for_all_routes(self) -> Result<()> {
        self.register(Vec::new());
        Ok(())
    }

    fn register(self, routes: Vec<MiddlewareRoute>) {
        self.consumer.entries.push(MiddlewareConsumerEntry {
            middleware: self.middleware,
            routes,
            excluded: self.excluded,
        });
    }
}

#[derive(Clone)]
struct MiddlewareConsumerEntry {
    middleware: Vec<Arc<dyn Middleware>>,
    routes: Vec<MiddlewareRoute>,
    excluded: Vec<MiddlewareRoute>,
}

impl MiddlewareConsumerEntry {
    fn matches(&self, route_method: HttpMethod, path_candidates: &[String]) -> bool {
        let included = self.routes.is_empty()
            || self
                .routes
                .iter()
                .any(|route| route.matches(route_method, path_candidates));
        let excluded = self
            .excluded
            .iter()
            .any(|route| route.matches(route_method, path_candidates));

        included && !excluded
    }
}
