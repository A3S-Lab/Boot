# A3S Boot Nest Parity Roadmap

This roadmap tracks the work needed to move `a3s-boot` from a Nest-inspired
HTTP framework slice toward a fuller Rust equivalent of the high-value Nest.js
developer experience.

The goal is not to copy TypeScript runtime reflection. A3S Boot should keep
Rust's explicit types, compile-time macros, adapter-neutral core, and small
runtime surface. Nest parity means preserving the workflows users expect:
module boundaries, injectable services, declarative controllers, request
pipelines, generated API documentation, and transport integrations.

Official Nest.js areas used as reference:

- Modules: https://docs.nestjs.com/modules
- Providers: https://docs.nestjs.com/providers
- Controllers: https://docs.nestjs.com/controllers
- Middleware: https://docs.nestjs.com/middleware
- Pipes and validation: https://docs.nestjs.com/pipes and https://docs.nestjs.com/techniques/validation
- Guards, interceptors, filters: https://docs.nestjs.com/guards, https://docs.nestjs.com/interceptors, https://docs.nestjs.com/exception-filters
- OpenAPI: https://docs.nestjs.com/openapi/introduction
- WebSockets: https://docs.nestjs.com/websockets/gateways
- Microservices: https://docs.nestjs.com/microservices/basics
- Techniques: https://docs.nestjs.com/techniques/configuration

## Current Baseline

Implemented today:

- `Module` with imports, providers, controllers, direct routes, and lifecycle hooks.
- `ProviderDefinition` and `ModuleRef` for typed provider registration,
  singleton/request/transient lifecycle scopes, singleton provider lifecycle
  hooks, and lookup.
- `ControllerDefinition` and `RouteDefinition` for HTTP route groups.
- Nest-style attribute macros: `#[injectable]`, `#[controller]`, `#[get]`,
  `#[post]`, `#[put]`, `#[patch]`, `#[delete]`, `#[sse]`, raw route mode, and
  method argument extractors including `#[body]`, `#[request]`,
  `#[param("name")]`, `#[params]`, `#[query]`, `#[query("name")]`,
  `#[header("name")]`, `#[headers]`, `#[host_param("name")]`, and `#[ip]`,
  plus `#[host]` for host-scoped controllers and routes, `#[metadata]` for
  Nest-style custom route/controller metadata and `#[http_code]` for Nest-style
  response status metadata, `#[header]` for response headers, and `#[redirect]`
  for redirect responses.
- Host-scoped HTTP routes with `RouteDefinition::with_host(...)` and
  `ControllerDefinition::with_host(...)` for Nest-style host-based controllers.
- API versioning macros: `#[version]`, `#[versions]`, and
  `#[version_neutral]` at controller and route scope.
- Serialization macros with `#[serialize(include = [...], exclude = [...],
  skip_null)]` at controller and route scope.
- Nest-style generic pipeline macros: `#[use_guard]`, `#[use_interceptor]`,
  `#[use_filter]`, and `#[use_pipe]` at controller and route scope.
- JSON body and JSON response helpers.
- SSE responses with `SseEvent`, `SseStream`, `BootResponse::sse(...)`,
  `RouteDefinition::sse(...)`, `ControllerDefinition::sse(...)`, and Axum
  streaming support.
- Global, module, controller-level, and route-level middleware plus global and
  controller-level `Pipe`, `Guard`, `Interceptor`, and `ExceptionFilter`
  support.
- Adapter-neutral request/response types, typed params/query helpers, header
  helpers, route matching, global prefixes, lifecycle hooks, and an Axum adapter.
- OpenAPI route metadata, schema-crate-neutral document generation from resolved
  routes, automatic path-parameter documentation, and optional
  `serve_openapi(...)` JSON route registration.
- Custom route/controller metadata through builders and `#[metadata]`,
  route-level override semantics, `ExecutionContext` access for guards and
  interceptors, and typed `Reflector` lookup from discovery snapshots.
- DTO validation with `Validate`, body/query/params validation hooks, global,
  controller-level, route-level validation switches, and `#[validate]` /
  `#[skip_validation]` macros.
- Module-scoped provider registries, explicit provider exports, transitive
  re-exports, global module exports, and `DynamicModule` for runtime-built
  provider modules.
- Provider lifecycle scopes with default singleton providers, request-scoped
  providers cached per in-process request context, transient providers built per
  resolution, request-time lookup through `BootRequest`, and singleton provider
  startup/shutdown hooks.
- Provider aliases that mirror Nest custom provider `useExisting` semantics and
  preserve target provider scope.
