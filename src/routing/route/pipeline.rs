use super::definition::RouteDefinition;
use crate::pipeline::{
    ExecutionInterceptorAdapter, PipelineComponent, PipelineComponents, PipelineOverrides,
};
use crate::{
    body_validator, params_validator, query_validator, BootErrorKind, ExceptionFilter,
    ExecutionInterceptor, Guard, Interceptor, Middleware, Pipe, Validate,
};
use serde::de::DeserializeOwned;
use std::sync::Arc;

impl RouteDefinition {
    pub(crate) fn with_pipeline_prefix(mut self, pipeline: &PipelineComponents) -> Self {
        self.middleware = prepend_arc(&pipeline.middleware, self.middleware);
        self.pipes = prepend_component(&pipeline.pipes, self.pipes);
        self.guards = prepend_component(&pipeline.guards, self.guards);
        self.interceptors = prepend_component(&pipeline.interceptors, self.interceptors);
        self.filters = prepend_component(&pipeline.filters, self.filters);
        if !self.validation_disabled {
            self.validation_enabled = pipeline.validation_enabled || self.validation_enabled;
        }
        self
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: Pipe,
    {
        self.pipes.push(PipelineComponent::<dyn Pipe>::new(pipe));
        self
    }

    pub fn with_middleware<M>(mut self, middleware: M) -> Self
    where
        M: Middleware,
    {
        self.middleware.push(Arc::new(middleware));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards.push(PipelineComponent::<dyn Guard>::new(guard));
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        self.interceptors
            .push(PipelineComponent::<dyn Interceptor>::new(interceptor));
        self
    }

    pub fn with_execution_interceptor<I>(self, interceptor: I) -> Self
    where
        I: ExecutionInterceptor,
    {
        self.with_interceptor(ExecutionInterceptorAdapter::new(interceptor))
    }

    pub(crate) fn with_pipeline_overrides(mut self, overrides: &PipelineOverrides) -> Self {
        overrides.apply_to_pipes(&mut self.pipes);
        overrides.apply_to_guards(&mut self.guards);
        overrides.apply_to_interceptors(&mut self.interceptors);
        overrides.apply_to_filters(&mut self.filters);
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        self.filters
            .push(PipelineComponent::<dyn ExceptionFilter>::new(filter));
        self
    }

    pub fn with_catch_filter<I, F>(self, kinds: I, filter: F) -> Self
    where
        I: IntoIterator<Item = BootErrorKind>,
        F: ExceptionFilter,
    {
        self.with_filter(crate::catch_errors(kinds, filter))
    }

    pub fn with_validation(mut self) -> Self {
        self.validation_enabled = true;
        self.validation_disabled = false;
        self
    }

    pub fn without_validation(mut self) -> Self {
        self.validation_enabled = false;
        self.validation_disabled = true;
        self
    }

    pub fn with_body_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(body_validator::<T>());
        self
    }

    pub fn with_params_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(params_validator::<T>());
        self
    }

    pub fn with_query_validation<T>(mut self) -> Self
    where
        T: DeserializeOwned + Validate + 'static,
    {
        self.validators.push(query_validator::<T>());
        self
    }
}

fn prepend_arc<T>(prefix: &[Arc<T>], values: Vec<Arc<T>>) -> Vec<Arc<T>>
where
    T: ?Sized,
{
    let mut merged = prefix.to_vec();
    merged.extend(values);
    merged
}

fn prepend_component<T>(
    prefix: &[PipelineComponent<T>],
    values: Vec<PipelineComponent<T>>,
) -> Vec<PipelineComponent<T>>
where
    T: ?Sized,
{
    let mut merged = prefix.to_vec();
    merged.extend(values);
    merged
}
