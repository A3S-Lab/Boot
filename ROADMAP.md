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
- `ProviderDefinition` and `ModuleRef` for typed provider registration and lookup.
- `ControllerDefinition` and `RouteDefinition` for HTTP route groups.
- Nest-style attribute macros: `#[injectable]`, `#[controller]`, `#[get]`,
  `#[post]`, `#[put]`, `#[patch]`, `#[delete]`, `#[sse]`, and raw route mode.
- JSON body and JSON response helpers.
- SSE responses with `SseEvent`, `SseStream`, `BootResponse::sse(...)`,
  `RouteDefinition::sse(...)`, `ControllerDefinition::sse(...)`, and Axum
  streaming support.
- Global and controller-level `Pipe`, `Guard`, `Interceptor`, and
  `ExceptionFilter` support.
- Adapter-neutral request/response types, typed params/query helpers, header
  helpers, route matching, global prefixes, lifecycle hooks, and an Axum adapter.

## Priority Order

1. Parameter extraction macros
2. OpenAPI metadata and generator
3. Validation pipeline
4. Module encapsulation and dynamic modules
5. Middleware
6. WebSocket gateways
7. Microservice transports
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
- `@Query()`
- `@Headers("x-request-id")`
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
    ) -> Result<CatDto> {
        self.cats.find_one(id, query, request_id).await
    }

    #[post("/", status = 201)]
    async fn create(&self, #[body] dto: CreateCatDto) -> Result<CatDto> {
        self.cats.create(dto).await
    }
}
```

Tasks:

- Extend `a3s-boot-macros` to parse attributes on route method arguments.
- Support `#[body]`, `#[request]`, `#[param("name")]`, `#[params]`,
  `#[query]`, `#[query("name")]`, `#[header("name")]`, and `#[headers]`.
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

- Add route metadata storage to `RouteDefinition`.
- Add metadata fields for tags, operation id, summary, description, params,
  query, request body, response bodies, status codes, auth requirements, and
  deprecation.
- Add a schema abstraction that can use a crate such as `schemars` without
  coupling the core to it unless a feature is enabled.
- Add `OpenApiDocument` generation from `BootApplication`.
- Add optional route to serve JSON, for example `/openapi.json`.
- Preserve adapter neutrality.

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

Proposed A3S Boot shape:

```rust
#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
    age: Option<u8>,
}

impl Validate for CreateCatDto {
    fn validate(&self) -> Result<()> {
        ensure!(!self.name.trim().is_empty(), "name is required");
        Ok(())
    }
}
```

Tasks:

- Add a small `Validate` trait in core or a `validation` feature.
- Integrate validation after DTO extraction and before handler invocation.
- Support explicit validation pipe composition for projects that prefer a third
  party crate such as `garde` or `validator`.
- Support request body, params, and query validation.
- Add consistent validation error response mapping.

Acceptance:

- Invalid JSON body DTOs return HTTP 400 with contextual messages.
- Invalid query/param DTOs return HTTP 400.
- Validation can be enabled globally, controller-level, and route-level.
- Validation does not run for raw handlers unless explicitly configured.

## Milestone 4: Module Encapsulation And Dynamic Modules

Nest equivalent:

- module `exports`
- re-exported modules
- global modules
- dynamic modules

Current gap:

`a3s-boot` registers providers into one resolved application container. Nest
modules encapsulate providers unless they are exported, and dynamic modules can
produce providers/imports from runtime configuration.

Tasks:

- Introduce module-scoped provider registries.
- Add explicit provider exports and imported-module visibility.
- Support re-exporting imported modules.
- Add global modules for opt-in application-wide providers.
- Add dynamic module builders for configuration-driven providers.
- Preserve direct host access through `BootApplication::get(...)` where it makes
  sense, but avoid accidentally exposing private feature-module providers.

Acceptance:

- A provider declared but not exported by an imported module is not visible to
  the importing module.
- Exported providers are visible transitively according to explicit imports.
- Duplicate-provider checks respect module scope.
- Existing simple module examples continue to work or have a documented migration.

## Milestone 5: Middleware

Nest equivalent:

- `NestMiddleware`
- `MiddlewareConsumer`
- route-scoped middleware

Tasks:

- Add middleware trait that can inspect/mutate `BootRequest` before pipes and
  guards.
- Allow middleware to short-circuit with `BootResponse`.
- Support global, module/controller, and route-scoped registration.
- Preserve order: middleware, pipes, guards, interceptors, handler, filters.
- Ensure adapter-level request validation remains before middleware.

Acceptance:

- Middleware can add request headers or context values before a handler.
- Middleware can reject a request before guards run.
- Route-scoped middleware only applies to matching route groups.
- Pipeline ordering is covered by tests.

## Milestone 6: WebSocket Gateways

Nest equivalent:

- `@WebSocketGateway()`
- `@SubscribeMessage()`
- gateway lifecycle hooks
- gateway guards/pipes/interceptors

Tasks:

- Define adapter-neutral WebSocket connection and message traits.
- Add gateway registration API.
- Add `#[websocket_gateway]` and `#[subscribe_message]` macros.
- Implement Axum WebSocket adapter support behind the `axum` feature.
- Reuse DI and pipeline concepts where possible.

Acceptance:

- A gateway can accept a WebSocket connection and dispatch messages by event
  name.
- Gateway handlers can use providers.
- Gateway guards/pipes/interceptors run in deterministic order.
- Tests cover in-process adapter behavior and Axum integration.

## Milestone 7: Microservice Transports

Nest equivalent:

- Redis, NATS, MQTT, RabbitMQ, Kafka, gRPC, and custom transports.
- message pattern handlers.

Tasks:

- Define an adapter-neutral message transport trait.
- Add message pattern registration APIs and macros.
- Reuse provider lookup and pipeline primitives.
- Start with an in-process test transport before external brokers.
- Add one production transport only after the core contract is stable.

Acceptance:

- A module can register message handlers independently from HTTP routes.
- Message handlers can use providers and validation.
- Tests cover request-response and event-only patterns.

## Milestone 8: Technique Modules

Nest equivalent areas:

- configuration
- cache
- task scheduling
- queues
- logging
- API versioning
- serialization
- compression
- file upload
- security helpers such as CORS, CSRF, helmet-like headers, and rate limiting

Tasks:

- Prefer companion crates or feature modules over bloating the core.
- Keep configuration HCL-first unless a local convention says otherwise.
- Define integration points through providers, middleware, guards, interceptors,
  and adapters.

Acceptance:

- Each technique module has its own tests and docs.
- Core remains usable without the technique modules.
- Modules compose through the same provider and lifecycle APIs.

## Immediate Next Task

Start with Milestone 1: parameter extraction macros.

Suggested implementation sequence:

1. Add macro parser support for method argument attributes in
   `macros/src/lib.rs`.
2. Generate `BootRequest` extraction wrappers for `#[body]`, `#[param]`,
   `#[query]`, and `#[header]`.
3. Add focused macro tests in `tests/macros.rs`.
4. Update README examples to use extractor macros.
5. Run:

```sh
cargo fmt --all
cargo test --test macros --test controllers
cargo test --all-targets
cargo test --no-default-features
cargo clippy --all-targets -- -D warnings
cargo clippy --no-default-features --all-targets -- -D warnings
git diff --check
```
