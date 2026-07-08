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
- protocol-neutral execution context for shared guards and observers
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
`@Injectable` and `@Controller` style. See [ROADMAP.md](ROADMAP.md) for the
Nest parity development plan.

## Quick Start

```toml
[dependencies]
a3s-boot = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional ACL-backed configuration module when the application needs
typed runtime configuration:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["config"] }
serde = { version = "1", features = ["derive"] }
```

Enable the optional in-memory cache module when the application needs a typed
cache provider:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["cache"] }
serde = { version = "1", features = ["derive"] }
```

Enable the optional scheduler module when the application needs Nest-style
scheduled jobs:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["schedule"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional in-process queue module when the application needs
provider-backed background job processing:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["queue"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional TCP microservice transport when services need
newline-delimited JSON message patterns over a network socket:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["tcp-transport"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional Redis microservice transport when services need Nest-style
request-response and event-only message patterns over Redis Pub/Sub:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["redis-transport"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional NATS microservice transport when services need Nest-style
request-response and event-only message patterns over NATS subjects:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["nats-transport"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional MQTT microservice transport when services need Nest-style
request-response and event-only message patterns over MQTT topics:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["mqtt-transport"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional RabbitMQ microservice transport when services need
Nest-style request-response and event-only message patterns over AMQP queues:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["rabbitmq-transport"] }
serde = { version = "1", features = ["derive"] }
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
```

Enable the optional structured logger module when the application needs
provider-backed logging without forcing a concrete backend:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["logging"] }
```

Enable the optional gzip compression interceptor when the application should
compress eligible responses for clients that send `Accept-Encoding: gzip`:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["compression"] }
```

Enable optional multipart file upload helpers when handlers need to parse
`multipart/form-data` requests:

```toml
[dependencies]
a3s-boot = { version = "0.1", features = ["file-upload"] }
```

```rust
use a3s_boot::{
    AxumAdapter, BootFactory, BootResponse, ControllerDefinition, Module, ModuleRef,
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
    let mut app = BootFactory::create(AppModule)?;
    app.listen_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await
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
| `@Controller({ host: ":account.example.com" })` | `#[host("{account}.example.com")]` below `#[controller]` |
| `@Get(":id")` | `#[get("/{id}")]` on an async method |
| `@Post()` | `#[post("/", status = 201)]` on an async method |
| `@All("catch")` | `#[all("/catch")]` on an async method that handles every standard HTTP method |
| `@Param("id")` | `#[param("id")]` on a method argument |
| `@Param("id", ParsePipe)` | `#[param("id", pipe = parse_cat_id)]` on a method argument |
| `@HostParam("account")` | `#[host_param("account")]` on a method argument |
| `@Query()` / `@Query("page")` | `#[query]` for a DTO or `#[query("page")]` for one value |
| `@Query("page", ParsePipe)` | `#[query("page", pipe = parse_page)]` on a method argument |
| `@Body()` | `#[body]` on a JSON body DTO argument |
| `@Headers("x-request-id")` | `#[header("x-request-id")]` on a method argument |
| `@Ip()` | `#[ip]` on a method argument |
| `@Req()` | `#[request]` on a `BootRequest` argument |
| `createParamDecorator(...)` | `#[extract(current_user)]` with a `RequestExtractor<T>` or function |
| `@Sse("events")` | `#[sse("/events")]` on an async method returning an SSE event stream |
| `@WebSocketGateway()` | `#[websocket_gateway("/ws")]` on an inherent `impl` block |
| `@SubscribeMessage("cat.find")` | `#[subscribe_message("cat.find")]` on an async gateway method |
| Microservice controller | `#[message_controller]` on an inherent `impl` block |
| `@MessagePattern("cat.find")` | `#[message_pattern("cat.find")]` on an async message method |
| `@EventPattern("cat.created")` | `#[event_pattern("cat.created")]` on an async event method |
| `@Payload()` | A typed message method argument deserialized from `TransportMessage::data` |
| `@OnEvent("cat.created")` | `#[on_event("cat.created")]` inside `#[event_listener]` |
| `@UseGuards(AuthGuard)` | `#[use_guard(AuthGuard)]` on a controller impl or route method |
| `@UseInterceptors(TraceInterceptor)` | `#[use_interceptor(TraceInterceptor)]` on a controller impl or route method |
| `@UseFilters(HttpErrorFilter)` | `#[use_filter(HttpErrorFilter)]` on a controller impl or route method |
| `@Catch(BadRequestException)` | `catch_errors([BootErrorKind::BadRequest], BadRequestFilter)` inside `#[use_filter(...)]` or `with_catch_filter(...)` |
| `@UsePipes(ParsePipe)` | `#[use_pipe(ParsePipe)]` on a controller impl or route method |
| `@UsePipes(new ValidationPipe())` | `#[validate]` on a controller impl or route method for DTO validation |
| `@SetMetadata("roles", ["admin"])` | `#[metadata("roles", ["admin"])]` below `#[controller]` or on a route method |
| `@Version("1")` | `#[version("1")]` below `#[controller]` or on a route method |
| `@Version(["1", "2"])` | `#[versions("1", "2")]` below `#[controller]` or on a route method |
| `VERSION_NEUTRAL` | `#[version_neutral]` below `#[controller]` or on a route method |
| `@SerializeOptions(...)` | `#[serialize(include = ["id"], exclude = ["password"], skip_null)]` below `#[controller]` or on a route method |
| `@Cron("0 0 0 * * * *")` | `#[cron("cats.prune", "0 0 0 * * * *")]` inside `#[schedule]` |
| `@Interval("cats.refresh", 60000)` | `#[interval("cats.refresh", 60000)]` inside `#[schedule]` |
| `@Timeout("cats.warmup", 5000)` | `#[timeout("cats.warmup", 5000)]` inside `#[schedule]` |
| `@HttpCode(202)` | `#[http_code(202)]` on a JSON route method |
| `@Header("cache-control", "max-age=60")` | `#[header("cache-control", "max-age=60")]` on a route method |
| `@Redirect("/new", 301)` | `#[redirect("/new", status = 301)]` on a route method |
| `@ApiTags("cats")` | `#[tag("cats")]` below `#[controller]` |
| `@ApiOperation(...)` | `#[operation(summary = "...", operation_id = "...")]` on a route method |
| `@ApiResponse(...)` | `#[response(status = 200, description = "...", schema = CatDto)]` |
| `@ApiBearerAuth()` | `#[bearer_auth]` on a route method |
| Constructor injection | `#[injectable]` fields such as `cats: Arc<CatsService>` plus `CatsController::provider()` |
| `@Inject("TOKEN")` | `#[inject("token")]` on an `Arc<T>` or `Option<Arc<T>>` field |
| `@Optional()` | `Option<Arc<T>>` on an injectable field |
| `@Module({ providers, controllers, imports })` | `impl Module` with `providers()`, `controllers()`, and `imports()` |
| `NestFactory.create(AppModule)` | `BootFactory::create(AppModule)?` |
| `app.listen(3000)` | `app.listen_with(&AxumAdapter::new(), addr).await` |
| `app.close()` | `app.close().await` |
| `NestFactory.createApplicationContext(...)` | `BootFactory::create_application_context(...)` |
| `NestFactory.createMicroservice(...)` | `BootFactory::create_microservice(...)` |

These are Rust procedural macros, not TypeScript runtime decorators. They
generate ordinary `ProviderDefinition`, `ControllerDefinition`, and
`MessagePatternDefinition` values at
compile time. The explicit API remains available and is what the macros expand
into:

```rust
use std::sync::Arc;

use a3s_boot::{
    controller, injectable, AxumAdapter, BootError, BootFactory, BootRequest, BootResponse,
    ControllerDefinition, Module, ModuleRef, ProviderDefinition, Result, SseEvent, SseStream,
    Validate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
}

impl Validate for CreateCatDto {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize)]
struct FindCatQuery {
    include_toys: Option<bool>,
}

impl Validate for FindCatQuery {}

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

#[injectable]
#[derive(Debug)]
struct CatsController {
    cats: Arc<CatsService>,
}

#[controller("/cats")]
#[validate]
#[tag("cats")]
impl CatsController {
    #[get("/{id}")]
    #[operation(summary = "Find a cat", operation_id = "findCat")]
    #[response(status = 200, description = "Cat found", schema = CatDto)]
    #[bearer_auth]
    async fn find_one(
        &self,
        #[param("id")] id: String,
        #[query] query: FindCatQuery,
        #[header("x-request-id")] request_id: Option<String>,
    ) -> Result<CatDto> {
        let mut cat = self.cats.find_one(&id);
        if query.include_toys.unwrap_or(false) {
            cat.name = format!("{} with toys", cat.name);
        }
        if let Some(request_id) = request_id {
            cat.id = format!("{}:{request_id}", cat.id);
        }
        Ok(cat)
    }

    #[post("/", status = 201)]
    #[operation(summary = "Create a cat", operation_id = "createCat")]
    #[request_body(schema = CreateCatDto)]
    #[response(status = 201, description = "Cat created", schema = CatDto)]
    async fn create(&self, #[body] dto: CreateCatDto) -> Result<CatDto> {
        Ok(self.cats.create(dto))
    }

    #[all("/catch")]
    async fn catch_all(&self, #[request] request: BootRequest) -> Result<CatDto> {
        Ok(CatDto {
            id: request.method().as_str().to_string(),
            name: "catch".to_string(),
        })
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
        Ok(vec![CatsService::provider(), CatsController::provider()])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![module_ref.get::<CatsController>()?.controller()?])
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let mut app = BootFactory::create(CatsModule)?;
    app.listen_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await
}
```

`#[injectable]` adds auto-wired provider helpers such as `provider()` and
`request_scoped_provider()`, plus explicit value helpers such as
`into_provider()` and `from_arc_provider(...)`. It auto-wires fields shaped as
`Arc<T>` or `Option<Arc<T>>`; add `#[inject("token")]` on a field to resolve a
named provider. `#[controller("/cats")]` adds a
`controller(self: Arc<Self>)` method that collects route attributes from the
impl block. GET, POST, PUT, PATCH, and DELETE route attributes default to JSON.
Use extractor attributes on method arguments for Nest-style request binding:
`#[param("id")]`, `#[params]`, `#[query]`, `#[query("name")]`, `#[body]`,
`#[header("name")]`, `#[headers]`, `#[host_param("account")]`, `#[ip]`, and
`#[request]`. Single-value extractors parse into the argument type with
`FromStr`, so `#[param("id")] id: u64` and `#[query("active")] active: bool`
work without a separate parse pipe. For custom Nest-style parameter pipes, add
`pipe = <expr>` to `#[param]`, `#[query("name")]`, `#[header]`,
`#[host_param]`, or `#[ip]`; the pipe receives the raw `String` and returns
`Result<T>`:

```rust
#[derive(Debug)]
struct CatId(String);

fn parse_cat_id(value: String) -> Result<CatId> {
    if value.starts_with("cat_") {
        Ok(CatId(value))
    } else {
        Err(BootError::BadRequest("invalid cat id".to_string()))
    }
}

fn parse_page(value: String) -> Result<u16> {
    value
        .parse::<u16>()
        .map_err(|error| BootError::BadRequest(format!("invalid page: {error}")))
}

#[get("/{id}")]
async fn find(
    &self,
    #[param("id", pipe = parse_cat_id)] id: CatId,
    #[query("page", pipe = parse_page)] page: Option<u16>,
) -> Result<CatDto> {
    self.cats.find(id, page).await
}
```

Add `raw` only when the method should return
`Result<BootResponse>` directly, for example `#[get("/health", raw)]`. The
explicit `*_json` route attributes remain available as compatibility aliases,
but typical code should use `#[get]` and `#[post]` directly.
`#[sse("/events")]` registers a GET endpoint that returns a
`text/event-stream` response and accepts any stream whose items are
`Result<SseEvent>`.

## Application Factory

`BootFactory` is the NestFactory-style entrypoint for managed startup and
shutdown. `create(...)` returns an application handle with `init()`,
`listen_with(...)`, `close()`, and provider lookup helpers. Use
`create_application_context(...)` for provider-only workers, and
`create_microservice(...)` for standalone message transports. When a module
registers async provider factories, use the async variants:
`create_async(...)`, `create_application_context_async(...)`, or
`create_microservice_async(...)`.

```rust
use a3s_boot::{AxumAdapter, BootFactory, InProcessTransport, Result};

async fn run() -> Result<()> {
    let mut app = BootFactory::create(AppModule)?;
    app.connect_microservice(InProcessTransport::new());
    app.start_all_microservices().await?;
    app.listen_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await?;

    let mut worker = BootFactory::create_application_context(WorkerModule)?;
    worker.init().await?;
    worker.close().await?;

    let mut service =
        BootFactory::create_microservice(CatsModule, InProcessTransport::new())?;
    service.listen().await
}
```

Custom parameter decorators use `#[extract(...)]`, where the expression
implements `RequestExtractor<T>` or is a function that takes `&BootRequest` and
returns `Result<T>`:

```rust
use a3s_boot::{controller, extract, get, BootRequest, Result};

fn current_user(request: &BootRequest) -> Result<String> {
    Ok(request.header("x-user").unwrap_or("anonymous").to_string())
}

#[derive(Debug)]
struct CatsController;

#[controller("/cats")]
impl CatsController {
    #[get("/me")]
    async fn me(&self, #[extract(current_user)] user: String) -> Result<String> {
        Ok(user)
    }
}
```

Manual handlers can use the same parsing rules through
`BootRequest::param_as::<T>()`, `query_value_as::<T>()`,
`query_values_as::<T>()`, `header_as::<T>()`, `host_param_as::<T>()`, and
`ip_as::<T>()`.

Host-scoped controllers mirror Nest's host-based controller option. Put
`#[host("{account}.example.com")]` below `#[controller]` to constrain every
route in that controller, or put `#[host("api.example.com")]` on a route method
to override the controller default:

```rust
use a3s_boot::{controller, get, Result};

#[derive(Debug)]
struct CatsController;

#[controller("/cats")]
#[host("{account}.example.com")]
impl CatsController {
    #[get("/")]
    async fn list(
        &self,
        #[host_param("account")] account: String,
        #[ip] ip: Option<String>,
    ) -> Result<String> {
        Ok(format!("{account}:{:?}", ip))
    }
}
```

Host parameters accept both Boot's `{account}.example.com` style and Nest's
`:account.example.com` style in explicit route definitions. `#[ip]` reads the
standard forwarding headers (`Forwarded`, `X-Forwarded-For`, then `X-Real-Ip`)
as an adapter-neutral client IP hint; when the argument is not `Option<T>`,
missing or invalid values map to `BootError::BadRequest`.

## Validation Pipeline

DTO validation is explicit. Implement `Validate` for request DTOs, then enable
validation globally, at controller scope, or at route scope. Invalid DTOs map to
`BootError::BadRequest`, which adapters expose as HTTP 400. The
`post_validated_json` / `put_validated_json` / `patch_validated_json` helpers are
route-level shortcuts; global and controller-level validation run validators
registered on each route.

```rust
use a3s_boot::{
    BootApplication, BootError, ControllerDefinition, Result, RouteDefinition, Validate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
}

impl Validate for CreateCatDto {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

#[derive(Debug, Serialize)]
struct CatDto {
    name: String,
}

let route = RouteDefinition::post_validated_json("/", |dto: CreateCatDto| async move {
    Ok(CatDto { name: dto.name })
})?;

let controller = ControllerDefinition::new("/cats")?
    .with_validation()
    .route(
        RouteDefinition::post_json("/", |dto: CreateCatDto| async move {
            Ok(CatDto { name: dto.name })
        })?
        .with_body_validation::<CreateCatDto>(),
    )?;

let app = BootApplication::builder()
    .use_global_validation()
    .route(
        RouteDefinition::post_json("/", |dto: CreateCatDto| async move {
            Ok(CatDto { name: dto.name })
        })?
        .with_body_validation::<CreateCatDto>(),
    )
    .build()?;
```

For Nest-style controllers, put `#[validate]` below `#[controller]`. It adds
validators for `#[body]`, `#[query]`, and `#[params]` DTO arguments. Use
`#[skip_validation]` on a route method when a controller-level validation policy
should not apply to that method.

```rust
#[controller("/cats")]
#[validate]
impl CatsController {
    #[post("/", status = 201)]
    async fn create(&self, #[body] dto: CreateCatDto) -> Result<CatDto> {
        Ok(self.cats.create(dto))
    }

    #[get("/search")]
    async fn search(&self, #[query] query: FindCatQuery) -> Result<Vec<CatDto>> {
        Ok(self.cats.search(query))
    }

    #[post("/raw", raw)]
    #[skip_validation]
    async fn raw(&self, #[request] request: BootRequest) -> Result<BootResponse> {
        Ok(BootResponse::text(request.text()?))
    }
}
```

Manual handlers can also call `BootRequest::validated_json::<T>()`,
`validated_query::<T>()`, or `validated_params::<T>()`. Raw handlers are not
validated unless they register validators explicitly, for example with
`RouteDefinition::with_body_validation::<T>()`.

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

## API Versioning

Boot supports Nest-style API versioning without coupling route matching to a
specific HTTP adapter. Enable one version extraction strategy on the application,
then attach versions to individual routes or a whole controller.

```rust
use a3s_boot::{
    ApiVersioning, BootApplication, BootRequest, BootResponse, ControllerDefinition,
    Result, RouteDefinition,
};

let app = BootApplication::builder()
    .enable_api_versioning(ApiVersioning::uri().with_default_version("1"))
    .route(
        RouteDefinition::get_json("/cats/{id}", |request: BootRequest| async move {
            Ok(serde_json::json!({
                "id": request.param("id").unwrap_or("unknown"),
                "version": "1"
            }))
        })?
        .with_version("1"),
    )
    .route(
        RouteDefinition::get("/health", |_| async {
            Ok(BootResponse::text("ok"))
        })?
        .version_neutral(),
    )
    .build()?;
```

With URI versioning, `/v1/cats/milo` matches the route path `/cats/{id}`;
handlers receive decoded params from the unversioned route shape. A
version-neutral route such as `/health` matches any requested version.

Controller-level versions are inherited by routes that do not declare their own
version:

```rust
use a3s_boot::{BootRequest, ControllerDefinition, Result};

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?
        .with_version("1")
        .get_json("/{id}", |request: BootRequest| async move {
            Ok(serde_json::json!({
                "id": request.param("id").unwrap_or("unknown"),
                "version": "1"
            }))
        })
}
```

The macro form mirrors Nest's `@Version()` decorator. Put `#[version("1")]`,
`#[versions("1", "2")]`, or `#[version_neutral]` below `#[controller]` or on a
route method:

```rust
use a3s_boot::{controller, get, Result};

#[derive(Debug)]
struct CatsController;

#[controller("/cats")]
#[version("1")]
impl CatsController {
    #[get("/")]
    async fn list_v1(&self) -> Result<String> {
        Ok("cats v1".to_string())
    }

    #[get("/")]
    #[version("2")]
    async fn list_v2(&self) -> Result<String> {
        Ok("cats v2".to_string())
    }

    #[get("/health")]
    #[version_neutral]
    async fn health(&self) -> Result<String> {
        Ok("ok".to_string())
    }
}
```

Header and media type strategies use the same route metadata:

```rust
use a3s_boot::{ApiVersioning, BootApplication, BootResponse, RouteDefinition};

let header_versioned = BootApplication::builder()
    .enable_api_versioning(ApiVersioning::header("x-api-version"))
    .route(
        RouteDefinition::get("/cats", |_| async {
            Ok(BootResponse::text("cats v1"))
        })?
        .with_version("1"),
    )
    .route(
        RouteDefinition::get("/cats", |_| async {
            Ok(BootResponse::text("cats v2"))
        })?
        .with_version("2"),
    )
    .build()?;

let media_type_versioned = BootApplication::builder()
    .enable_api_versioning(ApiVersioning::media_type())
    .route(
        RouteDefinition::get("/cats", |_| async {
            Ok(BootResponse::text("cats v2"))
        })?
        .with_version("2"),
    )
    .build()?;
```

For header versioning, send `x-api-version: 2`. For media type versioning, send
an `Accept` value such as `application/json; v=2`. Routes without explicit
version metadata match unversioned requests, and also match the configured
default version when one is set with `.with_default_version("1")`.

## OpenAPI Metadata

