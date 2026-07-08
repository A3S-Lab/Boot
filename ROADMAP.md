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
- Lifecycle events: https://docs.nestjs.com/fundamentals/lifecycle-events
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

- `Module` with imports, providers, controllers, direct routes, module route
  prefixes, and lifecycle hooks.
- `BootFactory` with NestFactory-style `create`, `create_application_context`,
  `create_microservice`, async provider-aware `create_async`,
  `create_application_context_async`, and `create_microservice_async`, managed
  `init`/`close`, signal-aware shutdown helpers, Nest-style
  `enable_shutdown_hooks`, `listen_with`, and hybrid microservice startup
  helpers.
- `ProviderDefinition` and `ModuleRef` for typed provider registration,
  singleton/request/transient lifecycle scopes, async singleton provider
  factories, singleton provider lifecycle hooks, lookup, `FromModuleRef`
  auto-wired provider factories, named or optional dependency resolution,
  `ProviderRef<T>` lazy provider handles for forward-reference-style
  dependencies, fresh resolution contexts, and dynamic injectable creation.
- `TestingModule` with provider overrides, async provider-aware
  `compile_async`, and typed route pipeline overrides for guards,
  interceptors, exception filters, and pipes.
- `ControllerDefinition` and `RouteDefinition` for HTTP route groups, including
  specificity-aware path params, catch-all route params, and Nest-style ALL
  method routes with exact-method precedence.
- Nest-style attribute macros: `#[module]`, `#[injectable]`, `#[controller]`,
  `#[all]`, `#[get]`, `#[post]`, `#[put]`, `#[patch]`, `#[delete]`, `#[sse]`,
  raw route mode, and method argument extractors including `#[body]`, `#[request]`,
  `#[param("name")]`, `#[params]`, `#[query]`, `#[query("name")]`,
  `#[header("name")]`, `#[headers]`, `#[host_param("name")]`, `#[ip]`, and
  custom `#[extract(...)]` request value binding. Single-value extractors can
  also run Nest-style parameter pipes through `pipe = <expr>`, for example
  `#[param("id", pipe = parse_cat_id)]` and
  `#[query("page", pipe = parse_page)]`, plus `#[host]` for
  host-scoped controllers and routes, `#[metadata]` for
  Nest-style custom route/controller metadata and `#[http_code]` for Nest-style
  response status metadata, `#[header]` for response headers, and `#[redirect]`
  for redirect responses. `#[injectable]` implements `FromModuleRef` for unit
  structs and named-field structs whose dependencies are `Arc<T>` or
  `Option<Arc<T>>`, with `#[inject("token")]` for named provider lookup.
  `#[module]` implements `Module` from Nest-style metadata lists for imports,
  providers, controllers, routes, gateways, message controllers, exports,
  route prefixes, and global modules.
- WebSocket lifecycle macros: `#[on_gateway_init]`,
  `#[on_gateway_connection]`, and `#[on_gateway_disconnect]`.
- Host-scoped HTTP routes with `RouteDefinition::with_host(...)` and
  `ControllerDefinition::with_host(...)` for Nest-style host-based controllers.
- API versioning macros: `#[version]`, `#[versions]`, and
  `#[version_neutral]` at controller and route scope.
- Serialization macros with `#[serialize(include = [...], exclude = [...],
  skip_null)]` at controller and route scope.
- Nest-style generic pipeline macros: `#[use_guard]`, `#[use_interceptor]`,
  `#[use_filter]`, and `#[use_pipe]` at controller and route scope.
- Nest-style catch-filter targeting with `#[catch]`, `BootErrorKind`,
  `catch_errors(...)`, `with_catch_filter(...)`, and
  `use_global_catch_filter(...)`.
- Nest-style built-in request value pipes with `ParseIntPipe`, `ParseBoolPipe`,
  `ParseFloatPipe`, `ParseArrayPipe`, `ParseEnumPipe`, `ParseUuidPipe`,
  `DefaultValuePipe`, and extractor-level `default = ...` fallbacks.
- JSON body and JSON response helpers.
- SSE responses with `SseEvent`, `SseStream`, `BootResponse::sse(...)`,
  `RouteDefinition::sse(...)`, `ControllerDefinition::sse(...)`, and Axum
  streaming support.
- Nest-style streamable file and download responses with `StreamableFile`,
  `StreamableFileOptions`, `BootResponse::streamable_file(...)`,
  `BootResponse::download(...)`, byte-stream support, content disposition, and
  Axum streaming support.