- Request-scoped route/controller handler factories through `*_scoped` helpers.
- Middleware with request mutation, short-circuit responses, global/module/
  controller/route scopes, filter integration for errors, and adapter
  validation before middleware execution.
- WebSocket gateways with adapter-neutral messages and connections, gateway
  pipes/guards/interceptors, provider-backed handlers, Nest-style gateway
  macros, and Axum WebSocket route registration.
- Microservice transports with adapter-neutral `TransportMessage` /
  `TransportReply`, request-response and event-only message patterns,
  provider-backed handlers, validation helpers, transport pipes/guards/
  interceptors, Nest-style message macros, and an in-process transport.
- ACL-backed typed configuration modules with `ConfigModule`, named/global
  provider exports, environment/default function support, and validation hooks.
- Typed cache modules with `CacheModule`, `Cache`, in-memory storage,
  default TTLs, named/global provider exports, and cache-store abstraction.
- Provider-backed task scheduling with `ScheduleModule`, `Scheduler`,
  in-process timeout/interval/cron jobs, named/global provider exports,
  Nest-style `#[schedule]` / `#[cron]` / `#[interval]` / `#[timeout]` macros,
  and lifecycle-managed shutdown.
- Provider-backed queues with `QueueModule`, `Queue`, in-process background
  processors, typed serde JSON payloads, named/global provider exports, and
  lifecycle-managed workers.
- Provider-backed structured logging with `LoggingModule`, `Logger`, pluggable
  sinks, in-memory test capture, request middleware/interceptor helpers, and
  worker-friendly injection through the same provider graph.
- API versioning with URI, header, and media type strategies; route-level and
  controller-level version metadata; default versions; and version-neutral
  routes.
- JSON response serialization with `SerializationInterceptor`, route/controller
  `SerializationOptions`, include/exclude field shaping, null skipping, and
  content-length updates after body rewriting.
- Optional gzip response compression with `CompressionInterceptor`,
  `CompressionOptions`, `Accept-Encoding` negotiation, `Vary` handling, and
  content-length updates.
- Optional multipart file upload helpers with `BootRequest::multipart_form`,
  `MultipartOptions`, text field and uploaded-file accessors, and body/count/
  per-field/per-file limits.

## Priority Order

1. Parameter extraction macros
2. OpenAPI metadata and generator
3. Validation pipeline (implemented)
4. Module encapsulation, dynamic modules, and provider lifecycle scopes (implemented)
5. Middleware (implemented)
6. WebSocket gateways (implemented)
7. Microservice transports (implemented)
8. Technique modules: config, cache, schedule, queues, logging, versioning, file upload

This order maximizes developer-facing Nest familiarity before adding broad
transport integrations.

## Out Of Scope

GraphQL is intentionally out of scope for this roadmap. A3S Boot should focus
on HTTP, SSE, WebSocket gateways, message transports, and the Nest-style module
and controller experience. If GraphQL is ever needed, it should be evaluated as
a separate companion crate rather than part of the core parity plan.

## Milestone 1: Parameter Extraction Macros

Status: implemented.

Nest equivalent:

- `@Body()`
- `@Param("id")`
- `@HostParam("account")`
- `@Query()`
- `@Headers("x-request-id")`
- `@Ip()`
- `@Req()`

Proposed A3S Boot shape:

```rust
#[controller("/cats")]
impl CatsController {
    #[get("/{id}")]
    async fn find_one(
        &self,
        #[param("id")] id: String,
        #[query] query: FindCatQuery,
        #[header("x-request-id")] request_id: Option<String>,
        #[ip] ip: Option<String>,
    ) -> Result<CatDto> {
        self.cats.find_one(id, query, request_id, ip).await
    }

    #[get("/host")]
    #[host("{account}.example.com")]
    async fn host_scoped(
        &self,
        #[host_param("account")] account: String,
    ) -> Result<CatDto> {
        self.cats.find_for_account(account).await
    }

    #[post("/", status = 201)]
    async fn create(&self, #[body] dto: CreateCatDto) -> Result<CatDto> {
        self.cats.create(dto).await
    }
}
```

Completed tasks:

- Extend `a3s-boot-macros` to parse attributes on route method arguments.
- Support `#[body]`, `#[request]`, `#[param("name")]`, `#[params]`,
  `#[query]`, `#[query("name")]`, `#[header("name")]`, `#[headers]`,
  `#[host_param("name")]`, and `#[ip]`.
- Support host-scoped controller and route macros through `#[host("...")]`,
  plus explicit `RouteDefinition::with_host(...)` and
  `ControllerDefinition::with_host(...)` APIs.
- Generate a wrapper that receives `BootRequest`, extracts typed values, then
  calls the original method.
