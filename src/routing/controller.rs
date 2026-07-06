use super::handler::RouteHandler;
use super::path::normalize_prefix;
use super::route::RouteDefinition;
use crate::pipeline::PipelineComponents;
use crate::{ExceptionFilter, Guard, Interceptor, Pipe, Result};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::Future;

/// Group routes under a common HTTP prefix, similar to a Nest controller.
#[derive(Clone)]
pub struct ControllerDefinition {
    prefix: String,
    routes: Vec<RouteDefinition>,
    pipeline: PipelineComponents,
}

impl ControllerDefinition {
    pub fn new(prefix: impl Into<String>) -> Result<Self> {
        let prefix = normalize_prefix(&prefix.into())?;
        Ok(Self {
            prefix,
            routes: Vec::new(),
            pipeline: PipelineComponents::default(),
        })
    }

    pub fn route(mut self, route: RouteDefinition) -> Result<Self> {
        self.routes.push(
            route
                .with_prefix(&self.prefix)?
                .with_pipeline_prefix(&self.pipeline),
        );
        Ok(self)
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: Pipe,
    {
        self.pipeline.push_pipe(pipe);
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.pipeline.push_guard(guard);
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        self.pipeline.push_interceptor(interceptor);
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        self.pipeline.push_filter(filter);
        self
    }

    pub fn get<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::get(path, handler)?)
    }

    pub fn post<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::post(path, handler)?)
    }

    pub fn post_json<T, H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::post_json(path, handler)?)
    }

    pub fn put<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::put(path, handler)?)
    }

    pub fn put_json<T, H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::put_json(path, handler)?)
    }

    pub fn patch_json<T, H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::patch_json(path, handler)?)
    }

    pub fn delete<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::delete(path, handler)?)
    }

    pub fn prefix(&self) -> &str {
        &self.prefix
    }

    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }

    pub(crate) fn into_routes(self) -> Vec<RouteDefinition> {
        self.routes
    }
}