- Nest-style MVC view rendering with `ViewEngine`, `ViewRenderer`,
  `ViewModule`, `StringTemplateViewEngine`, `RouteDefinition::get_view(...)`,
  `ControllerDefinition::get_view(...)`, `BootResponse::html(...)`, and the
  `#[render("view")]` route macro.
- Global, module, controller-level, and route-level middleware plus Nest-style
  `MiddlewareConsumer` include/exclude route configuration, with global and
  controller-level `Pipe`, `Guard`, `Interceptor`, and `ExceptionFilter`
  support.
- Adapter-neutral request/response types, typed params/query helpers, typed
  single-value parsing helpers, header helpers, route matching, global prefixes,
  lifecycle hooks, and an Axum adapter.
- OpenAPI route metadata, schema-crate-neutral document generation from resolved
  routes, automatic path-parameter documentation, and optional
  `serve_openapi(...)` JSON route registration plus `serve_openapi_ui(...)`
  Swagger UI route registration.
- Custom route/controller metadata through builders and `#[metadata]`,
  route-level override semantics, protocol-neutral `ExecutionContext` access
  for HTTP, WebSocket, and transport guards/interceptors, and typed `Reflector`
  lookup from discovery snapshots.
- Nest-style runtime discovery and devtools-ready application graph snapshots
  for modules, imports, provider tokens, exports, route counts, WebSocket
  gateway counts, and microservice message pattern counts.
- Optional task-local request context with `RequestContext`, request id,
  path/param/query/header/metadata access, pipeline-local values, and auth
  principal propagation when authentication is enabled.
- DTO validation with `Validate`, body/query/params validation hooks, global,
  controller-level, route-level validation switches, and `#[validate]` /
  `#[skip_validation]` macros.
- Module-scoped provider registries, explicit provider exports, transitive
  re-exports, global module exports, module route prefixes, and `DynamicModule`
  for runtime-built provider modules, with provider-only lazy module loading
  and contextual module import cycle diagnostics.
- Provider lifecycle scopes with default singleton providers, request-scoped
  providers cached per in-process request context, transient providers built per
  resolution, async singleton provider factories awaited during async graph
  build, order-independent singleton provider graph initialization,
  request-time lookup through `BootRequest`, singleton/transient/request-scoped
  provider dependency cycle diagnostics, and singleton provider startup/shutdown
  hooks for module init, application bootstrap, module destroy, before
  application shutdown, and application shutdown, including OS signal labels
  from shutdown hooks.
- Provider aliases that mirror Nest custom provider `useExisting` semantics and
  preserve target provider scope.
- Lazy `ProviderRef<T>` handles that mirror the useful part of Nest
  `forwardRef(...)`: explicit delayed provider resolution without weakening
  normal cycle diagnostics.
- Request-scoped route/controller handler factories through `*_scoped` helpers.
- Middleware with request mutation, short-circuit responses, global/module/
  controller/route scopes, `MiddlewareConsumer::apply(...).for_routes(...)`
  include/exclude rules, filter integration for errors, and adapter validation
  before middleware execution.
- WebSocket gateways with adapter-neutral messages and connections, gateway
  init/connection/disconnect lifecycle hooks, pipes/guards/interceptors,
  provider-backed handlers, Nest-style gateway macros, and Axum WebSocket route
  registration.
- Microservice transports with adapter-neutral `TransportMessage` /
  `TransportReply`, request-response and event-only message patterns,
  provider-backed handlers, validation helpers, transport pipes/guards/
  interceptors, Nest-style message macros, an in-process transport, and an
  optional TCP transport for newline-delimited JSON message frames plus an
  optional Redis Pub/Sub transport and optional NATS request/reply and event
  subjects plus optional MQTT request/reply and event topics plus optional
  RabbitMQ request/reply and event queues plus optional Kafka request/reply and
  event topics plus optional gRPC unary request/reply and event calls.
- ACL-backed typed configuration modules with `ConfigModule`, named/global
  provider exports, environment/default function support, and validation hooks.
- Provider-backed outbound HTTP clients with `HttpModule`, `HttpService`,
  typed request/response helpers, base URL/default header/timeout options,
  named/global exports, async option factories, and replaceable backends.
- Optional CQRS buses with `CqrsModule`, typed `CommandBus`, `QueryBus`, and
  `EventBus`, module-scoped provider resolution through `CqrsContext`, duplicate
  command/query handler diagnostics, and multi-handler event publishing.
