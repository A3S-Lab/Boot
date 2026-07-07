use crate::{
    BootError, ExceptionFilter, Guard, HttpMethod, Interceptor, Middleware, OpenApiRouteMetadata,
    Pipe, RequestValidator, Result, RouteVersioning, SerializationOptions,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::routing::handler::{RequestScopedRouteHandler, RouteHandler};
use crate::routing::path::{
    match_path_params, match_path_shape, route_param_names, route_shape_key, validate_route_path,
};
use crate::ModuleRef;

/// A framework-neutral route definition.
#[derive(Clone)]
pub struct RouteDefinition {
    pub(super) method: HttpMethod,
    pub(super) path: String,
    pub(super) handler: Arc<dyn RouteHandler>,
    pub(super) middleware: Vec<Arc<dyn Middleware>>,
    pub(super) pipes: Vec<Arc<dyn Pipe>>,
    pub(super) guards: Vec<Arc<dyn Guard>>,
    pub(super) interceptors: Vec<Arc<dyn Interceptor>>,
    pub(super) filters: Vec<Arc<dyn ExceptionFilter>>,
    pub(super) validators: Vec<RequestValidator>,
    pub(super) validation_enabled: bool,
    pub(super) validation_disabled: bool,
    pub(super) module_name: Option<String>,
    pub(super) controller_prefix: Option<String>,
    pub(super) module_ref: Option<ModuleRef>,
    pub(super) openapi: OpenApiRouteMetadata,
    pub(super) versioning: RouteVersioning,
    pub(super) serialization: SerializationOptions,
    pub(super) metadata: BTreeMap<String, Value>,
}

impl RouteDefinition {
    pub fn new<H>(method: HttpMethod, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        let path = path.into();
        validate_route_path(&path)?;
        Ok(Self {
            method,
            path,
            handler: Arc::new(handler),
            middleware: Vec::new(),
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            validators: Vec::new(),
            validation_enabled: false,
            validation_disabled: false,
            module_name: None,
            controller_prefix: None,
            module_ref: None,
            openapi: OpenApiRouteMetadata::default(),
            versioning: RouteVersioning::default(),
            serialization: SerializationOptions::default(),
            metadata: BTreeMap::new(),
        })
    }

    pub fn new_scoped<F, H>(method: HttpMethod, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        Self::new(method, path, RequestScopedRouteHandler::new(factory))
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn path_shape(&self) -> String {
        route_shape_key(&self.path)
    }

    pub fn path_param_names(&self) -> Vec<&str> {
        route_param_names(&self.path)
    }

    pub fn matches_path(&self, path: &str) -> bool {
        match_path_shape(&self.path, path)
    }

    pub fn path_params(&self, path: &str) -> Result<Option<BTreeMap<String, String>>> {
        match_path_params(&self.path, path)
    }

    pub fn module_name(&self) -> Option<&str> {
        self.module_name.as_deref()
    }

    pub fn controller_prefix(&self) -> Option<&str> {
        self.controller_prefix.as_deref()
    }

    pub fn handler(&self) -> Arc<dyn RouteHandler> {
        Arc::clone(&self.handler)
    }

    pub fn openapi(&self) -> &OpenApiRouteMetadata {
        &self.openapi
    }

    pub fn versioning(&self) -> &RouteVersioning {
        &self.versioning
    }

    pub fn serialization(&self) -> &SerializationOptions {
        &self.serialization
    }

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn metadata_as<T>(&self, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.metadata.get(key) else {
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

    pub fn validation_enabled(&self) -> bool {
        self.validation_enabled
    }

    pub fn with_version(mut self, version: impl Into<String>) -> Self {
        self.versioning = RouteVersioning::version(version);
        self
    }

    pub fn with_versions<I, V>(mut self, versions: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<String>,
    {
        self.versioning = RouteVersioning::versions(versions);
        self
    }

    pub fn version_neutral(mut self) -> Self {
        self.versioning = RouteVersioning::neutral();
        self
    }

    pub fn with_serialization(mut self, options: SerializationOptions) -> Self {
        self.serialization = options;
        self
    }

    pub fn with_metadata<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize route metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        self.metadata.insert(key.into(), value);
        self
    }

    pub(crate) fn with_metadata_defaults(mut self, metadata: &BTreeMap<String, Value>) -> Self {
        for (key, value) in metadata {
            self.metadata
                .entry(key.clone())
                .or_insert_with(|| value.clone());
        }
        self
    }

    pub(crate) fn with_metadata_default_value(
        mut self,
        key: impl Into<String>,
        value: Value,
    ) -> Self {
        self.metadata.entry(key.into()).or_insert(value);
        self
    }
}