- Keep existing direct DTO body handlers working.
- Decide extraction errors:
  - missing required path/query/header values should map to `BootError::BadRequest`
  - type decode failures should map to `BootError::BadRequest`
  - optional values should use `Option<T>`
- Add macro compile errors for unsupported combinations, duplicate body args,
  or non-simple patterns.

Acceptance:

- Macro tests cover every extractor type.
- Existing macro tests still pass.
- README examples show `#[body]`, `#[param]`, and `#[query]`.
- `cargo test --test macros --test controllers` passes.

## Milestone 2: OpenAPI Metadata And Generator

Status: implemented.

Nest equivalent:

- `@nestjs/swagger`
- `@ApiTags`
- `@ApiOperation`
- `@ApiResponse`
- `@ApiParam`
- `@ApiQuery`
- `@ApiBearerAuth`

Proposed A3S Boot shape:

```rust
#[controller("/cats")]
#[tag("cats")]
impl CatsController {
    #[get("/{id}")]
    #[operation(summary = "Find a cat")]
    #[response(status = 200, ty = CatDto)]
    #[response(status = 404, description = "Cat not found")]
    async fn find_one(&self, #[param("id")] id: String) -> Result<CatDto> {
        self.cats.find_one(id).await
    }
}
```

Tasks:

- Add route metadata storage to `RouteDefinition`. (Implemented)
- Add metadata fields for tags, operation id, summary, description, params,
  query, request body, response bodies, status codes, auth requirements, and
  deprecation. (Core fields implemented)
- Add a schema abstraction that can use a crate such as `schemars` without
  coupling the core to it unless a feature is enabled. (Implemented with
  `OpenApiSchema` and optional `openapi-schemas`)
- Add `OpenApiDocument` generation from `BootApplication`. (Implemented)
- Add optional route to serve JSON, for example `/openapi.json`. (Implemented)
- Preserve adapter neutrality. (Implemented)
- Add Nest-style OpenAPI macros such as `#[tag]`, `#[operation]`,
  `#[response]`, and auth metadata attributes. (Implemented)
- Add optional schema component generation from `schemars`. (Implemented)

Acceptance:

- A sample controller can generate a valid OpenAPI 3 document.
- The generated document includes paths, methods, params, request body,
  responses, tags, and security metadata.
- JSON examples in README are generated from tested code paths.
- OpenAPI tests validate a representative document with `serde_json`.

## Milestone 3: Validation Pipeline

Nest equivalent:

- `ValidationPipe`
- transform and whitelist options
- DTO-level validation

Status: implemented.

Proposed A3S Boot shape:

```rust
#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
    age: Option<u8>,
}

impl Validate for CreateCatDto {
    fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty() {
            return Err(BootError::BadRequest("name is required".to_string()));
        }
        Ok(())
    }
}
```

Tasks:

- Add a small `Validate` trait in core or a `validation` feature. (Implemented
  in core)
- Integrate validation after DTO extraction and before handler invocation.
  (Implemented with route validation hooks that run after request pipes and
  before guards/handlers for routes carrying validation metadata)
- Support explicit validation pipe composition for projects that prefer a third
  party crate such as `garde` or `validator`. (Implemented through ordinary
  `Pipe` composition plus explicit `Validate` implementations)
- Support request body, params, and query validation. (Implemented)
- Add consistent validation error response mapping. (Implemented through
  `BootError::BadRequest` / HTTP 400)

Acceptance:

- Invalid JSON body DTOs return HTTP 400 with contextual messages. (Covered)
- Invalid query/param DTOs return HTTP 400. (Covered)
- Validation can be enabled globally, controller-level, and route-level.
  (Covered through `use_global_validation`, `ControllerDefinition::with_validation`,
  `RouteDefinition::with_validation`, and `#[validate]`)
- Validation does not run for raw handlers unless explicitly configured.
  (Covered)

## Milestone 4: Module Encapsulation, Dynamic Modules, And Provider Lifecycle Scopes

Nest equivalent:

- module `exports`
- re-exported modules
- global modules
- dynamic modules
- provider scopes: singleton, request, transient
- singleton provider lifecycle hooks
- request-scoped controllers
- provider aliases / `useExisting`

Status: implemented.

Current gap:

`a3s-boot` previously registered providers into one resolved application
container. Boot now creates module-scoped provider registries. A module can see
its own providers plus exported providers from imports and global modules.
Dynamic modules can produce imports, providers, exports, controllers, and routes
from runtime configuration. Provider definitions can also choose singleton,
request-scoped, or transient lifecycle behavior. Singleton providers can opt
into module init, application bootstrap, and application shutdown hooks.
Request-scoped handler factories rebuild route/controller state from the current
request's module context. Provider aliases let one token delegate to an existing
provider token without changing the target provider's lifecycle scope.

