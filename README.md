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
- module lifecycle hooks

The crate does not try to copy TypeScript decorators. Rust modules are plain
types implementing the `Module` trait. Controllers are values that group
routes. Providers are normal Rust services stored in a typed container. Route
handling is delegated to an adapter; Axum is the default adapter, not the
framework kernel.

## Status

This repository contains the first framework slice:

- `Module` for declaring imports, providers, controllers, direct routes, and lifecycle hooks
- `ModuleRef` for typed provider lookup
- `ProviderDefinition` for singleton and factory providers
- `ControllerDefinition` for prefix-based route groups
- `Pipe`, `Guard`, `Interceptor`, and `ExceptionFilter` pipeline traits
- application-level `use_global_pipe`, `use_global_guard`,
  `use_global_interceptor`, and `use_global_filter`
- `BootRequest::json`, `BootResponse::json`, and controller `*_json` route helpers
- path params through `{id}` route segments and typed query DTOs
- `HttpAdapter` for plugging in different HTTP backends without coupling core to Axum
- `AxumAdapter` behind the default `axum` feature
- `BootApplicationBuilder` for resolving module imports, providers, controllers, and routes
- duplicate module deduplication by module name
- framework-neutral route registration
- `BootApplication::serve_with(...)` for running through a selected adapter

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

## JSON DTOs

Controllers can accept typed request DTOs and return serializable response DTOs
without manually parsing request bytes:

```rust
use a3s_boot::{BootResponse, ControllerDefinition, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Deserialize)]
struct CreateCatDto {
    name: String,
}

#[derive(Debug, Serialize)]
struct CatDto {
    name: String,
    adopted: bool,
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.post_json("/", |dto: CreateCatDto| async move {
        Ok(CatDto {
            name: dto.name,
            adopted: false,
        })
    })
}

fn manual_response() -> Result<BootResponse> {
    BootResponse::json(&CatDto {
        name: "Milo".to_string(),
        adopted: false,
    })
}
```

Invalid JSON maps to `BootError::BadRequest`; adapters can turn that into HTTP
400 while exception filters can override the response shape.

## Params And Query

Boot keeps route params adapter-neutral. Use `{name}` segments in routes and
read them from `BootRequest`; query strings can be read as raw key/value pairs
or decoded into a typed DTO:

```rust
use a3s_boot::{BootRequest, BootResponse, ControllerDefinition, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct FindCatQuery {
    verbose: bool,
}

fn cats_controller() -> Result<ControllerDefinition> {
    ControllerDefinition::new("/cats")?.get("/{id}", |request: BootRequest| async move {
        let query: FindCatQuery = request.query()?;
        let id = request.param("id").unwrap_or("unknown");

        Ok(BootResponse::text(format!(
            "cat={id}, verbose={}",
            query.verbose
        )))
    })
}
```

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
or custom runtime:

```rust
use std::net::SocketAddr;

use a3s_boot::{BootApplication, BoxFuture, HttpAdapter, Result};

#[derive(Debug)]
struct RouteSnapshot {
    routes: Vec<(String, &'static str)>,
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
                .map(|route| (route.path().to_string(), route.method().as_str()))
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
