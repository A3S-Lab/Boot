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
#[cfg(feature = "cache")]
mod cache;
#[cfg(feature = "config")]
mod config;
mod error;
mod http;
mod module;
mod openapi;
mod percent;
mod pipeline;
mod provider;
mod routing;
#[cfg(feature = "schedule")]
mod schedule;
mod transport;
mod validation;
mod websocket;

#[cfg(feature = "macros")]
pub use a3s_boot_macros::{
    bearer_auth, body, controller, delete, delete_json, event_pattern, get, get_json, head, header,
    headers, hide_from_openapi, injectable, message_controller, message_pattern, operation,
    options, param, params, patch, patch_json, post, post_json, put, put_json, query, request,
    request_body, response, skip_validation, sse, subscribe_message, tag, validate,
    websocket_gateway,
};
#[cfg(feature = "axum")]
pub use adapters::AxumAdapter;
pub use app::{BootApplication, BootApplicationBuilder, RouteMatch};
#[cfg(feature = "cache")]
pub use cache::{Cache, CacheModule, CacheOptions, CacheStore, InMemoryCacheStore};
#[cfg(feature = "config")]
pub use config::{
    acl_document_to_json, parse_acl_config, parse_validated_acl_config, ConfigModule,
};
pub use error::BootError;
pub use http::{BootRequest, BootResponse, HttpMethod, SseEvent, SseStream};
pub use module::{DynamicModule, Module};
pub use openapi::{
    openapi_schema_name, OpenApiComponents, OpenApiDocument, OpenApiInfo, OpenApiMediaType,
    OpenApiOperation, OpenApiParameter, OpenApiParameterLocation, OpenApiPathItem,
    OpenApiRequestBody, OpenApiResponse, OpenApiRouteMetadata, OpenApiSchema,
    OpenApiSecurityRequirement, OpenApiTag,
};
pub use pipeline::{
    ExceptionFilter, ExecutionContext, Guard, Interceptor, Middleware, MiddlewareOutcome, Pipe,
};
pub use provider::{ModuleRef, ProviderDefinition, ProviderToken};
pub use routing::{ControllerDefinition, RouteDefinition, RouteHandler};
#[cfg(feature = "schedule")]
pub use schedule::{
    InProcessScheduler, ScheduleContext, ScheduleModule, ScheduleTrigger, ScheduledJob,
    ScheduledJobError, ScheduledJobInfo, ScheduledTask, Scheduler, SchedulerBackend,
};
pub use transport::{
    InProcessTransport, InProcessTransportClient, IntoTransportReply, MessagePatternDefinition,
    MessagePatternKind, MessageTransport, TransportContext, TransportGuard, TransportInterceptor,
    TransportMessage, TransportPipe, TransportReply,
};
pub use validation::Validate;
pub(crate) use validation::{
    body_validator, params_validator, query_validator, validate_value, RequestValidator,
};
pub use websocket::{
    IntoWebSocketReply, WebSocketConnection, WebSocketContext, WebSocketGatewayConnection,
    WebSocketGatewayDefinition, WebSocketGuard, WebSocketInterceptor, WebSocketMessage,
    WebSocketPipe,
};

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