Tasks:

- Introduce module-scoped provider registries. (Implemented)
- Add explicit provider exports and imported-module visibility. (Implemented)
- Support re-exporting imported modules. (Implemented through transitive token
  exports)
- Add global modules for opt-in application-wide providers. (Implemented through
  `Module::is_global`)
- Add dynamic module builders for configuration-driven providers. (Implemented
  with `DynamicModule`)
- Preserve direct host access through `BootApplication::get(...)` where it makes
  sense, but avoid accidentally exposing private feature-module providers.
  (Implemented; root scopes and global exports are visible to the host)
- Add provider lifecycle scopes comparable to Nest singleton, request, and
  transient providers. (Implemented)
- Make request-scoped providers reuse one instance per request context,
  including dependencies resolved inside request-scoped provider factories.
  (Implemented)
- Add singleton provider lifecycle hooks for init, bootstrap, and shutdown.
  (Implemented)
- Add request-scoped route/controller handler factories. (Implemented)
- Add provider aliases comparable to Nest `useExisting`. (Implemented)

Acceptance:

- A provider declared but not exported by an imported module is not visible to
  the importing module. (Covered)
- Exported providers are visible transitively according to explicit imports.
  (Covered)
- Duplicate-provider checks respect module scope. (Covered)
- Existing simple module examples continue to work or have a documented migration.
  (Covered; root module providers remain visible through `BootApplication::get`)
- Transient providers are rebuilt for every resolution. (Covered)
- Request-scoped providers are cached per request and are isolated from other
  requests. (Covered)
- Singleton provider lifecycle hooks run with module lifecycle hooks and reject
  request/transient provider scopes. (Covered)
- Request-scoped controller handlers are rebuilt for each request and share the
  same request-scoped provider cache as `BootRequest::get(...)`. (Covered)
- Provider aliases resolve the same singleton instance, preserve request-scoped
  resolution, and reject alias cycles with contextual errors. (Covered)

## Milestone 5: Middleware

Nest equivalent:

- `NestMiddleware`
- `MiddlewareConsumer`
- route-scoped middleware

Status: implemented.

Tasks:

- Add middleware trait that can inspect/mutate `BootRequest` before pipes and
  guards. (Implemented)
- Allow middleware to short-circuit with `BootResponse`. (Implemented through
  `MiddlewareOutcome::Respond`)
- Support global, module/controller, and route-scoped registration.
  (Implemented)
- Preserve order: middleware, pipes, guards, interceptors, handler, filters.
  (Covered)
- Ensure adapter-level request validation remains before middleware.
  (Covered for Axum)

Acceptance:

- Middleware can add request headers or context values before a handler.
  (Covered)
- Middleware can reject a request before guards run. (Covered)
- Route-scoped middleware only applies to matching route groups. (Covered)
- Pipeline ordering is covered by tests. (Covered)

## Milestone 6: WebSocket Gateways

Nest equivalent:

- `@WebSocketGateway()`
- `@SubscribeMessage()`
- gateway lifecycle hooks
- gateway guards/pipes/interceptors

Status: implemented.

Tasks:

- Define adapter-neutral WebSocket connection and message traits. (Implemented)
- Add gateway registration API. (Implemented through `WebSocketGatewayDefinition`,
  `Module::gateways`, `DynamicModule::gateway`, and application builder support)
- Add `#[websocket_gateway]` and `#[subscribe_message]` macros. (Implemented)
- Implement Axum WebSocket adapter support behind the `axum` feature.
  (Implemented)
- Reuse DI and pipeline concepts where possible. (Implemented with provider-backed
  gateways and gateway-specific pipe/guard/interceptor hooks)

Acceptance:

- A gateway can accept a WebSocket connection and dispatch messages by event
  name. (Covered)
- Gateway handlers can use providers. (Covered)
- Gateway guards/pipes/interceptors run in deterministic order. (Covered)
- Tests cover in-process adapter behavior and Axum integration. (Covered)

## Milestone 7: Microservice Transports

Status: implemented.

Nest equivalent:

- Redis, NATS, MQTT, RabbitMQ, Kafka, gRPC, and custom transports.
- message pattern handlers.

Tasks:

- Define an adapter-neutral message transport trait. (Implemented with
  `MessageTransport`)
- Add message pattern registration APIs and macros. (Implemented with
  `MessagePatternDefinition`, `Module::message_patterns`,
  `BootApplicationBuilder::message_pattern`, `#[message_controller]`,
  `#[message_pattern]`, and `#[event_pattern]`)
