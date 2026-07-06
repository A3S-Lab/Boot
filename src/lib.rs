//! Modular web application primitives for A3S.
//!
//! `a3s-boot` is a Rust-first framework foundation inspired by Nest.js. The
//! core is adapter-oriented: modules register framework-neutral route
//! definitions, and an HTTP adapter turns the built application into a concrete
//! server such as Axum.

use std::collections::{BTreeMap, BTreeSet};
use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;
use std::sync::Arc;
use thiserror::Error;

#[cfg(feature = "axum")]
mod axum_adapter;

#[cfg(feature = "axum")]
pub use axum_adapter::AxumAdapter;

/// Result type used by A3S Boot.
pub type Result<T> = std::result::Result<T, BootError>;

/// Boxed future used by adapter traits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Errors returned while building or serving a Boot application.
#[derive(Debug, Error)]
pub enum BootError {
    #[error("module name cannot be empty")]
    EmptyModuleName,
    #[error("route path must start with '/': {0}")]
    InvalidRoutePath(String),
    #[error("adapter error: {0}")]
    Adapter(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// HTTP method understood by Boot route definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HttpMethod {
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

impl HttpMethod {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
        }
    }
}

/// Framework-neutral HTTP request passed to Boot route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootRequest {
    pub method: HttpMethod,
    pub path: String,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl BootRequest {
    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone()).map_err(|err| BootError::Adapter(err.to_string()))
    }
}

/// Framework-neutral HTTP response returned by Boot route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: Vec<u8>,
}

impl BootResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self::new(200, body.into()).with_header("content-type", "text/plain; charset=utf-8")
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }
}

/// Type-erased route handler used by adapters.
pub trait RouteHandler: Send + Sync + 'static {
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>>;
}

impl<F, Fut> RouteHandler for F
where
    F: Fn(BootRequest) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<BootResponse>> + Send + 'static,
{
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>> {
        Box::pin(self(request))
    }
}

/// A framework-neutral route definition.
#[derive(Clone)]
pub struct RouteDefinition {
    method: HttpMethod,
    path: String,
    handler: Arc<dyn RouteHandler>,
}

impl RouteDefinition {
    pub fn new<H>(method: HttpMethod, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        let path = path.into();
        validate_route_path(&path)?;
        Ok(Self {
            method,
            path,
            handler: Arc::new(handler),
        })
    }

    pub fn get<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Get, path, handler)
    }

    pub fn post<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Post, path, handler)
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn handler(&self) -> Arc<dyn RouteHandler> {
        Arc::clone(&self.handler)
    }
}

/// A module contributes routes and can import other modules.
///
/// This is the Rust equivalent of a Nest.js module boundary. Modules are
/// independent from the HTTP adapter; the same module graph can be served by
/// Axum today and a different adapter later.
pub trait Module: Send + Sync + 'static {
    /// Stable module name used for deduplication and diagnostics.
    fn name(&self) -> &'static str;

    /// Imported modules that should be registered before this module.
    fn imports(&self) -> Vec<Arc<dyn Module>> {
        Vec::new()
    }

    /// Framework-neutral routes contributed by this module.
    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        Ok(Vec::new())
    }
}

/// Adapter that turns a Boot application into a concrete HTTP server/router.
pub trait HttpAdapter {
    type Output;

    fn build(&self, app: BootApplication) -> Result<Self::Output>;

    fn serve(&self, app: BootApplication, addr: SocketAddr) -> BoxFuture<'static, Result<()>>;
}

/// Built application with a resolved module graph and framework-neutral routes.
#[derive(Clone)]
pub struct BootApplication {
    routes: Vec<RouteDefinition>,
    modules: Vec<String>,
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

    /// Build this application through a concrete HTTP adapter.
    pub fn into_adapter<A>(self, adapter: &A) -> Result<A::Output>
    where
        A: HttpAdapter,
    {
        adapter.build(self)
    }

    /// Serve this application through a concrete HTTP adapter.
    pub async fn serve_with<A>(self, adapter: &A, addr: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        adapter.serve(self, addr).await
    }
}

/// Builder for a [`BootApplication`].
#[derive(Default)]
pub struct BootApplicationBuilder {
    modules: Vec<Arc<dyn Module>>,
    routes: Vec<RouteDefinition>,
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

    /// Resolve module imports and build the application.
    pub fn build(self) -> Result<BootApplication> {
        let mut seen = BTreeSet::new();
        let mut modules = Vec::new();
        let mut routes = self.routes;

        for module in self.modules {
            register_module(module, &mut seen, &mut modules, &mut routes)?;
        }

        Ok(BootApplication { routes, modules })
    }
}

fn register_module(
    module: Arc<dyn Module>,
    seen: &mut BTreeSet<String>,
    modules: &mut Vec<String>,
    routes: &mut Vec<RouteDefinition>,
) -> Result<()> {
    let name = module.name();
    if name.trim().is_empty() {
        return Err(BootError::EmptyModuleName);
    }
    if !seen.insert(name.to_string()) {
        return Ok(());
    }

    for imported in module.imports() {
        register_module(imported, seen, modules, routes)?;
    }

    routes.extend(module.routes()?);
    modules.push(name.to_string());
    Ok(())
}

fn validate_route_path(path: &str) -> Result<()> {
    if path.starts_with('/') {
        Ok(())
    } else {
        Err(BootError::InvalidRoutePath(path.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug)]
    struct HealthModule;

    impl Module for HealthModule {
        fn name(&self) -> &'static str {
            "health"
        }

        fn routes(&self) -> Result<Vec<RouteDefinition>> {
            Ok(vec![RouteDefinition::get("/health", |_| async {
                Ok(BootResponse::text("ok"))
            })?])
        }
    }

    #[derive(Debug)]
    struct AppModule;

    impl Module for AppModule {
        fn name(&self) -> &'static str {
            "app"
        }

        fn imports(&self) -> Vec<Arc<dyn Module>> {
            vec![Arc::new(HealthModule)]
        }

        fn routes(&self) -> Result<Vec<RouteDefinition>> {
            Ok(vec![RouteDefinition::get("/", |_| async {
                Ok(BootResponse::text("hello"))
            })?])
        }
    }

    #[test]
    fn registers_imports_before_parent_modules() {
        let app = BootApplication::builder()
            .import(AppModule)
            .build()
            .unwrap();

        assert_eq!(app.module_names(), ["health", "app"]);
        assert_eq!(app.routes().len(), 2);
    }

    #[test]
    fn deduplicates_imported_modules_by_name() {
        let health = Arc::new(HealthModule);
        let app = BootApplication::builder()
            .import_arc(health.clone())
            .import_arc(health)
            .build()
            .unwrap();

        assert_eq!(app.module_names(), ["health"]);
        assert_eq!(app.routes().len(), 1);
    }

    #[test]
    fn rejects_empty_module_names() {
        struct EmptyModule;

        impl Module for EmptyModule {
            fn name(&self) -> &'static str {
                ""
            }
        }

        let result = BootApplication::builder().import(EmptyModule).build();

        assert!(matches!(result, Err(BootError::EmptyModuleName)));
    }

    #[test]
    fn rejects_relative_route_paths() {
        let result = RouteDefinition::get("health", |_| async { Ok(BootResponse::text("ok")) });

        assert!(matches!(result, Err(BootError::InvalidRoutePath(_))));
    }
}