Boot can generate an OpenAPI 3 document from resolved routes. Route metadata is
adapter-neutral and can be added with builder methods or Nest-style metadata
macros. `serve_openapi(...)` mounts a generated JSON document without including
that document route in its own output. Schema components can be registered
manually, or generated from `schemars::JsonSchema` when the `openapi-schemas`
feature is enabled.

```rust
use a3s_boot::{
    BootApplication, BootRequest, OpenApiInfo, OpenApiResponse, OpenApiSchema,
    RouteDefinition,
};
use serde::Serialize;

#[derive(Debug, Serialize)]
struct CatDto {
    id: String,
    name: String,
}

let route = RouteDefinition::get_json("/cats/{id}", |request: BootRequest| async move {
    Ok(CatDto {
        id: request.param("id").unwrap_or("unknown").to_string(),
        name: "Milo".to_string(),
    })
})?
.with_tag("cats")
.with_operation_id("findCat")
.with_summary("Find a cat")
.with_query_parameter("include_toys", false, OpenApiSchema::boolean())
.with_json_response(200, "Cat found", OpenApiSchema::object())
.with_response(404, OpenApiResponse::description("Cat not found"))
.with_schema_component("CatDto", OpenApiSchema::object());

let app = BootApplication::builder()
    .route(route)
    .serve_openapi("/openapi.json", OpenApiInfo::new("Cats API", "1.0.0"))
    .build()?;

let document = app.openapi(OpenApiInfo::new("Cats API", "1.0.0"));
let json = serde_json::to_value(document)?;
```

Path parameters are inferred from `{name}` route segments and documented as
required string parameters unless the route supplies a more specific path
parameter schema. `ControllerDefinition::with_tag("cats")` applies a tag to all
routes registered after it, similar to Nest Swagger's `@ApiTags`. In macro
controllers, `#[param("id")]`, `#[query("name")]`, `#[header("name")]`, and
`#[body]` also add matching OpenAPI parameter or request-body metadata.

With `openapi-schemas`, routes can collect component schemas directly from
types that derive `schemars::JsonSchema`:

```rust
#[derive(Debug, Serialize, schemars::JsonSchema)]
struct CatDto {
    id: String,
    name: String,
}

let route = RouteDefinition::get_json("/cats/{id}", |request: BootRequest| async move {
    Ok(CatDto {
        id: request.param("id").unwrap_or("unknown").to_string(),
        name: "Milo".to_string(),
    })
})?
.with_json_response(200, "Cat found", OpenApiSchema::reference("CatDto"))
.try_with_json_schema_component::<CatDto>()?;
```

## WebSocket Gateways

WebSocket gateways mirror Nest's `@WebSocketGateway()` and
`@SubscribeMessage()` style while keeping the runtime adapter-neutral. Messages
are JSON objects with an `event` string and optional `data` value. The Axum
adapter registers gateway paths as WebSocket upgrade routes behind the `axum`
feature.

```rust
use std::sync::Arc;

use a3s_boot::{
    injectable, subscribe_message, websocket_gateway, Module, ModuleRef,
    ProviderDefinition, Result, WebSocketGatewayDefinition, WebSocketMessage,
};
use serde::Serialize;

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
}

#[derive(Debug)]
struct CatsGateway {
    cats: Arc<CatsService>,
}

#[websocket_gateway("/cats/ws")]
impl CatsGateway {
    #[subscribe_message("cat.find")]
    async fn find(&self, message: WebSocketMessage) -> Result<WebSocketMessage> {
        let id = message
            .data()
            .get("id")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        WebSocketMessage::json("cat.found", &self.cats.find_one(id))
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

    fn gateways(&self, module_ref: &ModuleRef) -> Result<Vec<WebSocketGatewayDefinition>> {
        let cats = module_ref.get::<CatsService>()?;
        Ok(vec![Arc::new(CatsGateway { cats }).gateway()?])
    }
}
```

The explicit API is available for tests, dynamic modules, and adapters:

```rust
use a3s_boot::{
    BootRequest, HttpMethod, Result, WebSocketGatewayDefinition, WebSocketMessage,
};
use serde_json::json;

async fn dispatch() -> Result<()> {
    let gateway = WebSocketGatewayDefinition::new("/events")?
        .subscribe("ping", |message: WebSocketMessage| async move {
            Ok(WebSocketMessage::new("pong", message.data))
        })?;

    let reply = gateway
        .dispatch(
            BootRequest::new(HttpMethod::Get, "/events"),
            WebSocketMessage::new("ping", json!({ "id": 1 })),
        )
        .await?
        .unwrap();

    assert_eq!(reply.event(), "pong");
    Ok(())
}
```

Gateway-specific `WebSocketPipe`, `WebSocketGuard`, and `WebSocketInterceptor`
hooks run in deterministic order: guards, interceptor `before`, pipes, handler,
then interceptor `after` in reverse order. They are separate from HTTP
middleware because WebSocket message dispatch is event-based rather than
request/response-based, but they follow the same Nest-style pipeline order.

## Microservice Transports

Microservice message patterns mirror Nest's `@MessagePattern()` and
`@EventPattern()` style. The core is adapter-neutral: messages are JSON-like
`TransportMessage` values with a `pattern` and `data`, and external brokers can
implement `MessageTransport`. `InProcessTransport` is included for tests,
workers, and single-process dispatch. Enable the `tcp-transport` feature to use
`TcpTransport` for newline-delimited JSON messages over TCP, or
`redis-transport` to use Redis Pub/Sub channels, or `nats-transport` to use
NATS subjects, `mqtt-transport` to use MQTT topics, or `rabbitmq-transport` to
use RabbitMQ queues.

```rust
use std::sync::Arc;

use a3s_boot::{
    injectable, message_controller, event_pattern, message_pattern, InProcessTransport,
    MessagePatternDefinition, MessageTransport, Module, ModuleRef, ProviderDefinition, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct FindCatMessage {
    id: String,
}

#[derive(Debug, Deserialize, Serialize)]
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
}

#[derive(Debug)]
struct CatsMessages {
    cats: Arc<CatsService>,
}

#[message_controller]
impl CatsMessages {
    #[message_pattern("cat.find")]
    async fn find(&self, payload: FindCatMessage) -> Result<CatDto> {
        Ok(self.cats.find_one(&payload.id))
    }

    #[event_pattern("cat.created")]
    async fn created(&self, payload: FindCatMessage) -> Result<()> {
        let _ = self.cats.find_one(&payload.id);
        Ok(())
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

    fn message_patterns(&self, module_ref: &ModuleRef) -> Result<Vec<MessagePatternDefinition>> {
        let cats = module_ref.get::<CatsService>()?;
        Arc::new(CatsMessages { cats }).message_patterns()
    }
}
```

`#[message_pattern("cat.find")]` defaults to JSON responses, so returning
`Result<CatDto>` becomes a `TransportReply` automatically. Use
`#[message_pattern("cat.find", raw)]` when a handler should return
`TransportReply` directly. Typed method arguments are deserialized from
`TransportMessage::data`; use `TransportMessage` as the argument type when the
handler needs raw access to the pattern and payload.

The explicit API is available for dynamic registration and validation:

```rust
use a3s_boot::{
    BootApplication, InProcessTransport, MessagePatternDefinition, MessageTransport,
    Result, TransportMessage, Validate,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct CreateCatMessage {
    name: String,
}

impl Validate for CreateCatMessage {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(a3s_boot::BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}

async fn dispatch() -> Result<()> {
    let app = BootApplication::builder()
        .message_pattern(MessagePatternDefinition::request_validated_json(
            "cat.create",
            |payload: CreateCatMessage| async move { Ok(payload) },
        )?)
        .build()?;

    let client = InProcessTransport::new().build(app)?;
    let reply = client
        .send(TransportMessage::json(
            "cat.create",
            &CreateCatMessage {
                name: "Luna".to_string(),
            },
        )?)
        .await?
        .unwrap();

    assert_eq!(reply.data()["name"], "Luna");
    Ok(())
}
```

Transport-specific `TransportPipe`, `TransportGuard`, and
`TransportInterceptor` hooks run in deterministic order: guards, interceptor
`before`, pipes, validation, handler, then interceptor `after` in reverse order.

With `tcp-transport`, the same message patterns can be served over a network
socket:

```rust
use std::net::SocketAddr;

use a3s_boot::{BootFactory, Result, TcpTransport, TcpTransportClient, TransportMessage};

async fn run_tcp_microservice() -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 4001));
    let transport = TcpTransport::new(addr);
    let mut service = BootFactory::create_microservice(CatsModule, transport)?;
    service.listen().await
}

async fn call_tcp_microservice() -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], 4001));
    let client = TcpTransportClient::new(addr);
    let reply = client
        .send(TransportMessage::json(
            "cat.find",
            &FindCatMessage {
                id: "milo".to_string(),
            },
        )?)
        .await?
        .unwrap();

    assert_eq!(reply.data_as::<CatDto>()?.name, "Milo");
    Ok(())
}
```

The wire format is one UTF-8 JSON frame per line. Clients send a
`TransportMessage` such as `{"pattern":"cat.find","data":{"id":"1"}}`; servers
reply with a `reply`, `no_reply`, or `error` envelope. Handler errors are mapped
back into the closest `BootError` variant on the client.

With `redis-transport`, request-response messages go through a configured
request channel and receive replies on per-request reply channels. Event
messages are published to a configured event channel:

```rust
use std::time::Duration;

use a3s_boot::{
    BootFactory, RedisTransport, RedisTransportClient, RedisTransportOptions, Result,
    TransportMessage,
};

async fn run_redis_microservice() -> Result<()> {
    let options = RedisTransportOptions::new()
        .with_channel_prefix("cats")
        .with_request_timeout(Duration::from_secs(5));
    let transport = RedisTransport::with_options("redis://127.0.0.1/", options);
    let mut service = BootFactory::create_microservice(CatsModule, transport)?;
    service.listen().await
}

async fn call_redis_microservice() -> Result<()> {
    let options = RedisTransportOptions::new().with_channel_prefix("cats");
    let client = RedisTransportClient::with_options("redis://127.0.0.1/", options);
    let reply = client
        .send(TransportMessage::json(
            "cat.find",
            &FindCatMessage {
                id: "milo".to_string(),
            },
        )?)
        .await?
        .unwrap();

    client
        .emit(TransportMessage::json(
            "cat.created",
            &FindCatMessage {
                id: "luna".to_string(),
            },
        )?)
        .await?;

    assert_eq!(reply.data_as::<CatDto>()?.name, "Milo");
    Ok(())
}
```

