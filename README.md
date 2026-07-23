# A3S Boot

<p align="center">
  <strong>Progressive Rust Web Framework for A3S</strong>
</p>

<p align="center">
  <em>Build modular async services with typed providers, explicit pipelines, and replaceable protocol adapters</em>
</p>

<p align="center">
  <a href="#overview">Overview</a> •
  <a href="#features">Features</a> •
  <a href="#quick-start">Quick Start</a> •
  <a href="#application-model">Application Model</a> •
  <a href="#protocols">Protocols</a> •
  <a href="#architecture">Architecture</a> •
  <a href="#development">Development</a>
</p>

---

## Overview

**A3S Boot** is a modular async service framework for Rust, inspired by
[Nest.js](https://nestjs.com/). Modules organize an application, typed providers
supply dependencies, controllers expose routes, and a protocol-neutral pipeline
applies guards, interceptors, pipes, validation, and exception filters.

Boot is not an Axum wrapper. Requests, responses, routes, and execution contexts
belong to the framework core; Axum is the bundled default HTTP adapter. Rust
attribute macros generate ordinary Boot definitions at compile time rather than
relying on runtime decorator metadata.

### Basic usage

```rust,no_run
use a3s_boot::{
    AxumAdapter, BootApplication, BootResponse, ControllerDefinition, Module,
    ModuleRef, ProviderDefinition, Result,
};

#[derive(Debug)]
struct GreetingService;

impl GreetingService {
    fn hello(&self) -> &'static str {
        "Hello from A3S Boot"
    }
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(GreetingService)])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let greeting = module_ref.get::<GreetingService>()?;
        Ok(vec![ControllerDefinition::new("/")?.get("/", move |_| {
            let greeting = greeting.clone();
            async move { Ok(BootResponse::text(greeting.hello())) }
        })?])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = BootApplication::builder().import(AppModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
```

## Features

- **Application Modules**: Compose imports, providers, controllers, gateways,
  message controllers, exports, route prefixes, and lifecycle hooks
- **Typed Dependency Injection**: Resolve typed or named providers with
  singleton, request, and transient scopes
- **HTTP Controllers**: Define adapter-neutral routes, typed inputs, JSON or raw
  responses, cookies, redirects, files, views, and server-sent events
- **Execution Pipeline**: Apply middleware, guards, around interceptors, pipes,
  validation, and exception filters at global and local scopes
- **Compile-Time Macros**: Use Nest-style attributes for modules, providers,
  controllers, routes, extraction, validation, OpenAPI, and protocol handlers
- **OpenAPI**: Generate OpenAPI documents and serve a Swagger UI from route
  metadata, reusable components, and security schemes
- **WebSocket Gateways**: Handle subscriptions, connection lifecycle, rooms,
  broadcasts, and protocol-specific pipeline hooks
- **Microservices**: Dispatch request-response and event patterns through
  in-process or optional network transports
- **Application Lifecycles**: Bootstrap, shut down, lazy-load modules, or create
  provider-only application contexts and standalone microservices
- **Testing Support**: Compile testing modules and override providers or pipeline
  components without replacing application code

### Feature matrix

Default features are `axum`, `macros`, and `shutdown-hooks`. Other integrations
are opt-in.

| Area | Feature | Included capability |
| --- | --- | --- |
| HTTP runtime | `axum` | Axum HTTP and WebSocket adapter |
| Compile-time API | `macros` | Nest-style procedural attributes |
| Lifecycle | `shutdown-hooks` | SIGINT and SIGTERM shutdown handling |
| Configuration | `config` | ACL-backed typed configuration parsing |
| Authentication | `auth` | Strategy-backed authentication guards |
| Security | `security` | CORS, CSRF, local or provider-backed rate limiting, and security headers |
| Sessions | `session` | Session middleware and replaceable stores |
| Cache | `cache` | Cache abstraction, interceptor, and in-memory store |
| Database | `database` | Replaceable database facade and in-memory backend |
| Events | `events` | A3S Event-backed emitter and listeners |
| CQRS | `cqrs` | Command, query, and event buses |
| Queue | `queue` | A3S Lane-backed jobs, retries, priorities, and processors |
| Scheduling | `schedule` | Cron, interval, and timeout jobs |
| Observability | `logging`, `health` | Structured logging and health indicators |
| HTTP utilities | `http-client`, `compression` | Outbound HTTP and gzip responses |
| Channels | `ilink` | Tencent Weixin iLink QR login, polling, messaging, and lifecycle client |
| Content | `file-upload`, `static` | Multipart uploads and static files |
| Context | `request-context` | Task-local access to the current request |
| OpenAPI | `openapi-schemas` | `schemars`-based component schemas |
| Transports | `tcp-transport`, `redis-transport`, `nats-transport` | TCP, Redis, and NATS messaging |
| Transports | `mqtt-transport`, `rabbitmq-transport`, `kafka-transport` | MQTT, RabbitMQ, and Kafka messaging |
| Transports | `grpc-transport` | Unary gRPC messaging |

A feature exposes the corresponding framework integration; external transports
still require their broker or service to be available. Database, cache, session,
queue, and scheduler APIs are backend abstractions, and the bundled implementation
is not a claim of support for every production backend.

## Quick Start

### Installation

```toml
[dependencies]
a3s-boot = "0.1.3"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

For a core-only build without Axum, macros, or shutdown signal handling:

```toml
[dependencies]
a3s-boot = { version = "0.1.3", default-features = false }
```

Enable only the optional modules an application uses:

```toml
[dependencies]
a3s-boot = { version = "0.1.3", features = ["auth", "security", "openapi-schemas"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

### Attribute-based controllers

The default feature set includes compile-time macros. Providers and controllers
remain normal Rust types, while attributes generate their Boot registrations.

```rust,no_run
use std::sync::Arc;

use a3s_boot::{
    controller, get, injectable, module, param, AxumAdapter, BootFactory, Result,
};

#[injectable]
#[derive(Debug)]
struct GreetingService;

impl GreetingService {
    fn hello(&self, name: &str) -> String {
        format!("Hello, {name}")
    }
}

#[injectable]
#[derive(Debug)]
struct GreetingController {
    greeting: Arc<GreetingService>,
}

#[controller("/greetings")]
impl GreetingController {
    #[get("/{name}")]
    async fn greet(&self, #[param("name")] name: String) -> Result<String> {
        Ok(self.greeting.hello(&name))
    }
}

#[module(
    name = "app",
    providers = [GreetingService, GreetingController],
    controllers = [GreetingController],
)]
#[derive(Debug)]
struct AppModule;

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = BootFactory::create(AppModule)?;
    app.listen_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into())
        .await
}
```

The explicit builder API remains available for dynamic registration, adapters,
and applications that prefer not to use procedural macros.

## Application Model

### Modules and providers

A `Module` owns a feature boundary. It can import other modules, register and
export providers, expose HTTP controllers, attach WebSocket gateways and message
patterns, configure middleware, and participate in startup or shutdown.

Providers use typed or named tokens and support value, factory, async factory,
and alias definitions. `ModuleRef` resolves dependencies within module visibility
rules. `ProviderRef<T>` defers resolution for optional or circular graphs.
Provider scope can be singleton, request-scoped, or transient; request scope is
propagated through eager dependencies.

`DynamicModule` supports runtime module configuration, while `LazyModuleLoader`
loads isolated feature modules after startup. Application-wide pipeline providers
must be imported eagerly because loading them later would change already compiled
handlers.

### Factory and lifecycle

`BootFactory` is the managed entry point:

- `create` builds an HTTP-capable application
- `create_application_context` builds a provider-only worker
- `create_microservice` builds a standalone message service
- async variants support asynchronous provider factories

Modules and providers can observe initialization, bootstrap, destruction, and
application shutdown. Shutdown hooks can listen for SIGINT and SIGTERM when the
`shutdown-hooks` feature is enabled.

### Request pipeline

HTTP handlers run through deterministic middleware and pipeline stages:

```text
request → middleware → guards → interceptors → pipes → validation → handler
                              └──── exception filters on unrecovered errors ────┘
```

Around interceptors receive a `CallHandler`, so they can transform results,
short-circuit execution, recover errors, or replay the remaining pipeline for a
sequential retry. Retries have at-least-once semantics: provider state and
external side effects are not rolled back.

Equivalent WebSocket and transport hooks use the same execution model with
protocol-specific contexts and replies.

## HTTP and OpenAPI

Controllers support standard HTTP methods, host and path routing, query and body
DTOs, headers, cookies, client IP hints, custom extraction, redirects, response
passthrough, streaming files, server-sent events, and URI, header, or media-type
versioning.

Validation is explicit through `Validate` and optional `ValidationSchema`
implementations. Transform, whitelist, and reject-unknown-field policies can be
applied globally, per controller, or per handler.

OpenAPI metadata can be attached through builders or attributes. Boot generates
an OpenAPI document, supports reusable schemas and security schemes, and can
serve both JSON and a Swagger UI. Enable `openapi-schemas` to collect schemas
from `schemars::JsonSchema` types.

Optional HTTP modules add multipart uploads, static files, gzip compression,
views, sessions, security policies, request context, and an outbound HTTP client.

### Provider-backed rate limiting

The `security` feature keeps `use_global_rate_limit` process-local by default.
Applications that need one budget across multiple processes can implement the
public `RateLimitProvider` contract and register it with
`use_global_rate_limit_provider`. Each atomic acquisition receives a stable
policy identifier, a policy-scoped SHA-256 subject digest, and the configured
request limit and window. Selected header values and bearer credentials do not
cross the provider boundary in plaintext.

Every process using the same policy identifier must use identical limits and
windows. Provider errors reject guarded requests instead of bypassing the
limit. Boot deliberately does not select or bundle a distributed backend; the
built-in `InMemoryRateLimitProvider` does not share state between processes.
This boundary does not cover the separate streaming-disconnect, backpressure,
or graceful-drain work.

## Protocols

### WebSocket

`WebSocketGatewayDefinition` and `#[websocket_gateway]` define upgrade paths and
message subscriptions. Gateways support initialization, connection and disconnect
hooks, direct messages, rooms, broadcasts, typed payload extraction, validation,
and WebSocket-specific guards, interceptors, pipes, and filters. The bundled
Axum adapter performs real WebSocket upgrades.

### Microservices

Message controllers define request-response patterns and event-only patterns.
`InProcessTransport` is always available for tests, workers, and same-process
communication. Optional features add TCP, Redis, NATS, MQTT, RabbitMQ, Kafka,
and gRPC transports behind the common `MessageTransport` contract.

Transport implementations share typed payload handling, scoped providers,
validation, guards, interceptors, pipes, exception filters, and client APIs.
Protocol delivery and durability semantics still depend on the selected backend.

### Weixin iLink

The optional `ilink` feature provides the native Rust protocol boundary used by
the Tencent Weixin channel. `IlinkModule` exports a typed `IlinkClient`
provider; the client owns QR login requests, authenticated headers, strict
server URL validation, update polling, text replies, typing calls, and channel
start/stop notifications.

```rust
use a3s_boot::ilink::IlinkModule;

let module = IlinkModule::weixin("A3S/0.10.1");
```

The wire defaults are compatible with Tencent `openclaw-weixin` v2.4.6:
`iLink-App-Id: bot`, `bot_type=3`, and packed client version `2.4.6`. The
product-specific `bot_agent` remains `A3S/<version>` so upstream diagnostics do
not misidentify the caller. Boot deliberately does not own browser APIs,
credential persistence, owner authorization, or agent/session commands; those
policies stay in the host application.

## Architecture

The application core is independent of its HTTP server and message broker:

```text
modules + typed providers
          │
 controllers / gateways / message patterns
          │
 protocol-neutral execution pipeline
          │
 BootRequest / WebSocketMessage / TransportMessage
          │
 HTTP adapter / WebSocket adapter / MessageTransport
```

Source is split by responsibility under `app/`, `module/`, `provider/`,
`routing/`, `pipeline/`, `websocket/`, and `transport/`. Optional infrastructure
modules are feature-gated. The public `HttpAdapter`, `MessageTransport`, and
backend traits are the primary extension contracts.

Axum is currently the bundled HTTP adapter. Compile-time attributes live in the
separate `a3s-boot-macros` crate and expand into the same explicit definitions
used by the core API.

## Development

Run checks from the `a3s-boot` crate directory:

```bash
cargo fmt --all -- --check
cargo test
cargo test --all-features
cargo clippy --all-targets --all-features -- -D warnings
RUSTDOCFLAGS="-D warnings" cargo doc --all-features --no-deps
```

The test suite covers modules and providers, scoped contexts, lifecycle,
routing, pipelines, macros, validation, OpenAPI, WebSocket, transports, and
feature-gated infrastructure. Tests for external transports may require their
corresponding services or environment configuration.

See [Roadmap](ROADMAP.md) for the Nest compatibility plan and remaining work.

## License

MIT
