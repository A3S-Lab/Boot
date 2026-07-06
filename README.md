# A3S Boot

<p align="center">
  <strong>Modular Rust Web Framework for A3S</strong>
</p>

<p align="center">
  <em>A Rust-first framework foundation inspired by Nest.js modules, built around replaceable HTTP adapters.</em>
</p>

---

## Overview

**A3S Boot** is an early Rust web framework crate for building modular A3S
services. Its goal is to bring the parts of Nest.js that work well for service
architecture into idiomatic Rust:

- explicit application modules
- importable feature modules
- framework-neutral route definitions
- replaceable HTTP adapters
- a single application builder
- future support for dependency injection, guards, interceptors, pipes, filters,
  and lifecycle hooks

The crate does not try to copy TypeScript decorators. Rust modules are plain
types implementing the `Module` trait. Route handling is delegated to an
adapter; Axum is the default adapter, not the framework kernel.

## Status

This repository contains the initial foundation:

- `Module` for declaring module names, imports, and routes
- `HttpAdapter` for plugging in different HTTP backends
- `AxumAdapter` behind the default `axum` feature
- `BootApplicationBuilder` for resolving module imports
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
use a3s_boot::{AxumAdapter, BootApplication, BootResponse, Module, RouteDefinition};

#[derive(Debug)]
struct AppModule;

impl Module for AppModule {
    fn name(&self) -> &'static str {
        "app"
    }

    fn routes(&self) -> a3s_boot::Result<Vec<RouteDefinition>> {
        Ok(vec![RouteDefinition::get("/", |_| async {
            Ok(BootResponse::text("Hello from A3S Boot"))
        })?])
    }
}

#[tokio::main]
async fn main() -> a3s_boot::Result<()> {
    let app = BootApplication::builder().import(AppModule).build()?;
    app.serve_with(&AxumAdapter::new(), ([127, 0, 0, 1], 3000).into()).await
}
```

Run the example:

```sh
cargo run --example hello
```

## Design Direction

A3S Boot aims to provide a structured service framework for A3S components:

| Concept | Direction |
| --- | --- |
| Module | A named feature boundary with imports, providers, and routes |
| HTTP adapter | Replaceable backend adapter; Axum is the first implementation |
| Controller | Typed request handlers grouped by route prefix |
| Provider | Injectable service or repository dependency |
| Guard | Request authorization and policy gate |
| Interceptor | Cross-cutting request/response behavior |
| Pipe | Request validation and transformation |
| Filter | Error mapping into HTTP responses |
| Lifecycle hook | Startup and shutdown behavior for modules and providers |

The first milestone is a small, explicit core that remains easy to embed in
A3S Gateway, A3S Code services, and standalone control-plane APIs.

## Development

```sh
cargo fmt --all
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
```

## License

MIT
