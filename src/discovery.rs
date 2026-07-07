use crate::{
    BootApplication, BootError, HttpMethod, MessagePatternKind, OpenApiRouteMetadata,
    ProviderToken, Result, RouteVersioning, SerializationOptions,
};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::BTreeMap;

/// Snapshot of a registered module and its local providers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredModule {
    pub name: String,
    pub provider_tokens: Vec<ProviderToken>,
}

/// Snapshot of a resolved HTTP route.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveredRoute {
    pub method: HttpMethod,
    pub path: String,
    pub path_shape: String,
    pub path_params: Vec<String>,
    pub module_name: Option<String>,
    pub controller_prefix: Option<String>,
    pub openapi: OpenApiRouteMetadata,
    pub versioning: RouteVersioning,
    pub serialization: SerializationOptions,
    pub metadata: BTreeMap<String, Value>,
    pub validation_enabled: bool,
}

/// Snapshot of a resolved WebSocket gateway.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredGateway {
    pub path: String,
    pub path_shape: String,
    pub module_name: Option<String>,
    pub events: Vec<String>,
}

/// Snapshot of a resolved microservice message pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiscoveredMessagePattern {
    pub pattern: String,
    pub kind: MessagePatternKind,
    pub module_name: Option<String>,
}

/// Read-only discovery snapshot for a built Boot application.
#[derive(Debug, Clone, PartialEq)]
pub struct DiscoveryService {
    modules: Vec<DiscoveredModule>,
    routes: Vec<DiscoveredRoute>,
    gateways: Vec<DiscoveredGateway>,
    message_patterns: Vec<DiscoveredMessagePattern>,
}

impl DiscoveryService {
    pub fn from_app(app: &BootApplication) -> Result<Self> {
        Ok(Self {
            modules: discover_modules(app)?,
            routes: discover_routes(app),
            gateways: discover_gateways(app),
            message_patterns: discover_message_patterns(app),
        })
    }

    pub fn modules(&self) -> &[DiscoveredModule] {
        &self.modules
    }

    pub fn routes(&self) -> &[DiscoveredRoute] {
        &self.routes
    }

    pub fn gateways(&self) -> &[DiscoveredGateway] {
        &self.gateways
    }

    pub fn message_patterns(&self) -> &[DiscoveredMessagePattern] {
        &self.message_patterns
    }

    pub fn module(&self, name: &str) -> Option<&DiscoveredModule> {
        self.modules.iter().find(|module| module.name == name)
    }

    pub fn modules_with_provider(&self, token: &ProviderToken) -> Vec<&DiscoveredModule> {
        self.modules
            .iter()
            .filter(|module| module.provider_tokens.contains(token))
            .collect()
    }

    pub fn routes_for_module(&self, module_name: &str) -> Vec<&DiscoveredRoute> {
        self.routes
            .iter()
            .filter(|route| route.module_name.as_deref() == Some(module_name))
            .collect()
    }

    pub fn routes_for_controller(&self, controller_prefix: &str) -> Vec<&DiscoveredRoute> {
        self.routes
            .iter()
            .filter(|route| route.controller_prefix.as_deref() == Some(controller_prefix))
            .collect()
    }

    pub fn gateways_for_module(&self, module_name: &str) -> Vec<&DiscoveredGateway> {
        self.gateways
            .iter()
            .filter(|gateway| gateway.module_name.as_deref() == Some(module_name))
            .collect()
    }

    pub fn message_patterns_for_module(&self, module_name: &str) -> Vec<&DiscoveredMessagePattern> {
        self.message_patterns
            .iter()
            .filter(|pattern| pattern.module_name.as_deref() == Some(module_name))
            .collect()
    }

    pub fn reflector(&self) -> Reflector {
        Reflector::new(self.clone())
    }
}

/// Metadata lookup helper over a discovery snapshot.
#[derive(Debug, Clone, PartialEq)]
pub struct Reflector {
    discovery: DiscoveryService,
}

impl Reflector {
    pub fn new(discovery: DiscoveryService) -> Self {
        Self { discovery }
    }

    pub fn from_app(app: &BootApplication) -> Result<Self> {
        Ok(Self::new(DiscoveryService::from_app(app)?))
    }

    pub fn discovery(&self) -> &DiscoveryService {
        &self.discovery
    }

