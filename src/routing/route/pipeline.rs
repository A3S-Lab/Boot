use super::definition::RouteDefinition;
use crate::pipeline::PipelineComponents;
use crate::{ExceptionFilter, Guard, Interceptor, Pipe};
use std::sync::Arc;

impl RouteDefinition {
    pub(crate) fn with_pipeline_prefix(mut self, pipeline: &PipelineComponents) -> Self {
        self.pipes = prepend(&pipeline.pipes, self.pipes);
        self.guards = prepend(&pipeline.guards, self.guards);
        self.interceptors = prepend(&pipeline.interceptors, self.interceptors);
        self.filters = prepend(&pipeline.filters, self.filters);
        self
    }

    pub fn with_pipe<P>(mut self, pipe: P) -> Self
    where
        P: Pipe,
    {
        self.pipes.push(Arc::new(pipe));
        self
    }

    pub fn with_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.guards.push(Arc::new(guard));
        self
    }

    pub fn with_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        self.interceptors.push(Arc::new(interceptor));
        self
    }

    pub fn with_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        self.filters.push(Arc::new(filter));
        self
    }
}

fn prepend<T>(prefix: &[Arc<T>], values: Vec<Arc<T>>) -> Vec<Arc<T>>
where
    T: ?Sized,
{
    let mut merged = prefix.to_vec();
    merged.extend(values);
    merged
}
