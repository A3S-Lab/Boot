#[cfg(feature = "shutdown-hooks")]
use super::shutdown::{normalize_shutdown_signals, wait_for_shutdown_signal, ShutdownSignal};
use super::BootApplication;
use crate::{BoxFuture, HttpAdapter, MessageTransport, ModuleRef, Result};
#[cfg(feature = "shutdown-hooks")]
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

/// Managed application with idempotent startup and shutdown.
pub struct BootApplicationHandle {
    app: BootApplication,
    initialized: bool,
    microservices: Vec<Box<dyn ConnectedMicroservice>>,
    #[cfg(feature = "shutdown-hooks")]
    shutdown_signals: Option<Vec<ShutdownSignal>>,
}

impl BootApplicationHandle {
    pub fn from_app(app: BootApplication) -> Self {
        Self {
            app,
            initialized: false,
            microservices: Vec::new(),
            #[cfg(feature = "shutdown-hooks")]
            shutdown_signals: None,
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

    #[cfg(feature = "shutdown-hooks")]
    pub fn enable_shutdown_hooks<I>(&mut self, signals: I) -> &mut Self
    where
        I: IntoIterator<Item = ShutdownSignal>,
    {
        self.shutdown_signals = Some(normalize_shutdown_signals(signals));
        self
    }

    #[cfg(feature = "shutdown-hooks")]
    pub fn enable_default_shutdown_hooks(&mut self) -> &mut Self {
        self.enable_shutdown_hooks(ShutdownSignal::default_signals())
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
        self.close_inner(None).await
    }

    pub async fn close_with_signal(&mut self, signal: impl Into<String>) -> Result<()> {
        self.close_inner(Some(signal.into())).await
    }

    async fn close_inner(&mut self, signal: Option<String>) -> Result<()> {
        if !self.initialized {
            return Ok(());
        }

        match signal {
            Some(signal) => self.app.shutdown_with_signal(signal).await?,
            None => self.app.shutdown().await?,
        }
        self.initialized = false;
        Ok(())
    }

    pub async fn listen_with<A>(&mut self, adapter: &A, addr: SocketAddr) -> Result<()>
    where
        A: HttpAdapter,
    {
        #[cfg(feature = "shutdown-hooks")]
        if let Some(signals) = self.shutdown_signals.clone() {
            return self
                .listen_with_shutdown_signal_future(
                    adapter,
                    addr,
                    wait_for_shutdown_signal(signals),
                )
                .await;
        }

        self.init().await?;
        let serve_result = adapter.serve(self.app.clone(), addr).await;
        let close_result = self.close().await;

        match (serve_result, close_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[cfg(feature = "shutdown-hooks")]
    pub(crate) async fn listen_with_shutdown_signal_future<A, F>(
        &mut self,
        adapter: &A,
        addr: SocketAddr,
        signal_future: F,
    ) -> Result<()>
    where
        A: HttpAdapter,
        F: Future<Output = Result<ShutdownSignal>> + Send,
    {
        self.init().await?;
        let serve_future = adapter.serve(self.app.clone(), addr);
        futures_util::pin_mut!(signal_future);
        futures_util::pin_mut!(serve_future);

        tokio::select! {
            serve_result = &mut serve_future => {
                let close_result = self.close().await;
                match (serve_result, close_result) {
                    (Err(error), _) => Err(error),
                    (Ok(()), Err(error)) => Err(error),
                    (Ok(()), Ok(())) => Ok(()),
                }
            }
            signal_result = &mut signal_future => {
                match signal_result {
                    Ok(signal) => self.close_with_signal(signal.as_str()).await,
                    Err(error) => {
                        let _ = self.close().await;
                        Err(error)
                    }
                }
            }
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