With `nats-transport`, request-response messages use NATS request/reply on a
configured subject. Event messages are published to a separate subject. Use a
queue group when multiple service instances should share work:

```rust
use std::time::Duration;

use a3s_boot::{
    BootFactory, NatsTransport, NatsTransportClient, NatsTransportOptions, Result,
    TransportMessage,
};

async fn run_nats_microservice() -> Result<()> {
    let options = NatsTransportOptions::new()
        .with_subject_prefix("cats")
        .with_queue_group("cats-workers")
        .with_request_timeout(Duration::from_secs(5));
    let transport = NatsTransport::with_options("nats://127.0.0.1:4222", options);
    let mut service = BootFactory::create_microservice(CatsModule, transport)?;
    service.listen().await
}

async fn call_nats_microservice() -> Result<()> {
    let options = NatsTransportOptions::new().with_subject_prefix("cats");
    let client = NatsTransportClient::with_options("nats://127.0.0.1:4222", options);
    let reply = client
        .send(TransportMessage::json(
            "cat.find",
            &FindCatMessage {
                id: "milo".to_string(),
            },
        )?)
        .await?
        .unwrap();

    client
        .emit(TransportMessage::json(
            "cat.created",
            &FindCatMessage {
                id: "luna".to_string(),
            },
        )?)
        .await?;

    assert_eq!(reply.data_as::<CatDto>()?.name, "Milo");
    Ok(())
}
```

With `mqtt-transport`, request-response messages are published to a configured
request topic and receive replies on per-request reply topics. Event messages
are published to a separate event topic:

```rust
use std::time::Duration;

use a3s_boot::{
    BootFactory, MqttTransport, MqttTransportClient, MqttTransportOptions, MqttTransportQoS,
    Result, TransportMessage,
};

async fn run_mqtt_microservice() -> Result<()> {
    let options = MqttTransportOptions::new()
        .with_topic_prefix("cats")
        .with_client_id_prefix("cats-service")
        .with_qos(MqttTransportQoS::AtLeastOnce)
        .with_request_timeout(Duration::from_secs(5));
    let transport = MqttTransport::with_options("127.0.0.1", 1883, options);
    let mut service = BootFactory::create_microservice(CatsModule, transport)?;
    service.listen().await
}

async fn call_mqtt_microservice() -> Result<()> {
    let options = MqttTransportOptions::new()
        .with_topic_prefix("cats")
        .with_client_id_prefix("cats-client");
    let client = MqttTransportClient::with_options("127.0.0.1", 1883, options);
    let reply = client
        .send(TransportMessage::json(
            "cat.find",
            &FindCatMessage {
                id: "milo".to_string(),
            },
        )?)
        .await?
        .unwrap();

    client
        .emit(TransportMessage::json(
            "cat.created",
            &FindCatMessage {
                id: "luna".to_string(),
            },
        )?)
        .await?;

    assert_eq!(reply.data_as::<CatDto>()?.name, "Milo");
    Ok(())
}
```

With `rabbitmq-transport`, request-response messages are published to a
configured request queue and receive replies on exclusive per-request reply
queues. Event messages are published to a separate event queue:

```rust
use std::time::Duration;

use a3s_boot::{
    BootFactory, RabbitMqTransport, RabbitMqTransportClient, RabbitMqTransportOptions, Result,
    TransportMessage,
};

async fn run_rabbitmq_microservice() -> Result<()> {
    let options = RabbitMqTransportOptions::new()
        .with_queue_prefix("cats")
        .with_request_timeout(Duration::from_secs(5));
    let transport = RabbitMqTransport::with_options("amqp://127.0.0.1:5672/%2f", options);
    let mut service = BootFactory::create_microservice(CatsModule, transport)?;
    service.listen().await
}

async fn call_rabbitmq_microservice() -> Result<()> {
    let options = RabbitMqTransportOptions::new().with_queue_prefix("cats");
    let client = RabbitMqTransportClient::with_options("amqp://127.0.0.1:5672/%2f", options);
    let reply = client
        .send(TransportMessage::json(
            "cat.find",
            &FindCatMessage {
                id: "milo".to_string(),
            },
        )?)
        .await?
        .unwrap();

    client
        .emit(TransportMessage::json(
            "cat.created",
            &FindCatMessage {
                id: "luna".to_string(),
            },
        )?)
        .await?;

    assert_eq!(reply.data_as::<CatDto>()?.name, "Milo");
    Ok(())
}
```

## Application Events

Enable the `events` feature to use an in-process `EventEmitter` provider,
similar to Nest's event emitter module. `EventModule` can register exact
listeners such as `cat.created`, prefix listeners such as `cat.*`, or a global
`*` listener. Events are dispatched asynchronously and payloads are serialized
through `serde_json`, so handlers can decode typed DTOs with `data_as::<T>()`.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, EventContext, EventEmitter, EventModule, ProviderDefinition, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize, Serialize)]
struct CatEvent {
    name: String,
}

#[derive(Debug)]
struct CatsEvents;

#[a3s_boot::event_listener]
impl CatsEvents {
    #[a3s_boot::on_event("cat.created")]
    async fn cat_created(&self, payload: CatEvent, context: EventContext) -> Result<()> {
        let _emitter = context.get::<EventEmitter>()?;
        println!("created cat {}", payload.name);
        Ok(())
    }

    #[a3s_boot::on_event("cat.*")]
    async fn any_cat_event(&self, event: a3s_boot::EventEnvelope) -> Result<()> {
        println!("cat event {}", event.name());
        Ok(())
    }
}

#[derive(Debug)]
struct CatsModule {
    events: EventModule,
    listeners: Arc<CatsEvents>,
}

impl CatsModule {
    fn new() -> Self {
        let listeners = Arc::new(CatsEvents);
        let events = EventModule::in_process("events")
            .listeners(Arc::clone(&listeners).event_listeners());

        Self { events, listeners }
    }
}

impl a3s_boot::Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn a3s_boot::Module>> {
        vec![Arc::new(self.events.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::from_arc(Arc::clone(&self.listeners))])
    }
}

async fn app() -> Result<()> {
    let app = BootApplication::builder()
        .import(CatsModule::new())
        .build()?;

    let emitter = app.get::<EventEmitter>()?;
    emitter
        .emit(
            "cat.created",
            &CatEvent {
                name: "Milo".to_string(),
            },
        )
        .await?;
    Ok(())
}
```

`#[on_event]` handlers may accept no arguments, one typed payload decoded from
the event JSON, an `EventEnvelope`, an `EventContext`, or one event argument
plus `EventContext`. Use `EventModule::listener(...)` for inline listeners that
do not need an impl-level macro.

`EventModule::global()` exports the emitter across module boundaries, and
`EventModule::named(...)` can register a named emitter provider when a service
needs separate event channels.

## Health Checks

Enable the `health` feature to expose Terminus-style health checks through a
provider-backed `HealthCheckService`. `HealthModule::new(...)` registers the
service, registers async indicators, and contributes a JSON `GET /health` route
by default. The route returns HTTP 200 when every indicator is up and HTTP 503
when any indicator is down or returns an error.

```rust
use a3s_boot::{
    BootApplication, HealthIndicatorResult, HealthModule, Result,
};

fn app() -> Result<BootApplication> {
    BootApplication::builder()
        .import(
            HealthModule::new("health")
                .indicator("database", || async {
                    Ok(HealthIndicatorResult::up().with_detail_value("latency_ms", 2))
                })
                .indicator("cache", || async {
                    Ok(HealthIndicatorResult::up())
                }),
        )
        .build()
}
```

Use `without_route()` when the service should only be consumed by another
controller or host, `with_route("/ready")` for a custom endpoint, and
`named(...)` or `global()` when multiple modules need distinct health services
or shared readiness checks.

## Providers

Providers can be registered as owned singletons, factories, shared `Arc<T>`
values, factories that return `Arc<T>`, request-scoped providers, transient
providers, or aliases to existing providers. Provider tokens are unique inside a
module scope; different modules can declare the same token without colliding.
Importing modules can only see providers that imported modules explicitly
export.

Singleton providers are the default and are built once per module. Transient
providers are built for every resolution. Request-scoped providers are built
once per in-process request scope and are cached for that request, including
dependencies resolved inside another request-scoped provider factory. Outside a
request scope, request-scoped providers behave like a fresh resolution. Provider
aliases mirror Nest's `useExisting`: the alias token delegates to the target
token and preserves the target provider's scope.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, Module, ModuleRef,
    FromModuleRef, ProviderDefinition, ProviderToken, Result,
};

#[derive(Debug)]
struct Client;

#[derive(Debug)]
struct AppConfig {
    name: &'static str,
}

#[derive(Debug)]
struct Repository {
    client: Arc<Client>,
}

#[derive(Debug)]
struct Formatter {
    client: Arc<Client>,
}

#[derive(Debug)]
struct RequestContext {
    request_id: String,
}

impl FromModuleRef for Repository {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            client: module_ref.get::<Client>()?,
        })
    }
}

impl FromModuleRef for Formatter {
    fn from_module_ref(module_ref: &ModuleRef) -> Result<Self> {
        Ok(Self {
            client: module_ref.get::<Client>()?,
        })
    }
}

let providers = vec![
    ProviderDefinition::singleton(AppConfig { name: "cats" }),
    ProviderDefinition::factory_arc::<Client, _>(|_module_ref: &ModuleRef| {
        Ok(Arc::new(Client))
    }),
    ProviderDefinition::injectable::<Repository>(),
    ProviderDefinition::named_factory_arc::<Client, _>("readonly-client", |_| {
        Ok(Arc::new(Client))
    }),
    ProviderDefinition::named_alias(
        "primary-client",
        ProviderToken::of::<Client>(),
    ),
    ProviderDefinition::request_scoped::<RequestContext, _>(|_module_ref| {
        Ok(RequestContext {
            request_id: "generated-per-request".to_string(),
        })
    }),
    ProviderDefinition::transient_injectable::<Formatter>(),
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
    let primary = app.get_named::<Client>("primary-client")?;
    let missing = app.get_optional_named::<Client>("missing-client")?;

    let _ = (repository, readonly, primary, missing);
    Ok(())
}
```

Async provider factories mirror Nest's async providers. They are awaited while
the application graph is built, before controllers and routes resolve their
dependencies. Use `build_async()` or a `BootFactory::*_async(...)` method; the
sync `build()` path rejects async providers with a clear error:

```rust
use std::sync::Arc;