- Optional provider-backed authentication with `AuthModule`, `AuthService`,
  bearer or custom strategies, `AuthGuard`, route/controller auth metadata,
  roles/scopes checks, public-route bypass, and `BootRequest` principals.
- Optional provider-backed database facade with `DatabaseModule`, `Database`,
  backend and transaction traits, adapter-neutral statements/rows/results,
  named/global provider exports, and an in-memory backend for tests.
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
- Optional provider-backed static file serving with `StaticModule`,
  `StaticFileService`, GET/HEAD catch-all routes, index-file support, SPA
  fallback, cache-control headers, content-type detection, and traversal
  protection.

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
- Add optional Swagger UI route backed by a generated JSON document.
  (Implemented)
- Preserve adapter neutrality. (Implemented)
- Add Nest-style OpenAPI macros such as `#[tag]`, `#[operation]`,
  `#[response]`, and auth metadata attributes. (Implemented)
- Add optional schema component generation from `schemars`. (Implemented)

Acceptance:

- A sample controller can generate a valid OpenAPI 3 document.
- The generated document includes paths, methods, params, request body,
  responses, tags, and security metadata.
- A generated Swagger UI route can load the generated JSON document.
- JSON examples in README are generated from tested code paths.
- OpenAPI tests validate a representative document with `serde_json`.

## Milestone 3: Validation Pipeline

Nest equivalent:

- `ValidationPipe`
- transform and whitelist options
- DTO-level validation

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
  (Implemented with route validation hooks that run after guards, interceptor
  `before` hooks, and request pipes for routes carrying validation metadata)
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
- singleton provider lifecycle hooks, including shutdown phases
- `enableShutdownHooks`
- request-scoped controllers
- provider aliases / `useExisting`
- forward-reference-style provider dependencies
- lazy module loading

Current gap:

`a3s-boot` previously registered providers into one resolved application
container. Boot now creates module-scoped provider registries. A module can see
its own providers plus exported providers from imports and global modules.
Dynamic modules can produce imports, providers, exports, controllers, and routes
from runtime configuration. Provider definitions can also choose singleton,
request-scoped, or transient lifecycle behavior. Singleton providers can opt
into module init, application bootstrap, module destroy, before application
shutdown, and application shutdown hooks. Managed HTTP and microservice hosts
can enable Nest-style shutdown hooks so OS signals close the application through
the same signal-aware lifecycle phases.
Request-scoped handler factories rebuild route/controller state from the current
request's module context. Provider aliases let one token delegate to an existing
provider token without changing the target provider's lifecycle scope. Module
import cycles report the active module chain during sync and async application
graph builds. Singleton provider factories are initialized after all module
provider tokens are registered, so factories can depend on providers declared
later in the same module. `LazyModuleLoader` can load provider-only module
graphs on demand, reuse eagerly registered modules, and resolve async singleton
factories through `load_async(...)`. `ModuleRef` can resolve providers in a
fresh temporary request context and dynamically create unregistered
`FromModuleRef` values. `ProviderRef<T>` can capture a module context and
resolve a provider lazily, which gives Rust code an explicit
forward-reference-style escape hatch while keeping ordinary provider cycles
diagnostic.

Tasks:

- Introduce module-scoped provider registries. (Implemented)
- Add explicit provider exports and imported-module visibility. (Implemented)
- Support re-exporting imported modules. (Implemented through transitive token
  exports)
- Add global modules for opt-in application-wide providers. (Implemented through
  `Module::is_global`)
- Add dynamic module builders for configuration-driven providers. (Implemented
  with `DynamicModule`)
- Add module route prefixes comparable to Nest `RouterModule.register(...)`,
  including import-tree composition and `DynamicModule::route_prefix(...)`.
  (Implemented)
- Preserve direct host access through `BootApplication::get(...)` where it makes
  sense, but avoid accidentally exposing private feature-module providers.
  (Implemented; root scopes and global exports are visible to the host)
- Add provider lifecycle scopes comparable to Nest singleton, request, and
  transient providers. (Implemented)
- Make request-scoped providers reuse one instance per request context,
  including dependencies resolved inside request-scoped provider factories.
  (Implemented)
- Add singleton provider lifecycle hooks for init, bootstrap, module destroy,
  before application shutdown, and application shutdown. (Implemented)
- Add Nest-style shutdown hook enabling for managed HTTP and microservice
  hosts. (Implemented with `enable_shutdown_hooks(...)` and default
  `SIGINT`/`SIGTERM` support)
