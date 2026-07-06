use crate::{ExceptionFilter, Guard, HttpMethod, Interceptor, Pipe, Result};
use std::sync::Arc;

use crate::routing::handler::RouteHandler;
use crate::routing::path::validate_route_path;

/// A framework-neutral route definition.
#[derive(Clone)]
pub struct RouteDefinition {
    pub(super) method: HttpMethod,
    pub(super) path: String,
    pub(super) handler: Arc<dyn RouteHandler>,
    pub(super) pipes: Vec<Arc<dyn Pipe>>,
    pub(super) guards: Vec<Arc<dyn Guard>>,
    pub(super) interceptors: Vec<Arc<dyn Interceptor>>,
    pub(super) filters: Vec<Arc<dyn ExceptionFilter>>,
    pub(super) module_name: Option<String>,
    pub(super) controller_prefix: Option<String>,
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
            pipes: Vec::new(),
            guards: Vec::new(),
            interceptors: Vec::new(),
            filters: Vec::new(),
            module_name: None,
            controller_prefix: None,
        })
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn handler(&self) -> Arc<dyn RouteHandler> {
        Arc::clone(&self.handler)
    }
}
