//! Progressive Rust web framework primitives for A3S.
//!
//! `a3s-boot` is inspired by Nest.js, but keeps the Rust core explicit:
//! modules organize the graph, providers live in a typed container, controllers
//! group routes, request pipeline hooks are framework-neutral, and HTTP serving
//! is delegated to replaceable adapters.

use std::future::Future;
use std::net::SocketAddr;
use std::pin::Pin;

mod adapters;
mod app;
mod error;
mod http;
mod module;
mod pipeline;
mod provider;
mod routing;

#[cfg(feature = "axum")]
pub use adapters::AxumAdapter;
pub use app::{BootApplication, BootApplicationBuilder};
pub use error::BootError;
pub use http::{BootRequest, BootResponse, HttpMethod};
pub use module::Module;
pub use pipeline::{ExceptionFilter, ExecutionContext, Guard, Interceptor, Pipe};
pub use provider::{ModuleRef, ProviderDefinition, ProviderToken};
pub use routing::{ControllerDefinition, RouteDefinition, RouteHandler};

/// Result type used by A3S Boot.
pub type Result<T> = std::result::Result<T, BootError>;

/// Boxed future used by adapter traits.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// Adapter that turns a Boot application into a concrete HTTP server/router.
pub trait HttpAdapter {
    type Output;

    fn build(&self, app: BootApplication) -> Result<Self::Output>;

    fn serve(&self, app: BootApplication, addr: SocketAddr) -> BoxFuture<'static, Result<()>>;
}