use a3s_boot::{BootApplication, Module, ModuleRef, ProviderDefinition, Result};

#[derive(Debug)]
struct DatabaseClient {
    url: String,
}

#[derive(Debug)]
struct Repository {
    client: Arc<DatabaseClient>,
}

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![
            ProviderDefinition::async_factory::<DatabaseClient, _, _>(|_module_ref| async {
                Ok(DatabaseClient {
                    url: "postgres://localhost/app".to_string(),
                })
            }),
            ProviderDefinition::factory::<Repository, _>(|module_ref: &ModuleRef| {
                Ok(Repository {
                    client: module_ref.get::<DatabaseClient>()?,
                })
            }),
        ])
    }
}

# async fn build_app() -> Result<()> {
let app = BootApplication::builder()
    .import(AppModule)
    .build_async()
    .await?;
let repository = app.get::<Repository>()?;
# let _ = repository;
# Ok(())
# }
```

Async provider factories are singleton-only because provider lookup is
synchronous after the graph has been built. Use request-scoped or transient
providers for cheap per-request/per-resolution values, and let those providers
depend on async-built singletons.

Request-scoped providers are available from handlers through the request's
module context:

```rust
#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::request_scoped::<RequestContext, _>(
            |_module_ref| {
                Ok(RequestContext {
                    request_id: "generated-per-request".to_string(),
                })
            },
        )])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/cats")?.get(
            "/",
            |request: BootRequest| async move {
                let context = request.get::<RequestContext>()?;
                Ok(BootResponse::json(&serde_json::json!({
                    "requestId": context.request_id,
                }))?)
            },
        )?])
    }
}
```

Use `*_scoped` route helpers when the handler itself should be rebuilt for each
request scope, similar to request-scoped Nest controllers:

```rust
#[derive(Debug)]
struct CatsController {
    context: Arc<RequestContext>,
}

impl CatsController {
    async fn find_all(&self, request: BootRequest) -> Result<BootResponse> {
        let same_context = request.get::<RequestContext>()?;
        Ok(BootResponse::json(&serde_json::json!({
            "controllerRequestId": self.context.request_id,
            "handlerRequestId": same_context.request_id,
        }))?)
    }
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.get_scoped("/", |module_ref| {
        let controller = Arc::new(CatsController {
            context: module_ref.get::<RequestContext>()?,
        });
        Ok(move |request: BootRequest| {
            let controller = Arc::clone(&controller);
            async move { controller.find_all(request).await }
        })
    })
}
```

Singleton providers can opt into Nest-style lifecycle hooks:

```rust
use a3s_boot::{
    BoxFuture, ModuleRef, ProviderDefinition, ProviderOnApplicationShutdown,
    ProviderOnModuleInit, Result,
};

#[derive(Debug)]
struct CatsService;

impl ProviderOnModuleInit for CatsService {
    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        Ok(())
    }
}

impl ProviderOnApplicationShutdown for CatsService {
    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async { Ok(()) })
    }
}

let provider = ProviderDefinition::singleton(CatsService)
    .with_on_module_init::<CatsService>()
    .with_on_application_shutdown::<CatsService>();
```

Lifecycle hooks require singleton scope. Request-scoped and transient providers
are created for request or resolution contexts, so they do not participate in
application startup or shutdown hooks.

## Testing Modules

`TestingModule` mirrors Nest's test-module workflow: assemble a module graph,
override providers before controllers are built, compile it, resolve providers
from the compiled graph, override route pipeline components, and call the app
in process without binding a socket. Use `compile_async()` when the test module
contains async provider factories.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, HttpMethod, Module, ModuleRef,
    ProviderDefinition, Result, RouteDefinition, TestingModule,
};

#[derive(Debug)]
struct CatsService {
    name: &'static str,
}

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "CatsModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(CatsService { name: "real" })])
    }

    fn controllers(&self, module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        let service = module_ref.get::<CatsService>()?;
        Ok(vec![ControllerDefinition::new("/cats")?.route(
            RouteDefinition::get("/", move |_| {
                let service = Arc::clone(&service);
                async move { BootResponse::json(&service.name) }
            })?,
        )?])
    }
}

# async fn test() -> Result<()> {
let module = TestingModule::builder()
    .import(CatsModule)
    .override_provider(ProviderDefinition::singleton(CatsService { name: "test-cat" }))
    .compile()?;

assert_eq!(module.get::<CatsService>()?.name, "test-cat");

let response = module
    .call(BootRequest::new(HttpMethod::Get, "/cats"))
    .await?;

assert_eq!(response.body_json::<String>()?, "test-cat");
# Ok(())
# }
```

Pipeline overrides use the original component type as the first generic
argument and the replacement value as the method argument:

```rust
let module = TestingModule::builder()
    .import(CatsModule)
    .override_guard::<AuthGuard, _>(AllowGuard)
    .override_interceptor::<TraceInterceptor, _>(NoopInterceptor)
    .override_filter::<HttpErrorFilter, _>(TestErrorFilter)
    .override_pipe::<ParseCatPipe, _>(PassThroughPipe)
    .compile()?;
```

## Discovery And Reflector

`DiscoveryService` creates a read-only snapshot of a built application. It can
inspect modules, local provider tokens, resolved HTTP routes, WebSocket
gateways, and microservice message patterns. `Reflector` provides convenient
route metadata lookups over the same snapshot, similar to Nest's reflector
pattern.

```rust
use a3s_boot::{
    BootApplication, BootResponse, ControllerDefinition, DiscoveryService,
    HttpMethod, Module, ModuleRef, ProviderDefinition, Result, RouteDefinition,
};
use serde_json::json;

#[derive(Debug)]
struct CatsService;

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "CatsModule"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(CatsService)])
    }

    fn controllers(&self, _module_ref: &ModuleRef) -> Result<Vec<ControllerDefinition>> {
        Ok(vec![ControllerDefinition::new("/cats")?
            .with_metadata_value("resource", json!("cats"))
            .route(
                RouteDefinition::get("/{id}", |_| async { Ok(BootResponse::text("cat")) })?
                    .with_tag("cats")
                    .with_operation_id("getCat")
                    .with_metadata_value("roles", json!(["admin"])),
            )?])
    }
}

let app = BootApplication::builder().import(CatsModule).build()?;
let discovery = DiscoveryService::from_app(&app)?;
let reflector = discovery.reflector();

assert!(discovery.module("CatsModule").is_some());
assert_eq!(
    discovery.routes_for_module("CatsModule")[0].path,
    "/cats/{id}"
);
assert_eq!(
    reflector.operation_id(HttpMethod::Get, "/cats/{id}"),
    Some("getCat")
);
assert_eq!(
    reflector.metadata_value(HttpMethod::Get, "/cats/{id}", "roles"),
    Some(&json!(["admin"]))
);
assert_eq!(reflector.routes_with_tag("cats").len(), 1);
```

Route metadata is also copied into `ExecutionContext`, so guards and
interceptors can enforce policy without coupling to a concrete router:

```rust
use a3s_boot::{BootResponse, ExecutionContext, RouteDefinition};
use serde_json::json;

let route = RouteDefinition::get("/admin", |_| async {
    Ok(BootResponse::text("admin"))
})?
.with_metadata_value("roles", json!(["admin"]))
.with_guard(|context: ExecutionContext| async move {
    let roles = context
        .metadata_as::<Vec<String>>("roles")?
        .unwrap_or_default();
    Ok(roles.iter().any(|role| role == "admin"))
});
```

`ExecutionContext` is protocol-neutral. HTTP guards receive it directly, and
WebSocket gateways or microservice message patterns can reuse the same guard
through `with_execution_guard(...)` or `use_global_execution_guard(...)`. Use
`ExecutionInterceptor` when the hook only needs before/after observation and
should work across HTTP, WebSocket, and transport handlers:

```rust
use a3s_boot::{
    BootResponse, BoxFuture, ExecutionContext, ExecutionProtocol, Guard,
    MessagePatternDefinition, Result, RouteDefinition, TransportMessage,
    TransportReply, WebSocketGatewayDefinition, WebSocketMessage,
};

#[derive(Clone)]
struct ProtocolGuard;

impl Guard for ProtocolGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        Box::pin(async move {
            Ok(matches!(
                context.protocol(),
                ExecutionProtocol::Http
                    | ExecutionProtocol::WebSocket
                    | ExecutionProtocol::Transport
            ))
        })
    }
}

let guard = ProtocolGuard;

let route = RouteDefinition::get("/cats", |_| async {
    Ok(BootResponse::text("cats"))
})?
.with_guard(guard.clone());

let gateway = WebSocketGatewayDefinition::new("/cats/ws")?
    .with_execution_guard(guard.clone())
    .subscribe("ping", |_| async {
        Ok(WebSocketMessage::text("pong", "ok"))
    })?;

let pattern = MessagePatternDefinition::request(
    "cat.find",
    |message: TransportMessage| async move { Ok(TransportReply::new(message.data)) },
)?
.with_execution_guard(guard);
```

The macro form mirrors Nest's `@SetMetadata()` usage:

```rust
# use a3s_boot::Result;
#[derive(Debug)]
struct CatsController;

#[a3s_boot::controller("/cats")]
#[a3s_boot::metadata("resource", "cats")]
impl CatsController {
    #[a3s_boot::get("/{id}")]
    #[a3s_boot::metadata("roles", ["admin"])]
    async fn find_one(&self) -> Result<String> {
        Ok("cat".to_string())
    }
}
```

## Module Encapsulation

Modules are provider visibility boundaries. A module can use its own providers
and the exported providers of its imports. Providers declared by an imported
module stay private unless that module returns their `ProviderToken` from
`exports()`.

```rust
use a3s_boot::{
    BootApplication, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
};
use std::sync::Arc;

#[derive(Debug)]
struct Config {
    database_url: String,
}

#[derive(Debug)]
struct Repository {
    config: Arc<Config>,
}

#[derive(Debug)]
struct ConfigModule;

impl Module for ConfigModule {
    fn name(&self) -> &'static str {
        "config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(Config {
            database_url: "postgres://localhost/app".to_string(),
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<Config>()])
    }
}

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(ConfigModule)]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<Repository, _>(|module_ref| {
            Ok(Repository {
                config: module_ref.get::<Config>()?,
            })
        })])
    }
}

let app = BootApplication::builder().import(CatsModule).build()?;
let repository = app.get::<Repository>()?;
```