- Reuse provider lookup and pipeline primitives. (Implemented with
  provider-backed module registration plus transport-specific pipes, guards,
  interceptors, and payload validation)
- Start with an in-process test transport before external brokers.
  (Implemented with `InProcessTransport`)
- Add one production transport only after the core contract is stable.

Acceptance:

- A module can register message handlers independently from HTTP routes.
  (Covered)
- Message handlers can use providers and validation. (Covered)
- Tests cover request-response and event-only patterns. (Covered)

## Milestone 8: Technique Modules

Nest equivalent areas:

- configuration (implemented)
- cache (implemented)
- task scheduling (implemented)
- queues (implemented)
- application events (implemented)
- health checks (implemented)
- logging (implemented)
- API versioning (implemented)
- serialization (implemented)
- compression (implemented)
- file upload (implemented)
- security helpers such as CORS, CSRF, helmet-like headers, and rate limiting
  (implemented)
- sessions (implemented)

Tasks:

- Prefer companion crates or feature modules over bloating the core.
- Keep configuration ACL-first.
- Define integration points through providers, middleware, guards, interceptors,
  and adapters.

Acceptance:

- Each technique module has its own tests and docs.
- Core remains usable without the technique modules.
- Modules compose through the same provider and lifecycle APIs.
- Configuration can load ACL into typed providers, use environment defaults,
  and participate in module imports/exports. (Covered)
- Cache can register typed providers, cache serde values with TTL, and
  participate in module imports/exports. (Covered)
- Schedule can register typed providers, run timeout/interval/cron jobs through
  lifecycle-managed in-process tasks, expose Nest-style schedule macros, and
  participate in module imports/exports. (Covered)
- Queue can register typed providers, enqueue serde JSON jobs, run named
  processors through lifecycle-managed in-process workers, and participate in
  module imports/exports. (Covered)
- Application events can register an in-process `EventEmitter` provider,
  dispatch typed JSON payloads to exact or wildcard listeners, expose
  Nest-style listener macros, and participate in module imports/exports.
  (Covered)
- Health checks can register provider-backed async indicators, expose a typed
  `HealthCheckService`, return JSON readiness reports, and map unhealthy
  reports to HTTP 503. (Covered)
- Logging can register typed providers, write structured records through
  pluggable sinks, capture records in tests, and expose request/worker logging
  integration points without forcing a concrete backend. (Covered)
- API versioning can route by URI segment, request header, or media type
  parameter; inherit controller versions; use default versions; expose
  version-neutral routes; reject duplicate routes only when version metadata
  overlaps; and expose Nest-style version macros. (Covered)
- Serialization can shape JSON response objects and arrays through global
  interceptors plus route/controller metadata, leave non-JSON responses
  unchanged, keep content length valid after rewriting, and expose Nest-style
  serialization macros. (Covered)
- Compression can gzip eligible responses when requested by `Accept-Encoding`,
  skip too-small, streaming, and already-encoded responses, set `Vary`, and keep
  content length valid after rewriting. (Covered)
- File upload can parse adapter-neutral multipart forms, expose repeated text
  fields and uploaded files, reject non-multipart or malformed requests, and
  enforce body, field, file, and count limits. (Covered)
- Security helpers can handle CORS preflight and actual response headers, add
  helmet-like response headers, reject invalid CSRF tokens on unsafe methods,
  and enforce in-memory fixed-window rate limits. (Covered)
- Sessions can register a provider-backed `SessionManager`, bind session ids
  before handlers, persist session cookies after handlers, and support
  in-memory or custom stores. (Covered)
- Response cookies can be written and expired through typed `BootResponse`
  helpers instead of hand-built `Set-Cookie` strings. (Covered)
- Testing utilities can compile Nest-style testing modules, override providers
  before controllers are built, resolve providers, and dispatch in-process
  requests. (Covered)
- Discovery and reflector utilities can snapshot modules, provider tokens,
  HTTP route metadata, WebSocket gateways, and message patterns from a built
  application. (Covered)

## Immediate Next Task

Continue the Nest framework parity audit and pick the next missing framework
module. Keep GraphQL out of scope.

Suggested implementation sequence:

1. Re-check remaining Nest framework capabilities against implemented Boot
   modules.
2. Define integration through providers, middleware, guards, interceptors, or
   adapters instead of adding one-off framework hooks.
3. Add crate-local tests and README examples for the chosen framework module.
4. Run:

```sh
cargo fmt --all
cargo test --test macros --test controllers
cargo test --all-targets
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
git diff --check
```
