use super::application::BootApplication;
use super::registration::register_module;
use crate::pipeline::PipelineComponents;
use crate::{
    ExceptionFilter, Guard, Interceptor, Module, ModuleRef, Pipe, Result, RouteDefinition,
};
use std::collections::BTreeSet;
use std::sync::Arc;

/// Builder for a [`BootApplication`].
#[derive(Default)]
pub struct BootApplicationBuilder {
    modules: Vec<Arc<dyn Module>>,
    routes: Vec<RouteDefinition>,
    global_pipeline: PipelineComponents,
}

impl BootApplicationBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Import a root module.
    pub fn import<M>(mut self, module: M) -> Self
    where
        M: Module,
    {
        self.modules.push(Arc::new(module));
        self
    }

    /// Import a shared root module.
    pub fn import_arc(mut self, module: Arc<dyn Module>) -> Self {
        self.modules.push(module);
        self
    }

    /// Add a framework-neutral route directly to the application shell.
    pub fn route(mut self, route: RouteDefinition) -> Self {
        self.routes.push(route);
        self
    }

    /// Add an application-wide pipe, similar to Nest's global pipes.
    pub fn use_global_pipe<P>(mut self, pipe: P) -> Self
    where
        P: Pipe,
    {
        self.global_pipeline.push_pipe(pipe);
        self
    }

    /// Add an application-wide guard, similar to Nest's global guards.
    pub fn use_global_guard<G>(mut self, guard: G) -> Self
    where
        G: Guard,
    {
        self.global_pipeline.push_guard(guard);
        self
    }

    /// Add an application-wide interceptor, similar to Nest's global interceptors.
    pub fn use_global_interceptor<I>(mut self, interceptor: I) -> Self
    where
        I: Interceptor,
    {
        self.global_pipeline.push_interceptor(interceptor);
        self
    }

    /// Add an application-wide exception filter, similar to Nest's global filters.
    pub fn use_global_filter<F>(mut self, filter: F) -> Self
    where
        F: ExceptionFilter,
    {
        self.global_pipeline.push_filter(filter);
        self
    }

    /// Resolve module imports, providers, controllers, and routes.
    pub fn build(self) -> Result<BootApplication> {
        let module_ref = ModuleRef::new();
        let mut seen = BTreeSet::new();
        let mut modules = Vec::new();
        let mut module_instances = Vec::new();
        let mut routes = self
            .routes
            .into_iter()
            .map(|route| route.with_pipeline_prefix(&self.global_pipeline))
            .collect::<Vec<_>>();

        for module in &self.modules {
            register_module(
                Arc::clone(module),
                &module_ref,
                &self.global_pipeline,
                &mut seen,
                &mut modules,
                &mut module_instances,
                &mut routes,
            )?;
        }

        Ok(BootApplication {
            routes,
            modules,
            module_ref,
            module_instances,
        })
    }
}