A module can re-export an imported provider by listing the imported token in its
own `exports()`. Global modules expose their exported providers to other module
scopes after registration:

```rust
#[derive(Debug)]
struct GlobalConfigModule;

impl Module for GlobalConfigModule {
    fn name(&self) -> &'static str {
        "global-config"
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(Config {
            database_url: "postgres://localhost/app".to_string(),
        })])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<Config>()])
    }

    fn is_global(&self) -> bool {
        true
    }
}
```

Use `DynamicModule` when providers are assembled from runtime configuration:

```rust
use a3s_boot::DynamicModule;

fn config_module(database_url: String) -> DynamicModule {
    DynamicModule::new("runtime-config")
        .provider(ProviderDefinition::singleton(Config { database_url }))
        .export::<Config>()
        .global()
}
```

`BootApplication::get(...)` resolves from root module scopes and global exports.
Private providers from imported feature modules are not exposed unless they are
exported into a root-visible scope.

## Configuration

Enable the `config` feature to load typed configuration from ACL. `ConfigModule`
implements `Module`, registers the parsed config value as a provider, and
exports it to importing modules. This keeps configuration composition on the
same provider/module path as services and repositories.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, ConfigModule, Module, ModuleRef, ProviderDefinition, Result, Validate,
};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct AppConfig {
    database_url: String,
    port: u16,
}

impl Validate for AppConfig {
    fn validate(&self) -> Result<()> {
        if self.port == 0 {
            return Err(a3s_boot::BootError::BadRequest(
                "port must be non-zero".to_string(),
            ));
        }
        Ok(())
    }
}

#[derive(Debug)]
struct Repository {
    config: Arc<AppConfig>,
}

#[derive(Debug)]
struct CatsModule {
    config: ConfigModule<AppConfig>,
}

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.config.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<Repository, _>(|module_ref| {
            Ok(Repository {
                config: module_ref.get::<AppConfig>()?,
            })
        })])
    }
}

let config = ConfigModule::<AppConfig>::from_validated_acl_str(
    "app-config",
    r#"
        database_url = env("DATABASE_URL", "postgres://localhost/app")
        port = 3000
    "#,
)?;

let app = BootApplication::builder()
    .import(CatsModule { config })
    .build()?;
let repository = app.get::<Repository>()?;
```

ACL values are converted through serde into your config type. Top-level
attributes map to struct fields, unlabeled blocks map to nested structs, and
labeled blocks map to maps keyed by the first label:

```acl
database_url = "postgres://localhost/app"
features = ["http", "sse", "transport"]

limits {
    body_bytes = 4096
}

providers "openai" {
    api_key = env("OPENAI_API_KEY", "test-key")
    base_url = concat("https://", "api.openai.com", "/v1")
}
```

Use `ConfigModule::from_acl_file(...)` for file-backed configuration,
`ConfigModule::from_acl_str(...)` for embedded configuration, and
`ConfigModule::from_value(...)` for already-built values. Add `.named("token")`
to export a named provider, or `.global()` to make the config visible to every
module scope after registration.

## Caching

Enable the `cache` feature to register a typed cache provider. `CacheModule`
implements `Module`, exports `Cache`, and starts with an in-memory backend for
single-process services and tests. External stores can implement `CacheStore`
later without changing service code.

```rust
use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{
    BootApplication, Cache, CacheModule, CacheOptions, Module, ModuleRef, ProviderDefinition,
    Result,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct CatDto {
    id: String,
    name: String,
}

#[derive(Debug)]
struct CatsService {
    cache: Arc<Cache>,
}

impl CatsService {
    fn find_one(&self, id: &str) -> Result<CatDto> {
        self.cache.get_or_insert_with(format!("cat:{id}"), || {
            Ok(CatDto {
                id: id.to_string(),
                name: "Milo".to_string(),
            })
        })
    }
}

#[derive(Debug)]
struct CatsModule;

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(CacheModule::in_memory_with_options(
            "cache",
            CacheOptions::new().with_default_ttl(Duration::from_secs(60)),
        ))]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<CatsService, _>(|module_ref| {
            Ok(CatsService {
                cache: module_ref.get::<Cache>()?,
            })
        })])
    }
}

let app = BootApplication::builder().import(CatsModule).build()?;
let cats = app.get::<CatsService>()?;
let cat = cats.find_one("42")?;
```

Use `Cache::set(...)`, `get(...)`, `remove(...)`, and `clear(...)` for direct
operations. `Cache::get_or_insert_with(...)` caches computed values using the
module default TTL. Add `.named("token")` to `CacheModule` when a module needs
multiple cache providers, or `.global()` to make one cache visible to every
module scope after registration.

## Task Scheduling

Enable the `schedule` feature to register `ScheduleModule` and inject a
provider-backed `Scheduler`. The in-process backend supports the same core
shapes as Nest's `@Timeout`, `@Interval`, and `@Cron`; jobs are started during
application bootstrap and aborted during application shutdown.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, Module, ProviderDefinition, Result, ScheduleContext, ScheduleModule,
};

#[derive(Debug)]
struct CatsTasks;

#[a3s_boot::schedule]
impl CatsTasks {
    #[a3s_boot::interval("cats.refresh", 60000)]
    async fn refresh_cache(&self, context: ScheduleContext) -> Result<()> {
        let _run_count = context.run_count;
        Ok(())
    }

    #[a3s_boot::cron("cats.prune", "0 0 0 * * * *")]
    async fn prune_old_records(&self) -> Result<()> {
        Ok(())
    }

    #[a3s_boot::timeout("cats.warmup", 5000)]
    async fn warmup(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct CatsModule {
    tasks: Arc<CatsTasks>,
    schedule: ScheduleModule,
}

impl CatsModule {
    fn new() -> Self {
        let tasks = Arc::new(CatsTasks);
        let schedule = ScheduleModule::in_process("schedule")
            .jobs(Arc::clone(&tasks).scheduled_jobs());

        Self { tasks, schedule }
    }
}

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.schedule.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::from_arc(Arc::clone(&self.tasks))])
    }
}

let app = BootApplication::builder()
    .import(CatsModule::new())
    .build()?;
app.bootstrap().await?;
app.shutdown().await?;
```

`#[interval(60000)]` and `#[timeout(5000)]` infer the job name from the method
name. Add an explicit first string argument when the job needs a stable public
name for logs, metrics, or removal. The duration argument is milliseconds, which
matches Nest's schedule decorators. A scheduled method may accept no arguments
or one `ScheduleContext` argument.

The lower-level API remains available for jobs that should be registered
directly on the module or dynamically during application bootstrap:

```rust
use std::sync::Arc;
use std::time::Duration;

use a3s_boot::{
    BootApplication, BoxFuture, Module, ModuleRef, ProviderDefinition, Result, ScheduleModule,
    Scheduler,
};

#[derive(Debug)]
struct CatsService;

impl CatsService {
    async fn refresh_cache(&self) -> Result<()> {
        Ok(())
    }

    async fn prune_old_records(&self) -> Result<()> {
        Ok(())
    }

    async fn warmup(&self) -> Result<()> {
        Ok(())
    }
}

#[derive(Debug)]
struct CatsModule {
    schedule: ScheduleModule,
}

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.schedule.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(CatsService)])
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let scheduler = module_ref.get::<Scheduler>()?;
            let cats = module_ref.get::<CatsService>()?;

            // Nest @Interval("cats.refresh", 60000)
            scheduler.interval("cats.refresh", Duration::from_secs(60), {
                let cats = Arc::clone(&cats);
                move |_| {
                    let cats = Arc::clone(&cats);
                    async move { cats.refresh_cache().await }
                }
            })?;

            // Nest @Cron("0 0 0 * * * *")
            scheduler.cron("cats.prune", "0 0 0 * * * *", {
                let cats = Arc::clone(&cats);
                move |_| {
                    let cats = Arc::clone(&cats);
                    async move { cats.prune_old_records().await }
                }
            })?;

            // Nest @Timeout("cats.warmup", 5000)
            scheduler.timeout("cats.warmup", Duration::from_secs(5), move |_| {
                let cats = Arc::clone(&cats);
                async move { cats.warmup().await }
            })?;

            Ok(())
        })
    }
}

let app = BootApplication::builder()
    .import(CatsModule {
        schedule: ScheduleModule::in_process("schedule"),
    })
    .build()?;
app.bootstrap().await?;
app.shutdown().await?;
```

Use `ScheduleModule::in_process("schedule").interval(...)`,
`.timeout(...)`, `.cron(...)`, or `.jobs(...)` for jobs that can be declared
directly on the schedule module. Use injected `Scheduler` registration when a
job needs to be added after bootstrap has started. Add `.named("token")` when a
module needs multiple scheduler providers, or `.global()` to make one scheduler
visible to every module scope after registration.

## Queues

Enable the `queue` feature to register `QueueModule` and inject a
provider-backed `Queue`. The in-process backend is intended for tests,
embedded single-process workers, and adapter development; durable or distributed
backends can implement `QueueBackend` without changing service code.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, BoxFuture, Module, ModuleRef, ProviderDefinition, Queue, QueueJob,
    QueueModule, Result,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
struct EmailJob {
    to: String,
    subject: String,
}

#[derive(Debug)]
struct Mailer;

impl Mailer {
    async fn send(&self, job: EmailJob) -> Result<()> {
        let _ = job;
        Ok(())
    }
}

#[derive(Debug)]
struct MailModule {
    queue: QueueModule,
}

impl Module for MailModule {
    fn name(&self) -> &'static str {
        "mail"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.queue.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(Mailer)])
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        Box::pin(async move {
            let queue = module_ref.get::<Queue>()?;
            let mailer = module_ref.get::<Mailer>()?;

            // Nest queue processor equivalent for a named job.
            queue.process("email.send", move |job: QueueJob, _context| {
                let mailer = Arc::clone(&mailer);
                async move { mailer.send(job.data_as::<EmailJob>()?).await }
            })?;

            Ok(())
        })
    }
}

let app = BootApplication::builder()
    .import(MailModule {
        queue: QueueModule::in_process("mail-queue"),
    })
    .build()?;
let queue = app.get::<Queue>()?;

app.bootstrap().await?;
queue
    .enqueue(
        "email.send",
        &EmailJob {
            to: "milo@example.com".to_string(),
            subject: "Welcome".to_string(),
        },
    )
    .await?;