    pub fn route(&self, method: HttpMethod, path: &str) -> Option<&DiscoveredRoute> {
        self.discovery
            .routes
            .iter()
            .find(|route| route.method == method && route.path == path)
    }

    pub fn openapi(&self, method: HttpMethod, path: &str) -> Option<&OpenApiRouteMetadata> {
        self.route(method, path).map(|route| &route.openapi)
    }

    pub fn metadata(&self, method: HttpMethod, path: &str) -> Option<&BTreeMap<String, Value>> {
        self.route(method, path).map(|route| &route.metadata)
    }

    pub fn metadata_value(&self, method: HttpMethod, path: &str, key: &str) -> Option<&Value> {
        self.metadata(method, path)
            .and_then(|metadata| metadata.get(key))
    }

    pub fn metadata_as<T>(&self, method: HttpMethod, path: &str, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.metadata_value(method, path, key) else {
            return Ok(None);
        };

        serde_json::from_value(value.clone())
            .map(Some)
            .map_err(|error| {
                BootError::Internal(format!(
                    "failed to deserialize route metadata `{key}`: {error}"
                ))
            })
    }

    pub fn operation_id(&self, method: HttpMethod, path: &str) -> Option<&str> {
        self.openapi(method, path)
            .and_then(|metadata| metadata.operation_id.as_deref())
    }

    pub fn routes_with_tag(&self, tag: &str) -> Vec<&DiscoveredRoute> {
        self.discovery
            .routes
            .iter()
            .filter(|route| route.openapi.tags.iter().any(|value| value == tag))
            .collect()
    }

    pub fn routes_with_metadata(&self, key: &str) -> Vec<&DiscoveredRoute> {
        self.discovery
            .routes
            .iter()
            .filter(|route| route.metadata.contains_key(key))
            .collect()
    }

    pub fn routes_with_metadata_value(&self, key: &str, value: &Value) -> Vec<&DiscoveredRoute> {
        self.discovery
            .routes
            .iter()
            .filter(|route| route.metadata.get(key) == Some(value))
            .collect()
    }

    pub fn routes_for_module(&self, module_name: &str) -> Vec<&DiscoveredRoute> {
        self.discovery.routes_for_module(module_name)
    }

    pub fn routes_for_controller(&self, controller_prefix: &str) -> Vec<&DiscoveredRoute> {
        self.discovery.routes_for_controller(controller_prefix)
    }
}

fn discover_modules(app: &BootApplication) -> Result<Vec<DiscoveredModule>> {
    app.module_instances
        .iter()
        .map(|instance| {
            Ok(DiscoveredModule {
                name: instance.module.name().to_string(),
                provider_tokens: instance.module_ref.local_tokens()?,
            })
        })
        .collect()
}

fn discover_routes(app: &BootApplication) -> Vec<DiscoveredRoute> {
    app.routes()
        .iter()
        .map(|route| DiscoveredRoute {
            method: route.method(),
            path: route.path().to_string(),
            path_shape: route.path_shape(),
            path_params: route
                .path_param_names()
                .into_iter()
                .map(str::to_string)
                .collect(),
            module_name: route.module_name().map(str::to_string),
            controller_prefix: route.controller_prefix().map(str::to_string),
            openapi: route.openapi().clone(),
            versioning: route.versioning().clone(),
            serialization: route.serialization().clone(),
            metadata: route.metadata().clone(),
            validation_enabled: route.validation_enabled(),
        })
        .collect()
}

fn discover_gateways(app: &BootApplication) -> Vec<DiscoveredGateway> {
    app.gateways()
        .iter()
        .map(|gateway| DiscoveredGateway {
            path: gateway.path().to_string(),
            path_shape: gateway.path_shape(),
            module_name: gateway.module_name().map(str::to_string),
            events: gateway.events().into_iter().map(str::to_string).collect(),
        })
        .collect()
}

fn discover_message_patterns(app: &BootApplication) -> Vec<DiscoveredMessagePattern> {
    app.message_patterns()
        .iter()
        .map(|pattern| DiscoveredMessagePattern {
            pattern: pattern.pattern().to_string(),
            kind: pattern.kind(),
            module_name: pattern.module_name().map(str::to_string),
        })
        .collect()
}