- Add request-scoped route/controller handler factories. (Implemented)
- Add provider aliases comparable to Nest `useExisting`. (Implemented)
- Add lazy provider handles comparable to the useful provider side of Nest
  `forwardRef(...)`. (Implemented with `ProviderRef<T>`)
- Add Nest-style `ModuleRef::resolve(...)` and `ModuleRef::create(...)`
  runtime APIs. (Implemented)
- Add contextual diagnostics for transient and request-scoped provider
  dependency cycles. (Implemented)
- Add contextual diagnostics for module import cycles. (Implemented)
- Add order-independent singleton provider graph initialization. (Implemented)
- Add provider-only lazy module loading with cached module refs. (Implemented)

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
- Singleton provider lifecycle hooks run with module lifecycle hooks, reject
  request/transient provider scopes, and receive explicit shutdown signal labels
  through signal-aware close helpers. (Covered)
- Managed HTTP and microservice hosts can close through signal-aware lifecycle
  hooks when a configured shutdown signal wins the serve race. (Covered)
- Request-scoped controller handlers are rebuilt for each request and share the
  same request-scoped provider cache as `BootRequest::get(...)`. (Covered)
- Provider aliases resolve the same singleton instance, preserve request-scoped
  resolution, and reject alias cycles with contextual errors. (Covered)
- `ProviderRef<T>` resolves lazily, can break an intentional singleton
  dependency cycle, supports named and optional macro injection, and preserves a
  captured request scope. (Covered)
- `ModuleRef::resolve(...)` creates a fresh resolution context for
  request-scoped dependency caches, and `ModuleRef::create(...)` can instantiate
  `FromModuleRef` values without registering them. (Covered)
- Transient and request-scoped provider cycles report the active token chain.
  (Covered)
- Module import cycles report the active module chain during sync and async
  builds. (Covered)
- Singleton provider factories can resolve dependencies declared later in the
  same module, including sync factories that depend on async-built singletons in
  async builds. (Covered)
- Lazy module loading returns cached module refs, reuses eagerly imported
  modules, resolves imports/exports, supports async singleton providers through
  `load_async(...)`, and does not register controllers, routes, gateways,
  middleware, message patterns, or lifecycle hooks. (Covered)

## Milestone 5: Middleware

Nest equivalent:

- `NestMiddleware`
- `MiddlewareConsumer`
- route-scoped middleware

Tasks:

- Add middleware trait that can inspect/mutate `BootRequest` before guards,
  interceptor `before` hooks, and pipes. (Implemented)
- Allow middleware to short-circuit with `BootResponse`. (Implemented through
  `MiddlewareOutcome::Respond`)
- Support global, module/controller, and route-scoped registration.
  (Implemented)
- Add Nest-style `MiddlewareConsumer` with `apply`, `exclude`, `for_routes`,
  and `for_all_routes` for module-scoped route selection. (Implemented)
- Preserve order: middleware, guards, interceptor `before` hooks, pipes,
  validation, handler, interceptor `after` hooks, filters. (Covered)
- Ensure adapter-level request validation remains before middleware.
  (Covered for Axum)

Acceptance:

- Middleware can add request headers or context values before a handler.
  (Covered)
- Middleware can reject a request before guards run. (Covered)
- Route-scoped middleware only applies to matching route groups. (Covered)
- Consumer route selectors support method-specific include/exclude rules and
  module-local or module-prefixed paths. (Covered)
- Pipeline ordering is covered by tests. (Covered)

## Milestone 6: WebSocket Gateways

Nest equivalent:

- `@WebSocketGateway()`
- `@SubscribeMessage()`
- gateway lifecycle hooks
- gateway guards/pipes/interceptors

Tasks:

- Define adapter-neutral WebSocket connection and message traits. (Implemented)
- Add gateway registration API. (Implemented through `WebSocketGatewayDefinition`,
  `Module::gateways`, `DynamicModule::gateway`, and application builder support)
- Add `#[websocket_gateway]` and `#[subscribe_message]` macros. (Implemented)
- Add gateway lifecycle hooks comparable to Nest `OnGatewayInit`,
  `OnGatewayConnection`, and `OnGatewayDisconnect`. (Implemented with
  `#[on_gateway_init]`, `#[on_gateway_connection]`,
  `#[on_gateway_disconnect]`, and explicit hook builders)
- Implement Axum WebSocket adapter support behind the `axum` feature.
  (Implemented)
- Reuse DI and pipeline concepts where possible. (Implemented with provider-backed
  gateways and gateway-specific pipe/guard/interceptor hooks)

