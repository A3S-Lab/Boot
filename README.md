# A3S Boot

<p align="center">
  <strong>Progressive Rust Web Framework for A3S</strong>
</p>

<p align="center">
  <em>A Rust-first, Nest-inspired framework built around modules, providers, controllers, pipelines, and replaceable HTTP adapters.</em>
</p>

---

## Overview

**A3S Boot** is a progressive Rust web framework crate for building modular A3S
services. It takes the architectural ideas that make Nest.js useful for growing
services and expresses them in explicit, idiomatic Rust:

- explicit application modules
- importable feature modules
- typed providers resolved through `ModuleRef`
- controller route groups
- framework-neutral route definitions and requests
- global, controller-level, and route-level pipes, guards, interceptors, and
  exception filters
- typed JSON DTO helpers for controller inputs and responses
- replaceable HTTP adapters
- a single application builder
- startup and shutdown module lifecycle hooks

Rust does not have TypeScript's runtime decorator metadata model. A3S Boot
supports Nest-style Rust attribute macros through `a3s-boot-macros`, and those
macros expand at compile time into the same explicit module, provider, and
controller definitions used by the core API. Axum is the default adapter, not
the framework kernel. If you are coming from Nest.js, see
[Nest-Style Attribute Macros](#nest-style-attribute-macros) for the
`@Injectable` and `@Controller` style.

## Status

This repository contains the first framework slice:

- `Module` for declaring imports, providers, controllers, direct routes, and lifecycle hooks
- `ModuleRef` for typed provider lookup, optional lookup, presence checks, and token listing
- `ProviderDefinition` for singleton, factory, and shared `Arc` factory providers
- `ControllerDefinition` for prefix-based route groups
- `a3s-boot-macros` with `#[injectable]`, `#[controller]`, and route attributes
  such as `#[get("/{id}")]`, `#[post("/", status = 201)]`, and
  `#[sse("/events")]`
- `Pipe`, `Guard`, `Interceptor`, and `ExceptionFilter` pipeline traits
- application-level `use_global_pipe`, `use_global_guard`,
  `use_global_interceptor`, and `use_global_filter`
- exception filters that can recover errors from pipes, guards, handlers, and
  interceptors and decline to the next filter
- `BootRequest::text`, `json`, `with_text`, `with_json`, `BootResponse::json`,
  and controller `*_json` / `*_json_with_status` route helpers
- `BootResponse::text_with_status(...)`, `json_with_status(...)`,
  `sse(...)`, `body_text(...)`, `body_json(...)`, `from_error(...)`,
  `empty(...)`, and `no_content()` response helpers, redirect helpers,
  response body helpers, response status predicates, and response validation
- `SseEvent` and streaming `text/event-stream` route helpers for Nest-style SSE
- JSON request content-type and response accept helpers with HTTP 415/406 mapping
- request authorization/cookie helpers, HTTP 401 unauthorized mapping, and
  opt-in `WWW-Authenticate` response challenges
- route helpers for GET/POST/PUT/PATCH/DELETE/OPTIONS/HEAD and JSON responses
- `HttpMethod` canonical display names and strict parsing for supported methods
- percent-decoded path params through `{id}` route segments, including typed
  path and query DTOs
- case-insensitive request and response header lookup, plus content-type and
  strict content-length helpers
- application-level `global_prefix(...)` route prefixing
- `BootError` HTTP status and response-message helpers for custom adapters
- `HttpAdapter` for plugging in different HTTP backends without coupling core to Axum
- `AxumAdapter` behind the default `axum` feature
- `BootApplicationBuilder` for resolving module imports, providers, controllers, and routes
- `BootApplication::call(...)` and `handle(...)` for framework-neutral
  in-process request dispatch, `route_for(...)` and `route_match(...)` for route lookup, and
  `allowed_methods(...)` / `allowed_methods_header(...)` for adapter method introspection
- duplicate module deduplication by module name
- duplicate route rejection by HTTP method and path shape
- framework-neutral route registration
- route calls validate HTTP method and path pattern before executing handlers
- route matching preserves exact path segment shape, including trailing slashes,
  and prefers more specific static routes over parameter routes
- Axum requests preserve the real client HTTP method before route execution
- Axum routes register exact methods, so GET does not implicitly expose HEAD
- Axum HEAD responses preserve status and headers while sending an empty body
- Axum route registration normalizes parameter names so different methods can
  share the same dynamic path shape
- Axum unmatched paths map to Boot-style HTTP 404 responses
- Axum method-not-allowed paths map to Boot-style HTTP 405 responses while
  preserving exact Boot `Allow` values and route filters
- Axum body limit failures, including oversized `Content-Length` declarations,
  map to HTTP 413 Payload Too Large
- `BootApplication::bootstrap`, `shutdown`, and `serve_with(...)` lifecycle entrypoints
- `serve_with(...)` runs bootstrap before the adapter and shutdown after the adapter returns,
  including bootstrap and adapter errors

## Quick Start

```toml
[dependencies]
a3s-boot = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

```rust
use a3s_boot::{
    AxumAdapter, BootApplication, BootResponse, ControllerDefinition, Module, ModuleRef,
    ProviderDefinition, Result,
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
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await
}
```

Run the example:

```sh
cargo run --example hello
```

## Nest-Style Attribute Macros

The default `a3s-boot` features include `a3s-boot-macros`, so applications can
write Rust attributes that feel close to Nest.js decorators:

| Nest.js decorator | A3S Boot attribute macro |
| --- | --- |
| `@Injectable()` | `#[injectable]` on a service struct |
| `@Controller("cats")` | `#[controller("/cats")]` on an inherent `impl` block |
| `@Get(":id")` | `#[get("/{id}")]` on an async method |
| `@Post()` | `#[post("/", status = 201)]` on an async method |
| `@Sse("events")` | `#[sse("/events")]` on an async method returning an SSE event stream |
| Constructor injection | Resolve dependencies from `ModuleRef`, store them in the controller, then call `Arc<Self>.controller()?` |
| `@Module({ providers, controllers, imports })` | `impl Module` with `providers()`, `controllers()`, and `imports()` |

These are Rust procedural macros, not TypeScript runtime decorators. They
generate ordinary `ProviderDefinition` and `ControllerDefinition` values at
compile time. The explicit API remains available and is what the macros expand
into:

```rust
use std::sync::Arc;

use a3s_boot::{
    controller, injectable, AxumAdapter, BootApplication, BootRequest, BootResponse,
    ControllerDefinition, Module, ModuleRef, ProviderDefinition, Result, SseEvent, SseStream,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
}

#[derive(Debug, Serialize)]
struct CatDto {
    id: String,
    name: String,
}

#[injectable]
#[derive(Debug)]
struct CatsService;

impl CatsService {
    fn find_one(&self, id: &str) -> CatDto {
        CatDto {
            id: id.to_string(),
            name: "Milo".to_string(),
        }
    }

    fn create(&self, dto: CreateCatDto) -> CatDto {
        CatDto {
            id: "generated".to_string(),
            name: dto.name,
        }
    }
}

#[derive(Debug)]
struct CatsController {
    cats: Arc<CatsService>,
}

#[controller("/cats")]
impl CatsController {
    #[get("/{id}")]
    async fn find_one(&self, request: BootRequest) -> Result<CatDto> {
        let id = request.param("id").unwrap_or("unknown");
        Ok(self.cats.find_one(id))
    }

    #[post("/", status = 201)]
    async fn create(&self, dto: CreateCatDto) -> Result<CatDto> {
        Ok(self.cats.create(dto))
    }

    #[get("/health", raw)]
    async fn health(&self) -> Result<BootResponse> {
        Ok(BootResponse::text("ok"))
    }

    #[sse("/events")]
    async fn events(&self) -> Result<SseStream> {
        Ok(SseEvent::stream([
            SseEvent::new("Milo").with_event("cat.found")
        ]))
    }
}

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![CatsService.into_provider()])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let cats = module_ref.get::<CatsService>()?;
        Ok(vec![Arc::new(CatsController { cats }).controller()?])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let app = BootApplication::builder().import(CatsModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await
}
```

`#[injectable]` adds provider helper methods such as `into_provider()` and
`from_arc_provider(...)`. `#[controller("/cats")]` adds a
`controller(self: Arc<Self>)` method that collects route attributes from the
impl block. GET, POST, PUT, PATCH, and DELETE route attributes default to JSON:
`#[get]` and `#[delete]` can accept `BootRequest` and return serializable DTOs,
while `#[post]`, `#[put]`, and `#[patch]` accept one JSON DTO argument and
return a serializable DTO. Add `raw` only when the method should return
`Result<BootResponse>` directly, for example `#[get("/health", raw)]`. The
explicit `*_json` route attributes remain available as compatibility aliases,
but typical code should use `#[get]` and `#[post]` directly.
`#[sse("/events")]` registers a GET endpoint that returns a
`text/event-stream` response and accepts any stream whose items are
`Result<SseEvent>`.

## Server-Sent Events

SSE routes mirror Nest.js `@Sse()` endpoints: handlers return a stream of
`SseEvent` values, and Boot sends them as `text/event-stream` chunks through the
selected adapter. `SseEvent::stream(...)` is a small helper for finite streams;
long-running handlers can return any `Stream<Item = Result<SseEvent>>`.

```rust
use a3s_boot::{ControllerDefinition, Result, SseEvent, SseStream};

fn notifications_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/notifications")?.sse("/events", |_| async {
        Ok(SseEvent::stream([
            SseEvent::new("ready").with_event("app.ready"),
            SseEvent::json(&serde_json::json!({ "name": "Milo" }))?.with_id("cat-1"),
        ]))
    })
}
```

SSE routes require clients to accept `text/event-stream`; missing `Accept`,
`*/*`, `text/*`, and `text/event-stream` are accepted, while requests that only
accept unrelated media types return `BootError::NotAcceptable`.

## Providers

Providers can be registered as owned singletons, factories, shared `Arc<T>`
values, or factories that return `Arc<T>`. Provider tokens are unique across the
resolved module graph:

```rust
use std::sync::Arc;

use a3s_boot::{BootApplication, ModuleRef, ProviderDefinition, Result};

#[derive(Debug)]
struct Client;

#[derive(Debug)]
struct Repository {
    client: Arc<Client>,
}

let providers = vec![
    ProviderDefinition::factory_arc::<Client, _>(|_module_ref: &ModuleRef| {
        Ok(Arc::new(Client))
    }),
    ProviderDefinition::factory::<Repository, _>(|module_ref| {
        Ok(Repository {
            client: module_ref.get::<Client>()?,
        })
    }),
    ProviderDefinition::named_factory_arc::<Client, _>("readonly-client", |_| {
        Ok(Arc::new(Client))
    }),
];

fn inspect(module_ref: &ModuleRef) -> Result<()> {
    let maybe_client = module_ref.get_optional::<Client>()?;
    let has_client = module_ref.contains_provider::<Client>()?;
    let tokens = module_ref.tokens()?;

    let _ = (maybe_client, has_client, tokens);
    Ok(())
}

fn inspect_app(app: &BootApplication) -> Result<()> {
    let repository = app.get::<Repository>()?;
    let readonly = app.get_named::<Client>("readonly-client")?;
    let missing = app.get_optional_named::<Client>("missing-client")?;

    let _ = (repository, readonly, missing);
    Ok(())
}
```

## JSON DTOs

Controllers can accept typed request DTOs and return serializable response DTOs
without manually parsing request bytes:

```rust
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, HttpMethod, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct CreateCatDto {
    name: String,
}

#[derive(Debug, Serialize)]
struct CatDto {
    name: String,
    adopted: bool,
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.post_json_with_status(
        "/",
        201,
        |dto: CreateCatDto| async move {
            Ok(CatDto {
                name: dto.name,
                adopted: false,
            })
        },
    )
}

fn manual_response() -> Result<BootResponse> {
    BootResponse::json(&CatDto {
        name: "Milo".to_string(),
        adopted: false,
    })
}

fn manual_request() -> Result<BootRequest> {
    BootRequest::new(HttpMethod::Post, "/cats").with_json(&CreateCatDto {
        name: "Milo".to_string(),
    })
}
```

JSON body route helpers require a JSON-compatible request content type such as
`application/json` or `application/*+json`; missing or non-JSON content types map
to `BootError::UnsupportedMediaType` and HTTP 415. JSON response route helpers
honor `Accept`: requests with no `Accept` header, `application/json`,
concrete `application/*+json` types such as `application/problem+json`,
`application/*`, `application/*+json`, or `*/*` can receive JSON; requests that
explicitly exclude JSON map to `BootError::NotAcceptable` and HTTP 406. Invalid
JSON and invalid UTF-8 text bodies map to `BootError::BadRequest`; adapters can
turn those into HTTP 400 while exception filters can override the response
shape. Manual handlers can use `request.json()` when they only want to parse
bytes, `request.json_with_content_type()` when they want the same content-type
check, and `request.require_accepts_json()` before returning JSON from custom
handlers. Use `*_json_with_status(path, status, handler)` on
`ControllerDefinition` or `RouteDefinition` when the helper should still parse
and serialize DTOs but return a non-200 status such as 201 or 202.

Empty status responses can use a dedicated helper instead of constructing an
empty byte vector, and text or JSON responses can set a status code while
preserving the right content type. In-process callers can decode response bodies
with `body_text()` and `body_json()`, or check status classes with helpers like
`is_success()` and `is_client_error()`. They can also check `has_body()` and
`allows_body()` or call `validate()` before handing a response to an adapter.
`validate()` runs status-code, `Content-Length`, no-body status, and response
header name/value checks in adapter order; the individual `validate_status()`,
`validate_content_length()`, `validate_body_allowed()`, and `validate_headers()`
helpers remain available for focused checks. Error responses can reuse the framework's standard HTTP error
mapping:

```rust
use a3s_boot::{BootResponse, Result};

fn deleted() -> BootResponse {
    BootResponse::no_content()
}

fn created() -> Result<BootResponse> {
    BootResponse::json_with_status(201, &CatDto {
        name: "Milo".to_string(),
        adopted: false,
    })
}

fn rejected() -> BootResponse {
    BootResponse::text_with_status(400, "invalid cat")
}

fn moved() -> BootResponse {
    BootResponse::temporary_redirect("/cats/milo")
}

fn read_response(response: &BootResponse) -> Result<CatDto> {
    assert!(response.is_success());
    assert!(response.allows_body());
    assert!(response.has_body());
    response.validate_body_allowed()?;
    response.body_json()
}

fn from_error(error: &a3s_boot::BootError) -> BootResponse {
    BootResponse::from_error(error)
}
```

GET and DELETE helpers can also serialize response DTOs while still exposing the
request for params, query values, and headers:

```rust
use a3s_boot::{BootRequest, ControllerDefinition, Result};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct CatDto {
    name: String,
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.get_json("/{id}", |request: BootRequest| async move {
        Ok(CatDto {
            name: request.param("id").unwrap_or("unknown").to_string(),
        })
    })
}
```

## Params And Query

Boot keeps route params adapter-neutral. Use whole `{name}` segments in routes
and read decoded values from `BootRequest` one at a time or as a typed DTO;
query strings can be read as raw single values, repeated values, or decoded into
a typed DTO. Parameter names must be non-empty, well-formed, and unique after
controller and global prefixes are applied. Route definitions and prefixes are
path-only and reject query or fragment markers; read query values from the
request instead. Invalid percent encoding and invalid UTF-8 in decoded params or
query values map to `BootError::BadRequest`. Prefer `query_value(...)` and
`query_values(...)` when the handler should reject malformed query strings; use
`query_pairs(...)` when the handler or adapter needs every decoded query pair,
including repeated keys.
Route definitions can also be inspected without executing handlers via
`matches_path(...)`, `path_params(...)`, `path_shape(...)`, and
`path_param_names(...)`.

```rust
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct CatParams {
    id: String,
}

#[derive(Debug, Deserialize)]
struct FindCatQuery {
    verbose: bool,
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.get("/{id}", |request: BootRequest| async move {
        let params: CatParams = request.params()?;
        let query: FindCatQuery = request.query()?;
        let sort = request.query_value("sort")?.unwrap_or_else(|| "name".to_string());
        let labels = request.query_values("label")?;
        let pairs = request.query_pairs()?;

        Ok(BootResponse::text(format!(
            "cat={id}, verbose={}, sort={sort}, labels={}, pairs={}",
            id = params.id,
            query.verbose,
            labels.join(","),
            pairs.len()
        )))
    })
}
```

## Global Prefix And Headers

Use an application prefix when an adapter should expose every route under a
shared base path:

```rust
use a3s_boot::{BootApplication, BootResponse, RouteDefinition, Result};

fn app() -> Result<BootApplication> {
    BootApplication::builder()
        .global_prefix("/api/v1")
        .route(RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("ok").with_header("X-Boot", "ready"))
        })?)
        .build()
}
```

Header helpers normalize names for storage and lookup:

```rust
use a3s_boot::{BootRequest, BootResponse, HttpMethod};

let request = BootRequest::new(HttpMethod::Post, "/")
    .with_content_type("application/json")
    .with_body("{}")
    .with_content_length(2)
    .with_header("Authorization", "Bearer token-123")
    .with_header("Cookie", "session=abc; theme=dark")
    .append_header("Accept", "application/json")
    .append_header("Accept", "text/plain");

assert_eq!(request.method(), HttpMethod::Post);
assert_eq!(request.path(), "/");
assert_eq!(request.body(), b"{}");
assert_eq!(request.header("content-type"), Some("application/json"));
assert_eq!(request.header("CONTENT-TYPE"), Some("application/json"));
assert_eq!(request.content_type(), Some("application/json"));
assert_eq!(request.content_length().unwrap(), Some(2));
assert_eq!(request.strict_content_length().unwrap(), Some(2));
request.validate_headers().unwrap();
request.validate_content_length().unwrap();
request.validate_body_limit(1024).unwrap();
request.validate().unwrap();
request.validate_with_body_limit(1024).unwrap();
assert!(request.is_json_content_type());
assert!(request.accepts_json());
assert_eq!(request.authorization(), Some("Bearer token-123"));
assert_eq!(request.bearer_token(), Some("token-123"));
assert_eq!(request.require_bearer_token().unwrap(), "token-123");
assert_eq!(request.cookie("session").unwrap().as_deref(), Some("abc"));
assert_eq!(request.require_cookie("session").unwrap(), "abc");
assert_eq!(request.header_values("accept"), ["application/json", "text/plain"]);
assert!(request
    .header_entries()
    .any(|(name, value)| name == "content-type" && value == "application/json"));

let response = BootResponse::new(200, b"{}".to_vec())
    .with_content_type("application/json")
    .with_content_length(2)
    .with_location("/items/42")
    .append_header("Set-Cookie", "session=abc; Path=/")
    .append_header("Set-Cookie", "theme=dark; Path=/");

assert_eq!(
    response.header_values("set-cookie"),
    ["session=abc; Path=/", "theme=dark; Path=/"]
);
assert_eq!(response.status(), 200);
assert_eq!(response.body(), b"{}");
assert_eq!(response.header_entries().count(), 5);
assert_eq!(response.content_length().unwrap(), Some(2));
assert_eq!(response.strict_content_length().unwrap(), Some(2));
response.validate_content_length().unwrap();
assert_eq!(response.location(), Some("/items/42"));
assert!(response.is_json_content_type());

let unauthorized = BootResponse::empty(401)
    .with_www_authenticate(r#"Bearer realm="api""#)
    .append_www_authenticate(r#"Basic realm="legacy""#);

assert_eq!(unauthorized.www_authenticate(), Some(r#"Bearer realm="api""#));
assert_eq!(
    unauthorized.www_authenticate_values(),
    [r#"Bearer realm="api""#, r#"Basic realm="legacy""#]
);
```

The Axum adapter rejects request header values that cannot be represented as
text and reports them as `BootError::BadRequest`; invalid `Content-Length`
headers are also rejected as bad requests, repeated `Content-Length` values
must agree, declared lengths must match the decoded body length, and values
above the configured body limit map to HTTP 413 before the body is read.
The same request header, strict repeated-header, and body-length checks are
available in core via `BootRequest::validate_headers()`,
`BootRequest::strict_content_length()`, `BootRequest::validate_content_length()`,
`BootRequest::validate_body_limit(...)`, `BootRequest::validate()`, and
`BootRequest::validate_with_body_limit(...)`. Use `validate()` for header and
`Content-Length` checks, or `validate_with_body_limit(...)` when an adapter also
needs body-limit enforcement after reading the body. Use `header_entries()` on
requests and responses when an adapter needs to forward every stored header
line, including appended repeated headers. Request and response accessors such
as `method()`, `path()`, `status()`, `body()`, and `into_body()` let adapters
read common fields without depending on struct fields directly. Response-side
checks are available through `BootResponse::strict_content_length()` and
`BootResponse::validate_content_length()`.
Invalid response status codes or headers are reported as internal adapter
errors instead of being silently dropped; adapters can reuse
`BootResponse::validate()` for the same status-code, `Content-Length`, no-body
status, and response header checks. Response `Content-Length` values must also
be valid, consistent, and match the response body length, and statuses that
cannot carry a body reject non-empty response bodies. Unsupported HTTP
methods are rejected as method-not-allowed errors instead of being remapped to
GET.

## Replace The HTTP Backend

The core crate depends on the `HttpAdapter` trait, not on Axum. Axum is the
first adapter because it is a strong default for async Rust services, but a
Boot application can be served by any backend that can translate Boot routes,
requests, and responses.

Disable the default adapter when you only want the framework-neutral core:

```toml
[dependencies]
a3s-boot = { version = "0.1", default-features = false }
```

Implement an adapter for another HTTP stack, test harness, in-process gateway,
or custom runtime. In-process callers can also dispatch through the resolved
route table with `BootApplication::call(...)` or `handle(...)`, reusing route
matching, parameter decoding, pipeline hooks, and exception filters. `call(...)`
returns unhandled `BootError`s while `handle(...)` converts them to
`BootResponse::from_error(...)`. Individual route snapshots expose the same
`call(...)` and `handle(...)` split for direct dispatch after an adapter has
selected a route. Custom adapters can
also query `BootApplication::route_for(...)`, `route_match(...)`, and
`BootApplication::allowed_methods(...)` from the same most-specific path
matching rules. `allowed_methods_header(...)` returns the corresponding
comma-separated `Allow` header value when a path matches. Adapters can build
method-not-allowed responses and use
`BootResponse::from_error(...)` or `BootError::http_status_code(...)` plus
`http_response_message(...)` for consistent error responses. Route snapshots
also expose resolved path shape, path parameter names, module metadata, and
controller metadata for adapter registration, logging, and diagnostics. When an
adapter has an actual request path, `route_match(...)` returns the selected
route plus decoded path parameter values with the same bad-request semantics as
route execution:

```rust
use std::net::SocketAddr;

use a3s_boot::{BootApplication, BoxFuture, HttpAdapter, Result};

#[derive(Debug)]
struct RouteSnapshot {
    routes: Vec<RouteInfo>,
}

#[derive(Debug)]
struct RouteInfo {
    path: String,
    path_shape: String,
    path_params: Vec<String>,
    method: &'static str,
    module_name: Option<String>,
    controller_prefix: Option<String>,
}

#[derive(Debug, Default)]
struct SnapshotAdapter;

impl HttpAdapter for SnapshotAdapter {
    type Output = RouteSnapshot;

    fn build(&self, app: BootApplication) -> Result<Self::Output> {
        Ok(RouteSnapshot {
            routes: app
                .routes()
                .iter()
                .map(|route| RouteInfo {
                    path: route.path().to_string(),
                    path_shape: route.path_shape(),
                    path_params: route
                        .path_param_names()
                        .into_iter()
                        .map(str::to_string)
                        .collect(),
                    method: route.method().as_str(),
                    module_name: route.module_name().map(str::to_string),
                    controller_prefix: route.controller_prefix().map(str::to_string),
                })
                .collect(),
        })
    }

    fn serve(&self, app: BootApplication, addr: SocketAddr) -> BoxFuture<'static, Result<()>> {
        let route_count = app.routes().len();
        Box::pin(async move {
            println!("serving {route_count} routes on {addr}");
            Ok(())
        })
    }
}
```

## Design Direction

A3S Boot aims to provide a structured service framework for A3S components:

| Concept | Direction |
| --- | --- |
| Module | A named feature boundary with imports, providers, and routes |
| ModuleRef | Typed provider container used by controllers and hosts |
| HTTP adapter | Replaceable backend adapter; Axum is the first implementation |
| Controller | Typed request handlers grouped by route prefix |
| Provider | Injectable service or repository dependency |
| Guard | Request authorization and policy gate |
| Interceptor | Cross-cutting request/response behavior at global, controller, or route scope |
| Pipe | Request validation and transformation |
| Filter | Error mapping into HTTP responses |
| Lifecycle hook | Startup and shutdown behavior for modules and providers |

The design is intentionally progressive: a small service can start with direct
routes, then move into modules, providers, controllers, and request pipelines as
the codebase grows. The framework core remains independent from any specific
HTTP backend so A3S Gateway, A3S Code services, and standalone control-plane
APIs can choose their adapter.

## Source Layout

The crate is split by framework concern:

```text
src/
├── adapters/     # Optional backend adapters such as Axum
├── app/          # Application instance, builder, and module registration
├── http/         # Adapter-neutral request, response, methods, and query parsing
├── module/       # Module trait and lifecycle hooks
├── pipeline/     # Pipes, guards, interceptors, filters, and execution context
├── provider/     # Provider tokens, definitions, and ModuleRef container
├── routing/      # Route handlers, controllers, route execution, and path matching
├── error.rs
└── lib.rs
```

`lib.rs` only exports the public surface. Behavior tests live under `tests/`
and exercise the crate through public APIs.

## Development

```sh
cargo fmt --all
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## License

MIT