app.shutdown().await?;
```

Use `QueueModule::in_process("name").processor(...)` for processors that can
be declared directly on the queue module. Use injected `Queue::process(...)`
when a processor needs providers from the importing module. `Queue::enqueue(...)`
serializes payloads through serde JSON; processors can call
`QueueJob::data_as::<T>()` to decode typed payloads. Use `Queue::jobs()`,
`stats()`, and `failures()` for test assertions and local diagnostics. Add
`.named("token")` when a module needs multiple queue providers, or `.global()`
to make one queue visible to every module scope after registration.

## Logging

Enable the `logging` feature to register `LoggingModule` and inject a
provider-backed structured `Logger`. Boot ships a `NoopLogSink` and an
`InMemoryLogSink` for tests; production adapters can implement `LogSink` for
their preferred backend.

```rust
use std::sync::Arc;

use a3s_boot::{
    BootApplication, BootRequest, BootResponse, HttpMethod, InMemoryLogSink, LogFields, LogLevel,
    Logger, LoggingModule, Module, ModuleRef, ProviderDefinition, RequestLoggingInterceptor,
    Result, RouteDefinition,
};

#[derive(Debug)]
struct CatsService {
    logger: Arc<Logger>,
}

impl CatsService {
    fn create(&self, id: &str) -> Result<()> {
        self.logger.log_with_fields(
            LogLevel::Info,
            "cat created",
            LogFields::new().with("cat_id", id)?,
        )
    }
}

#[derive(Debug)]
struct CatsModule {
    logging: LoggingModule,
}

impl Module for CatsModule {
    fn name(&self) -> &'static str {
        "cats"
    }

    fn imports(&self) -> Vec<Arc<dyn Module>> {
        vec![Arc::new(self.logging.clone())]
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::factory::<CatsService, _>(|module_ref| {
            let logger = module_ref.get::<Logger>()?;
            Ok(CatsService {
                logger: Arc::new(logger.child("cats.service")),
            })
        })])
    }
}

let sink = InMemoryLogSink::new();
let logger = Logger::new(sink.clone()).with_target("app");
let request_logger = Arc::new(logger.clone().child("http"));

let app = BootApplication::builder()
    .use_global_interceptor(RequestLoggingInterceptor::new(request_logger))
    .import(CatsModule {
        logging: LoggingModule::from_logger("logging", logger),
    })
    .route(RouteDefinition::post("/cats", |_| async {
        Ok(BootResponse::text_with_status(201, "ok"))
    })?)
    .build()?;

app.call(BootRequest::new(HttpMethod::Post, "/cats")).await?;
let records = sink.records()?;
```

`Logger::with_target(...)` and `Logger::child(...)` keep service, request, and
worker logs separate without changing sinks. Queue processors, scheduled jobs,
transport handlers, and WebSocket gateways can inject the same `Logger`
provider through `ModuleRef`. `RequestLoggingMiddleware` logs incoming requests
before the pipeline; `RequestLoggingInterceptor` logs route start/completion and
response status after the handler.

## Middleware

Middleware runs after route matching and path parameter decoding, but before
guards, interceptor `before` hooks, pipes, validation, and handlers. A
middleware can mutate the `BootRequest` and continue, or short-circuit with a
`BootResponse`.

```rust
use a3s_boot::{
    BootApplication, BootRequest, BootResponse, ControllerDefinition, MiddlewareOutcome,
    Result, RouteDefinition,
};

fn app() -> Result<BootApplication> {
    let controller = ControllerDefinition::new("/cats")?
        .with_middleware(|request: BootRequest| async move {
            Ok(MiddlewareOutcome::next(
                request.with_header("x-controller", "cats"),
            ))
        })
        .get("/", |request: BootRequest| async move {
            Ok(BootResponse::text(
                request.header("x-controller").unwrap_or("missing"),
            ))
        })?;

    BootApplication::builder()
        .use_global_middleware(|request: BootRequest| async move {
            Ok(MiddlewareOutcome::next(
                request.with_header("x-global", "boot"),
            ))
        })
        .route(
            RouteDefinition::get("/health", |_| async {
                Ok(BootResponse::text("ok"))
            })?
            .with_middleware(|request: BootRequest| async move {
                if request.header("x-block").is_some() {
                    return Ok(MiddlewareOutcome::response(
                        BootResponse::text_with_status(403, "blocked"),
                    ));
                }
                Ok(MiddlewareOutcome::next(request))
            }),
        )
        .route(controller.routes()[0].clone())
        .build()
}
```

Global middleware is prepended to module middleware, then controller middleware,
then route middleware. Module middleware is returned from `Module::middleware()`
and applies to controllers and direct routes declared by that module. Errors
returned by middleware go through exception filters; short-circuit responses
skip the remaining request pipeline. Adapters still run their own request
validation before Boot middleware executes.

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
helpers remain available for focused checks. Error responses can reuse the
framework's standard HTTP error mapping. `BootErrorKind` and
`catch_errors(...)` provide Nest-style catch filters for selected error kinds;
use `with_catch_filter(...)` or `#[use_filter(catch_errors(...))]` when a filter
should only handle errors such as `BootErrorKind::BadRequest`. Routes can also
attach static response headers or redirect successful handler results
declaratively, mirroring Nest's `@Header()` and `@Redirect()` decorators while
keeping the behavior adapter-neutral:

```rust
use a3s_boot::{BootErrorKind, BootResponse, Result, RouteDefinition};

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

fn cached_route() -> Result<RouteDefinition> {
    RouteDefinition::get("/cached", |_| async { Ok(BootResponse::text("ok")) })
        .map(|route| route.with_response_header("cache-control", "max-age=60"))
}

fn moved_route() -> Result<RouteDefinition> {
    RouteDefinition::get("/old", |_| async { Ok(BootResponse::text("ignored")) })
        .map(|route| route.with_redirect_status(301, "/new"))
}

fn bad_request_route() -> Result<RouteDefinition> {
    RouteDefinition::get("/bad", |_| async {
        Err(a3s_boot::BootError::BadRequest("invalid cat".to_string()))
    })
    .map(|route| {
        route.with_catch_filter([BootErrorKind::BadRequest], |_, error| async move {
            Ok(Some(BootResponse::text(error.to_string()).with_status(400)))
        })
    })
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

The macro form is available on controller route methods:

```rust
# use a3s_boot::Result;
#[derive(Debug)]
struct CatsController;

#[a3s_boot::controller("/cats")]
impl CatsController {
    #[a3s_boot::get("/cached")]
    #[a3s_boot::header("cache-control", "max-age=60")]
    async fn cached(&self) -> Result<String> {
        Ok("cat".to_string())
    }

