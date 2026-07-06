use super::{ExceptionFilter, Guard, Interceptor, Pipe};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct PipelineComponents {
    pub pipes: Vec<Arc<dyn Pipe>>,
    pub guards: Vec<Arc<dyn Guard>>,
    pub interceptors: Vec<Arc<dyn Interceptor>>,
    pub filters: Vec<Arc<dyn ExceptionFilter>>,
}

impl PipelineComponents {
    pub fn push_pipe<P>(&mut self, pipe: P)
    where
        P: Pipe,
    {
        self.pipes.push(Arc::new(pipe));
    }

    pub fn push_guard<G>(&mut self, guard: G)
    where
        G: Guard,
    {
        self.guards.push(Arc::new(guard));
    }

    pub fn push_interceptor<I>(&mut self, interceptor: I)
    where
        I: Interceptor,
    {
        self.interceptors.push(Arc::new(interceptor));
    }

    pub fn push_filter<F>(&mut self, filter: F)
    where
        F: ExceptionFilter,
    {
        self.filters.push(Arc::new(filter));
    }
}
