use super::handler::RouteHandler;
use super::host::validate_host_pattern;
use super::path::normalize_prefix;
use super::route::RouteDefinition;
use crate::pipeline::{PipelineComponent, PipelineComponents};
use crate::{
    BootErrorKind, BootRequest, ExceptionFilter, ExecutionInterceptor, Guard, Interceptor,
    Middleware, ModuleRef, OpenApiExample, OpenApiHeader, OpenApiParameter, OpenApiRequestBody,
    OpenApiResponse, OpenApiSchema, Pipe, Result, RouteVersioning, SerializationOptions, SseEvent,
    Validate, ValidationOptions,
};
use futures_core::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::future::Future;
use std::sync::Arc;
#[cfg(feature = "cache")]
use std::time::Duration;

/// Group routes under a common HTTP prefix, similar to a Nest controller.
#[derive(Clone)]
pub struct ControllerDefinition {
    prefix: String,
    host: Option<String>,
    routes: Vec<RouteDefinition>,
    pipeline: PipelineComponents,
    openapi_tags: Vec<String>,
    openapi_schema_components: BTreeMap<String, OpenApiSchema>,
    openapi_response_components: BTreeMap<String, OpenApiResponse>,
    openapi_parameter_components: BTreeMap<String, OpenApiParameter>,
    openapi_example_components: BTreeMap<String, OpenApiExample>,
    openapi_request_body_components: BTreeMap<String, OpenApiRequestBody>,
    openapi_header_components: BTreeMap<String, OpenApiHeader>,
    openapi_extensions: BTreeMap<String, Value>,
    openapi_hidden: bool,
    versioning: RouteVersioning,
    serialization: Option<SerializationOptions>,
    metadata: BTreeMap<String, Value>,
}

impl ControllerDefinition {
    pub fn new(prefix: impl Into<String>) -> Result<Self> {
        let prefix = normalize_prefix(&prefix.into())?;
        Ok(Self {
            prefix,
            host: None,
            routes: Vec::new(),
            pipeline: PipelineComponents::default(),
            openapi_tags: Vec::new(),
            openapi_schema_components: BTreeMap::new(),
            openapi_response_components: BTreeMap::new(),
            openapi_parameter_components: BTreeMap::new(),
            openapi_example_components: BTreeMap::new(),
            openapi_request_body_components: BTreeMap::new(),
            openapi_header_components: BTreeMap::new(),
            openapi_extensions: BTreeMap::new(),
            openapi_hidden: false,
            versioning: RouteVersioning::default(),
            serialization: None,
            metadata: BTreeMap::new(),
        })
    }

    pub fn route(mut self, route: RouteDefinition) -> Result<Self> {
        let mut route = route;
        route = route.with_host_default(self.host.as_deref())?;
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
        for (name, schema) in &self.openapi_schema_components {
            route = route.with_schema_component(name.clone(), schema.clone());
        }
        for (name, response) in &self.openapi_response_components {
            route = route.with_response_component(name.clone(), response.clone());
        }
        for (name, parameter) in &self.openapi_parameter_components {
            route = route.with_parameter_component(name.clone(), parameter.clone());
        }
        for (name, example) in &self.openapi_example_components {
            route = route.with_example_component(name.clone(), example.clone());
        }
        for (name, request_body) in &self.openapi_request_body_components {
            route = route.with_request_body_component(name.clone(), request_body.clone());
        }
        for (name, header) in &self.openapi_header_components {
            route = route.with_header_component(name.clone(), header.clone());
        }
        for (name, value) in &self.openapi_extensions {
            route = route.with_openapi_extension_default_value(name.clone(), value.clone());
        }
        if self.openapi_hidden {
            route = route.hide_from_openapi();
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
        let index = self.pipeline.pipes.len();
        let component = PipelineComponent::<dyn Pipe>::new(pipe);
        self.pipeline.pipes.push(component.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.insert_pipe_prefix_at(index, component.clone()))
            .collect();
        self
    }

    pub fn with_middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        let index = self.pipeline.middleware.len();
        let middleware = Arc::new(middleware);
        self.pipeline.middleware.push(middleware.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.insert_middleware_prefix_at(index, middleware.clone()))
            .collect();
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        let index = self.pipeline.guards.len();
        let component = PipelineComponent::<dyn Guard>::new(guard);
        self.pipeline.guards.push(component.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.insert_guard_prefix_at(index, component.clone()))
            .collect();
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        let index = self.pipeline.interceptors.len();
        let component = PipelineComponent::<dyn Interceptor>::new(interceptor);
        self.pipeline.interceptors.push(component.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.insert_interceptor_prefix_at(index, component.clone()))
            .collect();
        self
    }