Acceptance:

- A gateway can accept a WebSocket connection and dispatch messages by event
  name. (Covered)
- Gateway init, connection, and disconnect hooks run through explicit APIs,
  provider-backed macros, and application bootstrap. (Covered)
- Gateway handlers can use providers. (Covered)
- Gateway guards/interceptors/pipes run in Nest-style deterministic order.
  (Covered)
- Tests cover in-process adapter behavior and Axum integration. (Covered)

## Milestone 7: Microservice Transports

Nest equivalent:

- TCP, Redis, NATS, MQTT, RabbitMQ, Kafka, gRPC, and custom transports.
- message pattern handlers.

Tasks:

- Define an adapter-neutral message transport trait. (Implemented with
  `MessageTransport`)
- Add message pattern registration APIs and macros. (Implemented with
  `MessagePatternDefinition`, `Module::message_patterns`,
  `BootApplicationBuilder::message_pattern`, `#[message_controller]`,
  `#[message_pattern]`, and `#[event_pattern]`)
- Reuse provider lookup and pipeline primitives. (Implemented with
  provider-backed module registration plus transport-specific guards,
  interceptors, pipes, and payload validation)
- Start with an in-process test transport before external brokers.
  (Implemented with `InProcessTransport`)
- Add one production transport only after the core contract is stable.
  (Implemented first with optional `TcpTransport`, followed by optional
  `RedisTransport`, `NatsTransport`, `MqttTransport`, and
  `RabbitMqTransport`, `KafkaTransport`, and `GrpcTransport`.)

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
- CQRS command, query, and event buses (implemented)
- authentication strategies and guards (implemented)
- database providers and transactions (implemented)
- request context / AsyncLocalStorage-style request state (implemented)
- health checks (implemented)
- outbound HTTP client module (implemented)
- logging (implemented)
- API versioning (implemented)
- serialization (implemented)
- compression (implemented)
- MVC view rendering and `@Render()`-style responses (implemented)
- streamable file and download responses (implemented)
- file upload (implemented)
- static assets and SPA shells (implemented)
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
- HTTP clients can be registered as module providers, use base URL/default
  header/timeout options, send and decode JSON request/response bodies, support
  named/global exports, build options asynchronously, and swap backends in
  tests. (Covered)
- CQRS can register command, query, and event handlers; dispatch typed command
  and query results; publish typed events to multiple handlers; resolve providers
  from handler context; export buses globally; and reject duplicate command or
  query handlers. (Covered)
- Authentication can register bearer or custom strategies, select strategies
  from guard configuration or route metadata, attach principals to requests,
  allow public routes, and enforce role/scope metadata. (Covered)
- Database can register a provider-backed facade, use replaceable backends,
  execute statements, query adapter-neutral rows, run commit/rollback
  transactions, support named/global exports, and expose an in-memory backend
  for tests. (Covered)
- Request context can bind task-local request data across middleware, guards,
  interceptors, pipes, handlers, and called provider methods; expose request id,
  path params, query values, headers, metadata, pipeline-local values, and auth
  principal data when authentication is enabled. (Covered)
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
- Streamable file responses can wrap in-memory bytes or byte streams, set
  content type, content length, inline or attachment content disposition, and
  reach adapters as streamed bytes instead of SSE events. (Covered)
- View rendering can register a provider-backed renderer, render serializable
  route return values into HTML responses, use module imports/exports, expose
  explicit route/controller helpers, and mirror Nest `@Render()` with
  `#[render(...)]`. (Covered)
- File upload can parse adapter-neutral multipart forms, expose repeated text
  fields and uploaded files, reject non-multipart or malformed requests, and
  enforce body, field, file, and count limits. (Covered)
- Static assets can be served from an imported module with GET and HEAD routes,
  optional SPA fallback, cache-control headers, basic content-type detection,
  hidden dotfile defaults, and root traversal protection. (Covered)
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
- Discovery and reflector utilities can snapshot modules, module graph edges,
  provider tokens, exports, HTTP route metadata, WebSocket gateways, and
  message patterns from a built application. (Covered)

## Immediate Next Task

Continue the Nest framework parity audit and implement the next missing
framework capability. Keep GraphQL out of scope.

Suggested implementation sequence:

1. Continue the Nest framework parity audit with the next non-GraphQL core
   capability.
2. Continue defining integrations through providers, middleware, guards,
   interceptors, or adapters instead of adding one-off framework hooks.
3. Add crate-local tests and README examples for each chosen framework module.
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
