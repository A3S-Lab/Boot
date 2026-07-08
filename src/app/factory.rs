use super::{BootApplication, BootApplicationBuilder};
use crate::{BoxFuture, HttpAdapter, MessageTransport, Module, ModuleRef, Result};
use std::net::SocketAddr;
use std::sync::Arc;

/// NestFactory-style entrypoint for building managed Boot applications.
pub struct BootFactory;

impl BootFactory {
    pub fn create<M>(module: M) -> Result<BootApplicationHandle>
    where
        M: Module,
    {
        Self::create_with_builder(BootApplication::builder().import(module))
    }

    pub fn create_arc(module: Arc<dyn Module>) -> Result<BootApplicationHandle> {
        Self::create_with_builder(BootApplication::builder().import_arc(module))
    }

    pub fn create_with_builder(builder: BootApplicationBuilder) -> Result<BootApplicationHandle> {
        Ok(BootApplicationHandle::from_app(builder.build()?))
    }

    pub async fn create_async<M>(module: M) -> Result<BootApplicationHandle>
    where
        M: Module,
    {
        Self::create_with_builder_async(BootApplication::builder().import(module)).await
    }

    pub async fn create_arc_async(module: Arc<dyn Module>) -> Result<BootApplicationHandle> {
        Self::create_with_builder_async(BootApplication::builder().import_arc(module)).await
    }

    pub async fn create_with_builder_async(
        builder: BootApplicationBuilder,
    ) -> Result<BootApplicationHandle> {
        Ok(BootApplicationHandle::from_app(
            builder.build_async().await?,
        ))
    }

    pub fn create_application_context<M>(module: M) -> Result<BootApplicationContext>
    where
        M: Module,
    {
        Self::create_application_context_with_builder(BootApplication::builder().import(module))
    }

    pub fn create_application_context_arc(
        module: Arc<dyn Module>,
    ) -> Result<BootApplicationContext> {
        Self::create_application_context_with_builder(BootApplication::builder().import_arc(module))
    }

    pub fn create_application_context_with_builder(
        builder: BootApplicationBuilder,
    ) -> Result<BootApplicationContext> {
        Ok(BootApplicationContext {
            handle: Self::create_with_builder(builder)?,
        })
    }

    pub async fn create_application_context_async<M>(module: M) -> Result<BootApplicationContext>
    where
        M: Module,
    {
        Self::create_application_context_with_builder_async(
            BootApplication::builder().import(module),
        )
        .await
    }

    pub async fn create_application_context_arc_async(
        module: Arc<dyn Module>,
    ) -> Result<BootApplicationContext> {
        Self::create_application_context_with_builder_async(
            BootApplication::builder().import_arc(module),
        )
        .await
    }

    pub async fn create_application_context_with_builder_async(
        builder: BootApplicationBuilder,
    ) -> Result<BootApplicationContext> {
        Ok(BootApplicationContext {
            handle: Self::create_with_builder_async(builder).await?,
        })
    }

    pub fn create_microservice<M, T>(module: M, transport: T) -> Result<BootMicroservice<T>>
    where
        M: Module,
        T: MessageTransport,
    {
        Self::create_microservice_with_builder(BootApplication::builder().import(module), transport)
    }

    pub fn create_microservice_with_builder<T>(
        builder: BootApplicationBuilder,
        transport: T,
    ) -> Result<BootMicroservice<T>>
    where
        T: MessageTransport,
    {
        Ok(BootMicroservice::new(builder.build()?, transport))
    }

    pub async fn create_microservice_async<M, T>(
        module: M,
        transport: T,
    ) -> Result<BootMicroservice<T>>
    where
        M: Module,
        T: MessageTransport,
    {
        Self::create_microservice_with_builder_async(
            BootApplication::builder().import(module),
            transport,
        )
        .await
    }

    pub async fn create_microservice_with_builder_async<T>(
        builder: BootApplicationBuilder,
        transport: T,
    ) -> Result<BootMicroservice<T>>
    where
        T: MessageTransport,
    {
        Ok(BootMicroservice::new(
            builder.build_async().await?,
            transport,
        ))
    }
}

