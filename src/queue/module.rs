use std::fmt;
use std::sync::Arc;

use a3s_lane::JobQueueBackend as LaneJobQueueBackend;

use crate::{BoxFuture, Module, ModuleRef, ProviderDefinition, ProviderToken, Result};

use super::{Queue, QueueOptions, QueueProcessor};

/// Module that registers and exports a [`Queue`] provider.
#[derive(Clone)]
pub struct QueueModule {
    name: &'static str,
    token: ProviderToken,
    queue: Arc<Queue>,
    processors: Vec<(String, Arc<dyn QueueProcessor>)>,
    global: bool,
}

impl fmt::Debug for QueueModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("QueueModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("queue", &self.queue)
            .field("processors", &self.processors.len())
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl QueueModule {
    pub fn in_process(name: &'static str) -> Self {
        Self::from_queue(name, Queue::in_process(name))
    }

    pub fn in_process_with_options(name: &'static str, options: QueueOptions) -> Self {
        Self::from_queue(name, Queue::in_process_with_options(name, options))
    }

    pub fn from_lane_backend_arc(
        name: &'static str,
        backend: Arc<dyn LaneJobQueueBackend>,
    ) -> Self {
        Self::from_queue(name, Queue::from_lane_backend_arc(name, backend))
    }

    pub fn from_queue(name: &'static str, queue: Queue) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Queue>(),
            queue: Arc::new(queue),
            processors: Vec::new(),
            global: false,
        }
    }

    pub fn processor<P>(mut self, name: impl Into<String>, processor: P) -> Self
    where
        P: QueueProcessor,
    {
        self.processors.push((name.into(), Arc::new(processor)));
        self
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for QueueModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.queue),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
        for (name, processor) in &self.processors {
            self.queue
                .process_arc(name.clone(), Arc::clone(processor))?;
        }
        Ok(())
    }

    fn on_application_bootstrap(&self, module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move { queue.start(module_ref).await })
    }

    fn on_application_shutdown(&self, _module_ref: ModuleRef) -> BoxFuture<'static, Result<()>> {
        let queue = Arc::clone(&self.queue);
        Box::pin(async move { queue.shutdown().await })
    }
}
