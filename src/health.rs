use crate::{
    BootError, BootRequest, BootResponse, BoxFuture, Module, ProviderDefinition, ProviderToken,
    Result, RouteDefinition,
};
use serde::Serialize;
use serde_json::{Map, Value};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Health status used by application health reports.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum HealthStatus {
    Up,
    Down,
}

impl HealthStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::Down => "down",
        }
    }
}

/// Result returned by one health indicator.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HealthIndicatorResult {
    pub status: HealthStatus,
    pub details: Value,
}

impl HealthIndicatorResult {
    pub fn up() -> Self {
        Self::new(HealthStatus::Up)
    }

    pub fn down() -> Self {
        Self::new(HealthStatus::Down)
    }

    pub fn new(status: HealthStatus) -> Self {
        Self {
            status,
            details: Value::Object(Map::new()),
        }
    }

    pub fn with_details(mut self, details: Value) -> Self {
        self.details = details;
        self
    }

    pub fn with_detail_value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        match &mut self.details {
            Value::Object(details) => {
                details.insert(key.into(), value.into());
            }
            _ => {
                let mut details = Map::new();
                details.insert(key.into(), value.into());
                self.details = Value::Object(details);
            }
        }
        self
    }

    pub fn is_up(&self) -> bool {
        self.status == HealthStatus::Up
    }
}

/// Async health indicator used by [`HealthCheckService`].
pub trait HealthIndicator: Send + Sync + 'static {
    fn check(&self) -> BoxFuture<'static, Result<HealthIndicatorResult>>;
}

impl<F, Fut> HealthIndicator for F
where
    F: Fn() -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<HealthIndicatorResult>> + Send + 'static,
{
    fn check(&self) -> BoxFuture<'static, Result<HealthIndicatorResult>> {
        Box::pin(self())
    }
}

/// Aggregate health report returned by [`HealthCheckService`].
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct HealthReport {
    pub status: HealthStatus,
    pub checks: BTreeMap<String, HealthIndicatorResult>,
}

impl HealthReport {
    pub fn is_healthy(&self) -> bool {
        self.status == HealthStatus::Up
    }
}

/// Provider-backed health check service.
#[derive(Clone, Default)]
pub struct HealthCheckService {
    indicators: Arc<RwLock<Vec<HealthIndicatorRegistration>>>,
}

#[derive(Clone)]
struct HealthIndicatorRegistration {
    name: String,
    indicator: Arc<dyn HealthIndicator>,
}

impl fmt::Debug for HealthCheckService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let indicator_count = self.indicators.read().map(|items| items.len()).unwrap_or(0);
        f.debug_struct("HealthCheckService")
            .field("indicators", &indicator_count)
            .finish_non_exhaustive()
    }
}

impl HealthCheckService {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn indicator<I>(&self, name: impl Into<String>, indicator: I) -> Result<()>
    where
        I: HealthIndicator,
    {
        self.indicator_arc(name, Arc::new(indicator))
    }

    pub fn indicator_arc(
        &self,
        name: impl Into<String>,
        indicator: Arc<dyn HealthIndicator>,
    ) -> Result<()> {
        let name = validate_indicator_name(name.into())?;
        self.write_indicators()?
            .push(HealthIndicatorRegistration { name, indicator });
        Ok(())
    }

    pub async fn check(&self) -> Result<HealthReport> {
        let indicators = self.read_indicators()?.clone();
        let mut checks = BTreeMap::new();
        let mut status = HealthStatus::Up;

        for registration in indicators {
            let result = match registration.indicator.check().await {
                Ok(result) => result,
                Err(error) => HealthIndicatorResult::down()
                    .with_detail_value("error", Value::from(error.to_string())),
            };

            if !result.is_up() {
                status = HealthStatus::Down;
            }
            checks.insert(registration.name, result);
        }

        Ok(HealthReport { status, checks })
    }

    pub fn indicator_count(&self) -> Result<usize> {
        Ok(self.read_indicators()?.len())
    }

    pub fn clear_indicators(&self) -> Result<()> {
        self.write_indicators()?.clear();
        Ok(())
    }

    fn read_indicators(
        &self,
    ) -> Result<std::sync::RwLockReadGuard<'_, Vec<HealthIndicatorRegistration>>> {
        self.indicators
            .read()
            .map_err(|_| BootError::Internal("health indicator lock is poisoned".to_string()))
    }

    fn write_indicators(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, Vec<HealthIndicatorRegistration>>> {
        self.indicators
            .write()
            .map_err(|_| BootError::Internal("health indicator lock is poisoned".to_string()))
    }
}

/// Module that registers a [`HealthCheckService`] provider and optional route.
#[derive(Clone)]
pub struct HealthModule {
    name: &'static str,
    token: ProviderToken,
    service: Arc<HealthCheckService>,
    indicators: Vec<(String, Arc<dyn HealthIndicator>)>,
    route_path: Option<String>,
    global: bool,
}

impl fmt::Debug for HealthModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HealthModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("indicators", &self.indicators.len())
            .field("route_path", &self.route_path)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl HealthModule {
    pub fn new(name: &'static str) -> Self {
        Self::from_service(name, HealthCheckService::new())
    }

    pub fn from_service(name: &'static str, service: HealthCheckService) -> Self {
        Self {
            name,
            token: ProviderToken::of::<HealthCheckService>(),
            service: Arc::new(service),
            indicators: Vec::new(),
            route_path: Some("/health".to_string()),
            global: false,
        }
    }

    pub fn indicator<I>(mut self, name: impl Into<String>, indicator: I) -> Self
    where
        I: HealthIndicator,
    {
        self.indicators.push((name.into(), Arc::new(indicator)));
        self
    }

    pub fn indicator_arc(
        mut self,
        name: impl Into<String>,
        indicator: Arc<dyn HealthIndicator>,
    ) -> Self {
        self.indicators.push((name.into(), indicator));
        self
    }

    pub fn with_route(mut self, path: impl Into<String>) -> Self {
        self.route_path = Some(path.into());
        self
    }

    pub fn without_route(mut self) -> Self {
        self.route_path = None;
        self
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for HealthModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.service),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        let Some(path) = &self.route_path else {
            return Ok(Vec::new());
        };

        let service = Arc::clone(&self.service);
        Ok(vec![RouteDefinition::get(
            path.clone(),
            move |request: BootRequest| {
                let service = Arc::clone(&service);
                async move {
                    request.require_accepts_json()?;
                    let report = service.check().await?;
                    let status = if report.is_healthy() { 200 } else { 503 };
                    BootResponse::json_with_status(status, &report)
                }
            },
        )?])
    }

    fn on_module_init(&self, _module_ref: &crate::ModuleRef) -> Result<()> {
        for (name, indicator) in &self.indicators {
            self.service
                .indicator_arc(name.clone(), Arc::clone(indicator))?;
        }
        Ok(())
    }
}

fn validate_indicator_name(name: String) -> Result<String> {
    let name = name.trim().to_string();
    if name.is_empty() || name.contains(char::is_whitespace) {
        return Err(BootError::Internal(format!(
            "health indicator name must be non-empty and contain no whitespace: {name:?}"
        )));
    }
    Ok(name)
}
