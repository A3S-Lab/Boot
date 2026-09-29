//! Lightweight observability primitives for Boot microservices (P0).

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

use serde::Serialize;

use crate::{
    BootRequest, BootResponse, Module, ProviderDefinition, ProviderToken, Result, RouteDefinition,
};

/// In-process RED metrics (rate / error / duration) for one process.
#[derive(Debug, Default, Clone)]
pub struct RedMetrics {
    inner: Arc<Mutex<RedState>>,
}

#[derive(Debug, Default)]
struct RedState {
    requests: u64,
    errors: u64,
    /// Sum of request durations in microseconds.
    duration_us: u64,
}

#[derive(Debug, Clone, Serialize)]
pub struct RedSnapshot {
    pub requests: u64,
    pub errors: u64,
    pub duration_us: u64,
    pub error_rate: f64,
    pub avg_duration_us: f64,
}

impl RedMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&self, status: u16, duration: std::time::Duration) {
        let Ok(mut state) = self.inner.lock() else {
            return;
        };
        state.requests = state.requests.saturating_add(1);
        if status >= 500 {
            state.errors = state.errors.saturating_add(1);
        }
        state.duration_us = state
            .duration_us
            .saturating_add(duration.as_micros() as u64);
    }

    pub fn snapshot(&self) -> RedSnapshot {
        let Ok(state) = self.inner.lock() else {
            return RedSnapshot {
                requests: 0,
                errors: 0,
                duration_us: 0,
                error_rate: 0.0,
                avg_duration_us: 0.0,
            };
        };
        let error_rate = if state.requests == 0 {
            0.0
        } else {
            state.errors as f64 / state.requests as f64
        };
        let avg_duration_us = if state.requests == 0 {
            0.0
        } else {
            state.duration_us as f64 / state.requests as f64
        };
        RedSnapshot {
            requests: state.requests,
            errors: state.errors,
            duration_us: state.duration_us,
            error_rate,
            avg_duration_us,
        }
    }

    /// Time a closure and record RED metrics using the HTTP status callback.
    pub async fn time<F, Fut, T>(&self, status_of: impl FnOnce(&T) -> u16, fut: F) -> T
    where
        F: std::future::Future<Output = T>,
    {
        let started = Instant::now();
        let result = fut.await;
        self.record(status_of(&result), started.elapsed());
        result
    }
}

/// Optional OpenTelemetry hook surface. Real exporters plug in later; this keeps
/// the process contract stable (P0.2).
pub trait OtelHooks: Send + Sync + 'static {
    fn on_request_start(&self, method: &str, path: &str);
    fn on_request_end(&self, method: &str, path: &str, status: u16, duration_ms: u64);
}

/// No-op hooks used when OTel is not configured.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopOtelHooks;

impl OtelHooks for NoopOtelHooks {
    fn on_request_start(&self, _method: &str, _path: &str) {}
    fn on_request_end(&self, _method: &str, _path: &str, _status: u16, _duration_ms: u64) {}
}

/// Module exporting [`RedMetrics`] and optional `GET /metrics` JSON snapshot.
pub struct MetricsModule {
    name: &'static str,
    metrics: Arc<RedMetrics>,
    route_path: Option<String>,
    global: bool,
}

impl MetricsModule {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            metrics: Arc::new(RedMetrics::new()),
            route_path: Some("/metrics".into()),
            global: true,
        }
    }

    pub fn from_metrics(name: &'static str, metrics: Arc<RedMetrics>) -> Self {
        Self {
            name,
            metrics,
            route_path: Some("/metrics".into()),
            global: true,
        }
    }

    pub fn without_route(mut self) -> Self {
        self.route_path = None;
        self
    }

    pub fn metrics(&self) -> Arc<RedMetrics> {
        Arc::clone(&self.metrics)
    }
}

impl Module for MetricsModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::from_arc(Arc::clone(
            &self.metrics,
        ))])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<RedMetrics>()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        let Some(path) = &self.route_path else {
            return Ok(Vec::new());
        };
        let metrics = Arc::clone(&self.metrics);
        Ok(vec![RouteDefinition::get(
            path.clone(),
            move |request: BootRequest| {
                let metrics = Arc::clone(&metrics);
                async move {
                    request.require_accepts_json()?;
                    let snap = metrics.snapshot();
                    let mut body = BTreeMap::new();
                    body.insert("red", serde_json::to_value(snap).unwrap_or_default());
                    BootResponse::json(&body)
                }
            },
        )?])
    }
}
