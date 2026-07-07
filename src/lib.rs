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
mod percent;
mod pipeline;
mod provider;
mod routing;

#[cfg(feature = "macros")]
pub use a3s_boot_macros::{
    controller, delete, delete_json, get, get_json, head, injectable, options, patch, patch_json,
    post, post_json, put, put_json, sse,
};
#[cfg(feature = "axum")]
pub use adapters::AxumAdapter;
pub use app::{BootApplication, BootApplicationBuilder, RouteMatch};
pub use error::BootError;
pub use http::{BootRequest, BootResponse, HttpMethod, SseEvent, SseStream};
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
