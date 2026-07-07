use super::{ExceptionFilter, Guard, Interceptor, Middleware, Pipe};
use std::sync::Arc;

#[derive(Clone, Default)]
pub(crate) struct PipelineComponents {
    pub middleware: Vec<Arc<dyn Middleware>>,
    pub pipes: Vec<Arc<dyn Pipe>>,
    pub guards: Vec<Arc<dyn Guard>>,
    pub interceptors: Vec<Arc<dyn Interceptor>>,
    pub filters: Vec<Arc<dyn ExceptionFilter>>,
    pub validation_enabled: bool,
}

impl PipelineComponents {
    pub fn push_middleware<M>(&mut self, middleware: M)
    where
        M: Middleware,
    {
        self.middleware.push(Arc::new(middleware));
    }

    pub fn push_middleware_arc(&mut self, middleware: Arc<dyn Middleware>) {
        self.middleware.push(middleware);
    }

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

    pub fn enable_validation(&mut self) {
        self.validation_enabled = true;
    }
}
