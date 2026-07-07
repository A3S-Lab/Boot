use crate::{
    ExceptionFilter, Guard, HttpMethod, Interceptor, Middleware, OpenApiRouteMetadata, Pipe,
    RequestValidator, Result, RouteVersioning, SerializationOptions,
};
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::routing::handler::RouteHandler;
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
        })
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
}