    pub fn with_execution_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self = self.with_interceptor(crate::pipeline::ExecutionInterceptorAdapter::new(
            interceptor,
        ));
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        let index = self.pipeline.filters.len();
        let component = PipelineComponent::<dyn ExceptionFilter>::new(filter);
        self.pipeline.filters.push(component.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.insert_filter_prefix_at(index, component.clone()))
            .collect();
        self
    }

    pub fn with_catch_filter<I, F>(mut self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: ExceptionFilter,
    {
        self = self.with_filter(crate::catch_errors(kinds, filter));
        self
    }

    pub fn with_validation(mut self) -> Self {
        self.pipeline.enable_validation();
        let prefix = PipelineComponents {
            validation_enabled: true,
            ..PipelineComponents::default()
        };
        self.apply_pipeline_prefix(prefix);
        self
    }

    pub fn with_validation_options(mut self, options: ValidationOptions) -> Self {
        self.pipeline.enable_validation_with_options(options);
        let prefix = PipelineComponents {
            validation_enabled: true,
            validation_options: options,
            ..PipelineComponents::default()
        };
        self.apply_pipeline_prefix(prefix);
        self
    }

    fn apply_pipeline_prefix(&mut self, prefix: PipelineComponents) {
        self.routes = self
            .routes
            .drain(..)
            .map(|route| route.with_pipeline_prefix(&prefix))
            .collect();
    }

    pub fn with_host(mut self, pattern: impl Into<String>) -> Result<Self> {
        let pattern = pattern.into();
        validate_host_pattern(&pattern)?;
        self.host = Some(pattern.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_host_default(Some(&pattern)))
            .collect::<Result<Vec<_>>>()?;
        Ok(self)
    }

    pub fn without_host(mut self) -> Self {
        self.host = None;
        self.routes = self
            .routes
            .into_iter()
            .map(RouteDefinition::without_host)
            .collect();
        self
    }

    pub fn host(&self) -> Option<&str> {
        self.host.as_deref()
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

    pub fn with_schema_component(mut self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        let name = name.into();
        self.openapi_schema_components
            .insert(name.clone(), schema.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_schema_component(name.clone(), schema.clone()))
            .collect();
        self
    }

    pub fn with_response_component(
        mut self,
        name: impl Into<String>,
        response: OpenApiResponse,
    ) -> Self {
        let name = name.into();
        self.openapi_response_components
            .insert(name.clone(), response.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_response_component(name.clone(), response.clone()))
            .collect();
        self
    }

    pub fn with_parameter_component(
        mut self,
        name: impl Into<String>,
        parameter: OpenApiParameter,
    ) -> Self {
        let name = name.into();
        self.openapi_parameter_components
            .insert(name.clone(), parameter.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_parameter_component(name.clone(), parameter.clone()))
            .collect();
        self
    }

    pub fn with_example_component(
        mut self,
        name: impl Into<String>,
        example: OpenApiExample,
    ) -> Self {
        let name = name.into();
        self.openapi_example_components
            .insert(name.clone(), example.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_example_component(name.clone(), example.clone()))
            .collect();
        self
    }

    pub fn try_with_example_component<T>(self, name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_example_component(name, OpenApiExample::try_value(value)?))
    }

    pub fn with_request_body_component(
        mut self,
        name: impl Into<String>,
        request_body: OpenApiRequestBody,
    ) -> Self {
        let name = name.into();
        self.openapi_request_body_components
            .insert(name.clone(), request_body.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_request_body_component(name.clone(), request_body.clone()))
            .collect();
        self
    }

    pub fn with_header_component(mut self, name: impl Into<String>, header: OpenApiHeader) -> Self {
        let name = name.into();
        self.openapi_header_components
            .insert(name.clone(), header.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_header_component(name.clone(), header.clone()))
            .collect();
        self
    }

    pub fn with_openapi_extension_value(mut self, name: impl Into<String>, value: Value) -> Self {
        let name = name.into();
        self.openapi_extensions.insert(name.clone(), value.clone());
        self.routes = self
            .routes
            .into_iter()
            .map(|route| route.with_openapi_extension_default_value(name.clone(), value.clone()))
            .collect();
        self
    }

    pub fn try_with_openapi_extension<T>(self, name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        let name = name.into();
        let value = serde_json::to_value(value).map_err(|error| {
            crate::BootError::Internal(format!(
                "OpenAPI extension `{name}` could not be serialized: {error}"
            ))
        })?;
        Ok(self.with_openapi_extension_value(name, value))
    }

    pub fn hide_from_openapi(mut self) -> Self {
        self.openapi_hidden = true;
        self.routes = self
            .routes
            .into_iter()
            .map(RouteDefinition::hide_from_openapi)
            .collect();
        self
    }

    #[cfg(feature = "openapi-schemas")]
    pub fn try_with_json_schema_component<T>(self) -> Result<Self>
    where
        T: schemars::JsonSchema,
    {
        let schema = OpenApiSchema::json_schema::<T>()
            .map_err(|error| crate::BootError::Internal(error.to_string()))?;
        Ok(self.with_schema_component(crate::openapi_schema_name::<T>(), schema))
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

    #[cfg(feature = "cache")]
    pub fn with_cache_key(self, key: impl Into<String>) -> Self {
        self.with_metadata_value(crate::CACHE_KEY_METADATA, Value::String(key.into()))
    }

    #[cfg(feature = "cache")]
    pub fn with_cache_ttl(self, ttl: Duration) -> Self {
        self.with_metadata_value(
            crate::CACHE_TTL_METADATA,
            Value::Number(serde_json::Number::from(ttl.as_millis() as u64)),
        )
    }

    #[cfg(feature = "cache")]
    pub fn without_cache(self) -> Self {
        self.with_metadata_value(crate::CACHE_DISABLED_METADATA, Value::Bool(true))
    }

    pub fn metadata(&self) -> &BTreeMap<String, Value> {
        &self.metadata
    }

    pub fn metadata_value(&self, key: &str) -> Option<&Value> {
        self.metadata.get(key)
    }

    pub fn all<H>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        self.route(RouteDefinition::all(path, handler)?)
    }

    pub fn all_scoped<F, H>(self, path: impl Into<String>, factory: F) -> Result<Self>
    where
        F: Fn(&ModuleRef) -> Result<H> + Send + Sync + 'static,
        H: RouteHandler,
    {
        self.route(RouteDefinition::all_scoped(path, factory)?)
    }

    pub fn all_json<H, Fut, R>(self, path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.all_json_with_status(path, 200, handler)
    }

    pub fn all_json_with_status<H, Fut, R>(
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
        self.route(RouteDefinition::all_json_with_status(
            path, status, handler,
        )?)
    }

    pub fn view<H, Fut, R>(
        self,
        method: crate::HttpMethod,
        path: impl Into<String>,
        view: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.view_with_status(method, path, view, 200, handler)
    }

    pub fn view_with_status<H, Fut, R>(
        self,
        method: crate::HttpMethod,
        path: impl Into<String>,
        view: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::view_with_status(
            method, path, view, status, handler,
        )?)
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

    pub fn get_view<H, Fut, R>(
        self,
        path: impl Into<String>,
        view: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.get_view_with_status(path, view, 200, handler)
    }

    pub fn get_view_with_status<H, Fut, R>(
        self,
        path: impl Into<String>,
        view: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::get_view_with_status(
            path, view, status, handler,
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

    pub fn post_view<H, Fut, R>(
        self,
        path: impl Into<String>,
        view: impl Into<String>,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.post_view_with_status(path, view, 200, handler)
    }

    pub fn post_view_with_status<H, Fut, R>(
        self,
        path: impl Into<String>,
        view: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        self.route(RouteDefinition::post_view_with_status(
            path, view, status, handler,
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
