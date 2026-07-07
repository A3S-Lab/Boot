use super::handler::RouteHandler;
use super::path::normalize_prefix;
use super::route::RouteDefinition;
use crate::pipeline::PipelineComponents;
use crate::{
    BootRequest, ExceptionFilter, Guard, Interceptor, Middleware, ModuleRef, Pipe, Result,
    RouteVersioning, SerializationOptions, SseEvent, Validate,
};
use futures_core::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;

/// Group routes under a common HTTP prefix, similar to a Nest controller.
#[derive(Clone)]
pub struct ControllerDefinition {
    prefix: String,
    routes: Vec<RouteDefinition>,
    pipeline: PipelineComponents,
    openapi_tags: Vec<String>,
    versioning: RouteVersioning,
    serialization: Option<SerializationOptions>,
    metadata: BTreeMap<String, Value>,
}

impl ControllerDefinition {
    pub fn new(prefix: impl Into<String>) -> Result<Self> {
        let prefix = normalize_prefix(&prefix.into())?;
        Ok(Self {
            prefix,
            routes: Vec::new(),
            pipeline: PipelineComponents::default(),
            openapi_tags: Vec::new(),
            versioning: RouteVersioning::default(),
            serialization: None,
            metadata: BTreeMap::new(),
        })
    }

    pub fn route(mut self, route: RouteDefinition) -> Result<Self> {
        let mut route = route;
        if route.versioning().is_unspecified() && !self.versioning.is_unspecified() {
            route = match &self.versioning {
                RouteVersioning::Unspecified => route,
                RouteVersioning::Versions(versions) => route.with_versions(versions.clone()),
                RouteVersioning::Neutral => route.version_neutral(),
            };
        }
        if route.serialization().is_empty() {
            if let Some(serialization) = &self.serialization {
                route = route.with_serialization(serialization.clone());
            }
        }
        for tag in &self.openapi_tags {
            route = route.with_tag(tag.clone());
        }
        route = route.with_metadata_defaults(&self.metadata);
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

    pub fn with_middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.pipeline.push_middleware(middleware);
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

    pub fn with_validation(mut self) -> Self {
        self.pipeline.enable_validation();
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        let tag = tag.into();
        if !self.openapi_tags.contains(&tag) {
            self.openapi_tags.push(tag.clone());
        }
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_tag(tag.clone()))
            .collect();
        self
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
        self.serialization = Some(options);
        self
    }

    pub fn with_metadata<V>(self, key: impl Into<String>, value: V) -> Result<Self>
    where
        V: Serialize,
    {
        let key = key.into();
        let value = serde_json::to_value(value).map_err(|error| {
            crate::BootError::Internal(format!(
                "failed to serialize controller metadata `{key}`: {error}"
            ))
        })?;
        Ok(self.with_metadata_value(key, value))
    }

    pub fn with_metadata_value(mut self, key: impl Into<String>, value: Value) -> Self {
        let key = key.into();
        self.metadata.insert(key.clone(), value.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_metadata_default_value(key.clone(), value.clone()))
            .collect();
        self
    }

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn get<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::get(path, handler)?)
    }

    pub fn get_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::get_scoped(path, factory)?)
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

    pub fn post_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::post_scoped(path, factory)?)
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

    pub fn post_validated_json<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.post_validated_json_with_status(path, 200, handler)
    }

    pub fn post_validated_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::post_validated_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn put<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::put(path, handler)?)
    }

    pub fn put_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::put_scoped(path, factory)?)
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

    pub fn put_validated_json<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.put_validated_json_with_status(path, 200, handler)
    }

    pub fn put_validated_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::put_validated_json_with_status(
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

    pub fn patch_validated_json<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.patch_validated_json_with_status(path, 200, handler)
    }

    pub fn patch_validated_json_with_status<T, H, Fut, R>(
        self,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::patch_validated_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn patch<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::patch(path, handler)?)
    }

    pub fn patch_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::patch_scoped(path, factory)?)
    }

    pub fn delete<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::delete(path, handler)?)
    }

    pub fn delete_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::delete_scoped(path, factory)?)
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

    pub fn options_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::options_scoped(path, factory)?)
    }

    pub fn head<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::head(path, handler)?)
    }

    pub fn head_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::head_scoped(path, factory)?)
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
