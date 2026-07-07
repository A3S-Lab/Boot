use super::handler::RouteHandler;
use super::path::normalize_prefix;
use super::route::RouteDefinition;
use crate::pipeline::PipelineComponents;
use crate::{BootRequest, ExceptionFilter, Guard, Interceptor, Pipe, Result, SseEvent};
use futures_core::Stream;
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

    pub fn get_json<H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.get_json_with_status(path, 200, handler)
    }

    pub fn get_json_with_status<H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::get_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn sse<H, Fut, S>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S>> + Send + 'static,
        S: Stream<Item = Result<SseEvent>> + Send + 'static,
    {
        self.route(RouteDefinition::sse(path, handler)?)
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
        self.post_json_with_status(path, 200, handler)
    }

    pub fn post_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::post_json_with_status(
            path, status, handler,
        )?)
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
        self.put_json_with_status(path, 200, handler)
    }

    pub fn put_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::put_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn patch_json<T, H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.patch_json_with_status(path, 200, handler)
    }

    pub fn patch_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::patch_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn patch<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::patch(path, handler)?)
    }

    pub fn delete<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::delete(path, handler)?)
    }

    pub fn delete_json<H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.delete_json_with_status(path, 200, handler)
    }

    pub fn delete_json_with_status<H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::delete_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn options<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::options(path, handler)?)
    }

    pub fn head<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::head(path, handler)?)
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