/// Managed application with idempotent startup and shutdown.
pub struct BootApplicationHandle {
    app: BootApplication,
    initialized: bool,
    microservices: Vec<Box<dyn ConnectedMicroservice>>,
}

impl BootApplicationHandle {
    pub fn from_app(app: BootApplication) -> Self {
        Self {
            app,
            initialized: false,
            microservices: Vec::new(),
        }
    }

    pub fn app(&self) -> &BootApplication {
        &self.app
    }

    pub fn module_ref(&self) -> &ModuleRef {
        self.app.module_ref()
    }

    pub fn into_app(self) -> BootApplication {
        self.app
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.app.get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.app.get_named::<T>(token)
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.app.get_optional::<T>()
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.app.get_optional_named::<T>(token)
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub async fn init(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        if let Err(error) = self.app.bootstrap().await {
            let _ = self.app.shutdown().await;
            return Err(error);
        }

        self.initialized = true;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        self.app.shutdown().await?;
        self.initialized = false;
        Ok(())
    }

    pub async fn listen_with<A>(&mut self, adapter: &A, addr: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        self.init().await?;
        let serve_result = adapter.serve(self.app.clone(), addr).await;
        let close_result = self.close().await;

        match (serve_result, close_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    pub fn connect_microservice<T>(&mut self, transport: T) -> usize
    where
        T: MessageTransport + Send + Sync + 'static,
    {
        let index = self.microservices.len();
        self.microservices
            .push(Box::new(ConnectedMessageTransport { transport }));
        index
    }

    pub fn connected_microservice_count(&self) -> usize {
        self.microservices.len()
    }

    pub async fn start_all_microservices(&mut self) -> Result<()> {
        self.init().await?;
        for microservice in &self.microservices {
            microservice.serve(self.app.clone()).await?;
        }
        Ok(())
    }
}

/// Managed application context for provider-only hosts and workers.
pub struct BootApplicationContext {
    handle: BootApplicationHandle,
}

impl BootApplicationContext {
    pub fn app(&self) -> &BootApplication {
        self.handle.app()
    }

    pub fn module_ref(&self) -> &ModuleRef {
        self.handle.module_ref()
    }

    pub fn into_app(self) -> BootApplication {
        self.handle.into_app()
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_named::<T>(token)
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_optional::<T>()
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        self.handle.get_optional_named::<T>(token)
    }

    pub fn is_initialized(&self) -> bool {
        self.handle.is_initialized()
    }

    pub async fn init(&mut self) -> Result<()> {
        self.handle.init().await
    }

    pub async fn close(&mut self) -> Result<()> {
        self.handle.close().await
    }
}

/// Managed standalone microservice built from Boot message patterns.
pub struct BootMicroservice<T> {
    app: BootApplication,
    transport: T,
    initialized: bool,
}

impl<T> BootMicroservice<T>
where
    T: MessageTransport,
{
    pub fn new(app: BootApplication, transport: T) -> Self {
        Self {
            app,
            transport,
            initialized: false,
        }
    }

    pub fn app(&self) -> &BootApplication {
        &self.app
    }

    pub fn transport(&self) -> &T {
        &self.transport
    }

    pub fn into_app(self) -> BootApplication {
        self.app
    }

    pub fn build_client(&self) -> Result<T::Output> {
        self.transport.build(self.app.clone())
    }

    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    pub async fn init(&mut self) -> Result<()> {
        if self.initialized {
            return Ok(());
        }

        if let Err(error) = self.app.bootstrap().await {
            let _ = self.app.shutdown().await;
            return Err(error);
        }

        self.initialized = true;
        Ok(())
    }

    pub async fn close(&mut self) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        self.app.shutdown().await?;
        self.initialized = false;
        Ok(())
    }

    pub async fn listen(&mut self) -> Result<()> {
        self.init().await?;
        let serve_result = self.transport.serve(self.app.clone()).await;
        let close_result = self.close().await;

        match (serve_result, close_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }
}

trait ConnectedMicroservice: Send + Sync {
    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>>;
}

struct ConnectedMessageTransport<T> {
    transport: T,
}

impl<T> ConnectedMicroservice for ConnectedMessageTransport<T>
where
    T: MessageTransport + Send + Sync + 'static,
{
    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        self.transport.serve(app)
    }
}
