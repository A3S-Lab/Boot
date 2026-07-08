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
#[cfg(feature = "compression")]
mod compression;
#[cfg(feature = "config")]
mod config;
mod discovery;
mod error;
#[cfg(feature = "events")]
mod events;
#[cfg(feature = "file-upload")]
mod file_upload;
#[cfg(feature = "health")]
mod health;
mod http;
#[cfg(feature = "logging")]
mod logging;
mod module;
mod openapi;
mod percent;
mod pipeline;
mod provider;
#[cfg(feature = "queue")]
mod queue;
mod routing;
#[cfg(feature = "schedule")]
mod schedule;
#[cfg(feature = "security")]
mod security;
mod serialization;
#[cfg(feature = "session")]
mod session;
mod testing;
mod transport;
mod validation;
mod versioning;
mod websocket;

#[cfg(feature = "macros")]
pub use a3s_boot_macros::{
    all, bearer_auth, body, controller, delete, delete_json, event_pattern, extract, get, get_json,
    head, header, headers, hide_from_openapi, host, host_param, http_code, injectable, ip,
    message_controller, message_pattern, metadata, operation, options, param, params, patch,
    patch_json, post, post_json, put, put_json, query, redirect, request, request_body, response,
    serialize, skip_validation, sse, subscribe_message, tag, use_filter, use_guard,
    use_interceptor, use_pipe, validate, version, version_neutral, versions, websocket_gateway,
};
#[cfg(all(feature = "macros", feature = "schedule"))]
pub use a3s_boot_macros::{cron, interval, schedule, timeout};
#[cfg(all(feature = "macros", feature = "events"))]
pub use a3s_boot_macros::{event_listener, on_event};
#[cfg(feature = "axum")]
pub use adapters::AxumAdapter;
pub use app::{
    BootApplication, BootApplicationBuilder, BootApplicationContext, BootApplicationHandle,
    BootFactory, BootMicroservice, RouteMatch,
};
#[cfg(feature = "cache")]
pub use cache::{Cache, CacheModule, CacheOptions, CacheStore, InMemoryCacheStore};
#[cfg(feature = "compression")]
pub use compression::{CompressionInterceptor, CompressionOptions};
#[cfg(feature = "config")]
pub use config::{
    acl_document_to_json, parse_acl_config, parse_validated_acl_config, ConfigModule,
};
pub use discovery::{
    DiscoveredGateway, DiscoveredMessagePattern, DiscoveredModule, DiscoveredRoute,
    DiscoveryService, Reflector,
};
pub use error::{BootError, BootErrorKind};
#[cfg(feature = "events")]
pub use events::{
    EventContext, EventEmitter, EventEnvelope, EventListener, EventListenerDefinition, EventModule,
};
#[cfg(feature = "file-upload")]
pub use file_upload::{MultipartField, MultipartForm, MultipartOptions, UploadedFile};
#[cfg(feature = "health")]
pub use health::{
    HealthCheckService, HealthIndicator, HealthIndicatorResult, HealthModule, HealthReport,
    HealthStatus,
};
pub use http::{
    extract_request_value, transform_request_value, BootRequest, BootResponse, CookieOptions,
    CookieSameSite, HttpMethod, RequestExtractor, RequestValuePipe, SseEvent, SseStream,
};
#[cfg(feature = "logging")]
pub use logging::{
    InMemoryLogSink, LogFields, LogLevel, LogRecord, LogSink, Logger, LoggingModule, NoopLogSink,
    RequestLoggingInterceptor, RequestLoggingMiddleware,
};
pub use module::{DynamicModule, Module};
pub use openapi::{
    openapi_schema_name, OpenApiComponents, OpenApiDocument, OpenApiInfo, OpenApiMediaType,
    OpenApiOperation, OpenApiParameter, OpenApiParameterLocation, OpenApiPathItem,
    OpenApiRequestBody, OpenApiResponse, OpenApiRouteMetadata, OpenApiSchema,
    OpenApiSecurityRequirement, OpenApiTag,
};
pub use pipeline::{
    catch_errors, CatchFilter, ExceptionFilter, ExecutionContext, ExecutionInterceptor,
    ExecutionProtocol, ExecutionTransportKind, Guard, Interceptor, Middleware, MiddlewareOutcome,
    Pipe, TransportExecutionContext, WebSocketExecutionContext,
};
pub use provider::{
    FromModuleRef, ModuleRef, ProviderDefinition, ProviderOnApplicationBootstrap,
    ProviderOnApplicationShutdown, ProviderOnModuleInit, ProviderScope, ProviderToken,
};
#[cfg(feature = "queue")]
pub use queue::{
    InProcessQueueBackend, Queue, QueueBackend, QueueContext, QueueJob, QueueJobFailure,
    QueueJobInfo, QueueJobReceipt, QueueJobState, QueueModule, QueueOptions, QueueProcessor,
    QueueStats,
};
pub use routing::{ControllerDefinition, RouteDefinition, RouteHandler};
#[cfg(feature = "schedule")]
pub use schedule::{
    InProcessScheduler, ScheduleContext, ScheduleModule, ScheduleTrigger, ScheduledJob,
    ScheduledJobError, ScheduledJobInfo, ScheduledTask, Scheduler, SchedulerBackend,
};
#[cfg(feature = "security")]
pub use security::{
    CorsMiddleware, CorsOptions, CorsPreflightRoute, CorsResponseInterceptor, CsrfGuard,
    CsrfOptions, RateLimitGuard, RateLimitOptions, SecurityHeadersInterceptor,
    SecurityHeadersOptions,
};
pub use serialization::{SerializationInterceptor, SerializationOptions};
#[cfg(feature = "session")]
pub use session::{
    InMemorySessionStore, SessionCookieInterceptor, SessionCookieSameSite, SessionManager,
    SessionMiddleware, SessionModule, SessionOptions, SessionStore,
};
pub use testing::{TestingModule, TestingModuleBuilder};
pub use transport::{
    InProcessTransport, InProcessTransportClient, IntoTransportReply, MessagePatternDefinition,
    MessagePatternKind, MessageTransport, TransportContext, TransportGuard, TransportInterceptor,
    TransportMessage, TransportPipe, TransportReply,
};
#[cfg(feature = "tcp-transport")]
pub use transport::{TcpTransport, TcpTransportClient, TcpTransportOptions};
pub use validation::Validate;
pub(crate) use validation::{
    body_validator, params_validator, query_validator, validate_value, RequestValidator,
};
pub use versioning::{ApiVersioning, ApiVersioningStrategy, RouteVersioning};
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