    #[a3s_boot::get("/old")]
    #[a3s_boot::redirect("/cats/new", status = 301)]
    async fn moved(&self) -> Result<String> {
        Ok("ignored".to_string())
    }
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

## Serialization

Boot serialization mirrors Nest's `ClassSerializerInterceptor` and
`@SerializeOptions` shape, but works on adapter-neutral `BootResponse` values.
Register `SerializationInterceptor` globally, then attach
`SerializationOptions` to routes or controllers. The interceptor only rewrites
JSON response bodies; text, raw bytes, redirects, empty responses, and SSE
streams pass through unchanged.

```rust
use a3s_boot::{
    BootApplication, BootRequest, ControllerDefinition, Result, RouteDefinition,
    SerializationInterceptor, SerializationOptions,
};
use serde_json::json;

let app = BootApplication::builder()
    .use_global_serialization()
    .route(
        RouteDefinition::get_json("/users/{id}", |request: BootRequest| async move {
            Ok(json!({
                "id": request.param("id").unwrap_or("unknown"),
                "email": "milo@example.com",
                "password": "secret",
                "nickname": null
            }))
        })?
        .with_serialization(
            SerializationOptions::new()
                .exclude_field("password")
                .skip_null_fields(),
        ),
    )
    .build()?;
```

Controller-level serialization options are inherited by routes that do not set
their own options:

```rust
use a3s_boot::{BootRequest, ControllerDefinition, Result, SerializationOptions};
use serde_json::json;

fn users_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/users")?
        .with_serialization(SerializationOptions::new().include_fields(["id", "email"]))
        .get_json("/{id}", |request: BootRequest| async move {
            Ok(json!({
                "id": request.param("id").unwrap_or("unknown"),
                "email": "milo@example.com",
                "password": "secret"
            }))
        })
}
```

The macro form mirrors Nest's `@SerializeOptions()`. Use `include`, `exclude`,
and `skip_null` on a controller impl or on an individual route:

```rust
use a3s_boot::{controller, get, Result};
use serde_json::{json, Value};

#[derive(Debug)]
struct UsersController;

#[controller("/users")]
#[serialize(exclude = ["password"], skip_null)]
impl UsersController {
    #[get("/{id}")]
    async fn user(&self) -> Result<Value> {
        Ok(json!({
            "id": "u1",
            "email": "milo@example.com",
            "password": "secret",
            "nickname": null
        }))
    }

    #[get("/{id}/public")]
    #[serialize(include = ["id", "email"])]
    async fn public_user(&self) -> Result<Value> {
        Ok(json!({
            "id": "u1",
            "email": "milo@example.com",
            "password": "secret"
        }))
    }
}
```

Use `SerializationInterceptor::with_options(...)` when the same default policy
should apply to every JSON response, even routes without explicit metadata:

```rust
use a3s_boot::{BootApplication, SerializationInterceptor, SerializationOptions};

let app = BootApplication::builder()
    .use_global_interceptor(SerializationInterceptor::with_options(
        SerializationOptions::new().exclude_fields(["password", "token"]),
    ))
    .build()?;
```

`include_fields(...)` keeps only the named top-level fields on a JSON object or
each object in a top-level JSON array. `exclude_fields(...)` removes named
top-level fields, and `skip_null_fields()` drops null top-level fields. When a
serialized response had a `Content-Length`, Boot updates it after rewriting the
body.

## Compression

Enable the `compression` feature to use `CompressionInterceptor`, a Nest-style
response compression hook for gzip. It runs after handlers and other route
interceptors, checks the request `Accept-Encoding` header, skips responses that
are already encoded or too small, and leaves SSE streams unchanged.

```rust
use a3s_boot::{
    BootApplication, BootResponse, CompressionOptions, Result, RouteDefinition,
};

fn app() -> Result<BootApplication> {
    BootApplication::builder()
        .use_global_compression(CompressionOptions::new().with_min_size(512))
        .route(RouteDefinition::get("/report", |_| async {
            Ok(BootResponse::text("large report body"))
        })?)
        .build()
}
```

Clients opt in with `Accept-Encoding: gzip`. Compressed responses include
`Content-Encoding: gzip` and `Vary: accept-encoding`. If the response had a
`Content-Length`, Boot rewrites it to the compressed body length.

## File Upload

Enable the `file-upload` feature to parse `multipart/form-data` bodies from
`BootRequest`. This mirrors Nest's file upload workflow while keeping the core
adapter-neutral: adapters only need to preserve the request body and headers.

```rust
use a3s_boot::{
    BootRequest, BootResponse, ControllerDefinition, MultipartOptions, Result,
};

fn uploads_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/uploads")?.post("/", |request: BootRequest| async move {
        let form = request
            .multipart_form_with_options(
                MultipartOptions::new()
                    .with_max_body_size(2 * 1024 * 1024)
                    .with_max_file_size(1024 * 1024)
                    .with_max_files(4),
            )
            .await?;

        let title = form.field("title").map(|field| field.value()).unwrap_or("");
        let avatar = form.file("avatar").ok_or_else(|| {
            a3s_boot::BootError::BadRequest("avatar is required".to_string())
        })?;

        Ok(BootResponse::text(format!(
            "title={title}, file={}, bytes={}",
            avatar.file_name(),
            avatar.size()
        )))
    })
}
```

`MultipartForm` separates text fields from files. Use `field(...)`,
`field_values(...)`, `file(...)`, and `files_by_name(...)` for lookup.
`MultipartOptions` can limit total body size, per-field size, per-file size,
field count, and file count. Non-multipart requests return
`BootError::UnsupportedMediaType`; malformed multipart bodies and invalid text
fields return `BootError::BadRequest`; limit failures return
`BootError::PayloadTooLarge`.

## Security Helpers

Enable the `security` feature to use Nest-style security hooks through the
same middleware, guard, and interceptor pipeline as the rest of Boot.
`use_global_cors(...)` registers CORS response headers and generated hidden
`OPTIONS` preflight routes for known route shapes. `use_global_security_headers`
adds helmet-like response headers when handlers have not set them. CSRF and
rate limiting are ordinary guards, so they can be global, controller-level, or
route-level.

```rust
use a3s_boot::{
    BootApplication, BootResponse, CorsOptions, CsrfOptions, HttpMethod,
    RateLimitOptions, Result, RouteDefinition, SecurityHeadersOptions,
};
use std::time::Duration;

fn app() -> Result<BootApplication> {
    BootApplication::builder()
        .use_global_cors(
            CorsOptions::new()
                .allow_origin("https://console.example")
                .allow_methods([HttpMethod::Get, HttpMethod::Post])
                .allow_headers(["content-type", "x-csrf-token"])
                .allow_credentials()
                .with_max_age(600),
        )
        .use_global_security_headers(
            SecurityHeadersOptions::new()
                .with_content_security_policy("default-src 'self'")
                .with_strict_transport_security("max-age=31536000"),
        )
        .use_global_csrf(CsrfOptions::new())
        .use_global_rate_limit(
            RateLimitOptions::new()
                .with_max_requests(120)
                .with_window(Duration::from_secs(60))
                .with_key_header("x-forwarded-for"),
        )
        .route(RouteDefinition::get("/cats", |_| async {
            Ok(BootResponse::text("Milo"))
        })?)
        .build()
}
```

CSRF checks protect `POST`, `PUT`, `PATCH`, and `DELETE` by default. The guard
compares the `x-csrf-token` header with the `csrf-token` cookie and returns
HTTP 403 for missing or mismatched tokens. The rate limit guard uses an
in-memory fixed window and returns HTTP 429 after the configured request count;
use it for local services, tests, and adapter-neutral policy wiring before
plugging in a distributed backend.

## Sessions

Enable the `session` feature to use provider-backed sessions, similar to Nest's
session middleware setup. `use_global_session_module(...)` imports the session
provider, runs `SessionMiddleware` before handlers, and runs
`SessionCookieInterceptor` after handlers so a session cookie is only written
when session data exists.

```rust
use a3s_boot::{
    BootApplication, BootRequest, BootResponse, Result, RouteDefinition,
    SessionManager, SessionModule, SessionOptions,
};
use std::time::Duration;

fn app() -> Result<BootApplication> {
    let sessions = SessionManager::in_memory(
        SessionOptions::new()
            .with_cookie_name("sid")
            .with_ttl(Duration::from_secs(60 * 60)),
    );
    let login_sessions = sessions.clone();

    BootApplication::builder()
        .use_global_session_module(SessionModule::from_manager("sessions", sessions))
        .route(RouteDefinition::post("/login", move |request: BootRequest| {
            let sessions = login_sessions.clone();
            async move {
                let session_id = sessions.require_session_id(&request)?;
                sessions.set(&session_id, "user_id", &"u1")?;
                Ok(BootResponse::text("logged in"))
            }
        })?)
        .build()
}
```

`SessionOptions` controls the cookie name, TTL, path, domain, `HttpOnly`,
`Secure`, `SameSite`, and rolling-cookie behavior. The default store is
in-memory for tests and single-process services; production adapters can provide
a custom `SessionStore`.

## Params And Query

Boot keeps route params adapter-neutral. Use whole `{name}` segments in routes
and read decoded values from `BootRequest` one at a time or as a typed DTO; use
a final `{*path}` segment for catch-all routes that capture zero or more
trailing segments. Query strings can be read as raw single values, repeated
values, or decoded into a typed DTO. Parameter names must be non-empty,
well-formed, and unique after controller and global prefixes are applied. Route
definitions and prefixes are path-only and reject query or fragment markers;
read query values from the request instead. Invalid percent encoding and
invalid UTF-8 in decoded params or query values map to `BootError::BadRequest`.
Prefer `query_value(...)` and `query_values(...)` when the handler should reject
malformed query strings; use `query_pairs(...)` when the handler or adapter
needs every decoded query pair, including repeated keys.
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
use a3s_boot::{BootRequest, BootResponse, CookieOptions, CookieSameSite, HttpMethod};
use std::time::Duration;

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
    .with_cookie(
        "session",
        "abc",
        CookieOptions::new()
            .with_path("/")
            .with_max_age(Duration::from_secs(3600))
            .with_http_only(true)
            .with_secure(true)
            .with_same_site(CookieSameSite::Lax),
    )
    .unwrap()
    .with_cookie("theme", "dark", CookieOptions::new())
    .unwrap();

assert_eq!(
    response.header_values("set-cookie"),
    [
        "session=abc; Path=/; Max-Age=3600; HttpOnly; Secure; SameSite=Lax",
        "theme=dark; Path=/"
    ]
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

let logout = BootResponse::no_content()
    .delete_cookie("session", CookieOptions::new().with_path("/"))
    .unwrap();

assert_eq!(
    logout.header_values("set-cookie"),
    ["session=; Path=/; Max-Age=0"]
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
| Testing module | Test-only module compilation with provider overrides and in-process calls |
| Discovery/Reflector | Runtime snapshots and metadata lookup for modules, routes, gateways, and message patterns |
| HTTP adapter | Replaceable backend adapter; Axum is the first implementation |
| Controller | Typed request handlers grouped by route prefix |
| Provider | Injectable service or repository dependency |
| Middleware | Request inspection, mutation, and short-circuiting before guards and pipes |
| Guard | Request authorization and policy gate |
| Interceptor | Cross-cutting request/response behavior at global, controller, or route scope |
| Pipe | Request validation and transformation |
| WebSocket gateway | Event-based bidirectional message handlers |
| Message transport | Adapter-neutral request-response and event-only message patterns |
| Event emitter | Optional provider-backed in-process application events |
| Health check | Optional provider-backed readiness/liveness reports |
| Configuration | Optional ACL-backed typed providers through `ConfigModule` |
| Cache | Optional typed cache provider through `CacheModule` |
| Scheduler | Optional provider-backed task scheduling through `ScheduleModule` |
| Queue | Optional provider-backed background jobs through `QueueModule` |
| Logger | Optional provider-backed structured logging through `LoggingModule` |
| Compression | Optional gzip response compression through `CompressionInterceptor` |
| File upload | Optional multipart form parsing through `BootRequest` helpers |
| Security | Optional CORS, security headers, CSRF, and rate limiting helpers |
| Session | Optional provider-backed session store, middleware, and cookie persistence |
| Response cookies | Typed `Set-Cookie` and delete-cookie helpers on `BootResponse` |
| API versioning | URI, header, or media type version matching through route metadata |
| Serialization | JSON response shaping through `SerializationInterceptor` metadata |
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
├── cache.rs      # Optional typed cache module and in-memory store
├── compression.rs # Optional gzip response compression interceptor
├── config.rs     # Optional ACL-backed typed configuration module
├── discovery.rs  # Runtime discovery snapshots and metadata reflector
├── events.rs     # Optional in-process application event emitter module
├── health.rs     # Optional provider-backed health check module
├── http/         # Adapter-neutral request, response, methods, and query parsing
├── logging.rs    # Optional provider-backed structured logging module
├── module/       # Module trait, dynamic modules, exports, and lifecycle hooks
├── pipeline/     # Middleware, pipes, guards, interceptors, filters, and execution context
├── provider/     # Provider tokens, definitions, and ModuleRef container
├── queue.rs      # Optional provider-backed queue and in-process backend
├── routing/      # Route handlers, controllers, route execution, and path matching
├── schedule.rs   # Optional provider-backed scheduler and in-process backend
├── security.rs   # Optional CORS, security headers, CSRF, and rate limiting helpers
├── serialization.rs # Adapter-neutral JSON response shaping interceptor
├── session.rs    # Optional provider-backed session store and cookie pipeline
├── testing.rs    # Nest-style test module builder and compiled testing module
├── transport/    # Adapter-neutral microservice message patterns and transports
├── validation.rs # DTO validation trait and route validation hooks
├── versioning.rs # Adapter-neutral API versioning strategies and route metadata
├── websocket/    # Adapter-neutral WebSocket gateways, messages, and pipeline hooks
├── error.rs
├── file_upload.rs # Optional multipart form and file upload helpers
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
