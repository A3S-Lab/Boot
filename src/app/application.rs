use super::builder::BootApplicationBuilder;
use crate::{HttpAdapter, Module, ModuleRef, Result, RouteDefinition};
use std::net::SocketAddr;
use std::sync::Arc;

/// Built application with a resolved module graph and framework-neutral routes.
#[derive(Clone)]
pub struct BootApplication {
    pub(crate) routes: Vec<RouteDefinition>,
    pub(crate) modules: Vec<String>,
    pub(crate) module_ref: ModuleRef,
    pub(crate) module_instances: Vec<Arc<dyn Module>>,
}

impl BootApplication {
    /// Create an application builder.
    pub fn builder() -> BootApplicationBuilder {
        BootApplicationBuilder::new()
    }

    /// Names of modules included in the application, in registration order.
    pub fn module_names(&self) -> &[String] {
        &self.modules
    }

    /// Routes exposed by the application.
    pub fn routes(&self) -> &[RouteDefinition] {
        &self.routes
    }

    /// Provider container available to hosts and controllers.
    pub fn module_ref(&self) -> &ModuleRef {
        &self.module_ref
    }

    /// Resolve a typed provider from the application container.
    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref.get::<T>()
    }

    /// Run async startup hooks before serving, when the host needs them.
    pub async fn bootstrap(&self) -> Result<()> {
        for module in &self.module_instances {
            module
                .on_application_bootstrap(self.module_ref.clone())
                .await?;
        }
        Ok(())
    }

    /// Build this application through a concrete HTTP adapter.
    pub fn into_adapter<A>(self, adapter: &A) -> Result<A::Output>
    where
        A: HttpAdapter,
    {
        adapter.build(self)
    }

    /// Serve this application through a concrete HTTP adapter.
    pub async fn serve_with<A>(self, adapter: &A, addr: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        self.bootstrap().await?;
        adapter.serve(self, addr).await
    }
}
