#[cfg(feature = "shutdown-hooks")]
use super::shutdown::{normalize_shutdown_signals, wait_for_shutdown_signal, ShutdownSignal};
use super::BootApplication;
use crate::{MessageTransport, Result};
#[cfg(feature = "shutdown-hooks")]
use std::future::Future;

/// Managed standalone microservice built from Boot message patterns.
pub struct BootMicroservice<T> {
    app: BootApplication,
    transport: T,
    initialized: bool,
    #[cfg(feature = "shutdown-hooks")]
    shutdown_signals: Option<Vec<ShutdownSignal>>,
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
            #[cfg(feature = "shutdown-hooks")]
            shutdown_signals: None,
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

    pub async fn listen(&mut self) -> Result<()> {
        #[cfg(feature = "shutdown-hooks")]
        if let Some(signals) = self.shutdown_signals.clone() {
            return self
                .listen_with_shutdown_signal_future(wait_for_shutdown_signal(signals))
                .await;
        }

        self.init().await?;
        let serve_result = self.transport.serve(self.app.clone()).await;
        let close_result = self.close().await;

        match (serve_result, close_result) {
            (Err(error), _) => Err(error),
            (Ok(()), Err(error)) => Err(error),
            (Ok(()), Ok(())) => Ok(()),
        }
    }

    #[cfg(feature = "shutdown-hooks")]
    pub(crate) async fn listen_with_shutdown_signal_future<F>(
        &mut self,
        signal_future: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<ShutdownSignal>> + Send,
    {
        self.init().await?;
        let serve_future = self.transport.serve(self.app.clone());
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
}
