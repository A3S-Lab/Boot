use super::{
    BootApplication, BootApplicationBuilder, BootApplicationContext, BootApplicationHandle,
    BootMicroservice,
};
use crate::{MessageTransport, Module, Result};
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

#[cfg(all(test, feature = "shutdown-hooks"))]
mod tests {
    use super::*;
    use crate::{
        BootApplication, BootError, BoxFuture, HttpAdapter, MessageTransport, Module, ModuleRef,
        ShutdownSignal,
    };
    use std::net::SocketAddr;
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    struct ShutdownHookModule {
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Module for ShutdownHookModule {
        fn name(&self) -> &'static str {
            "shutdown-hook"
        }

        fn on_module_init(&self, _module_ref: &ModuleRef) -> crate::Result<()> {
            self.log.lock().unwrap().push("init".to_string());
            Ok(())
        }

        fn on_application_bootstrap(
            &self,
            _module_ref: ModuleRef,
        ) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            Box::pin(async move {
                log.lock().unwrap().push("bootstrap".to_string());
                Ok(())
            })
        }

        fn on_module_destroy(
            &self,
            _module_ref: ModuleRef,
            signal: Option<String>,
        ) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            Box::pin(async move {
                log.lock()
                    .unwrap()
                    .push(format!("destroy:{}", signal.unwrap_or_default()));
                Ok(())
            })
        }

        fn before_application_shutdown(
            &self,
            _module_ref: ModuleRef,
            signal: Option<String>,
        ) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            Box::pin(async move {
                log.lock()
                    .unwrap()
                    .push(format!("before:{}", signal.unwrap_or_default()));
                Ok(())
            })
        }

        fn on_application_shutdown_with_signal(
            &self,
            _module_ref: ModuleRef,
            signal: Option<String>,
        ) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            Box::pin(async move {
                log.lock()
                    .unwrap()
                    .push(format!("shutdown:{}", signal.unwrap_or_default()));
                Ok(())
            })
        }
    }

    struct PendingAdapter {
        log: Arc<Mutex<Vec<String>>>,
        ready: Mutex<Option<oneshot::Sender<()>>>,
    }

    impl PendingAdapter {
        fn new(log: Arc<Mutex<Vec<String>>>, ready: oneshot::Sender<()>) -> Self {
            Self {
                log,
                ready: Mutex::new(Some(ready)),
            }
        }
    }

    impl HttpAdapter for PendingAdapter {
        type Output = ();

        fn build(&self, _app: BootApplication) -> crate::Result<Self::Output> {
            Ok(())
        }

        fn serve(
            &self,
            _app: BootApplication,
            _addr: SocketAddr,
        ) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            let ready = self.ready.lock().unwrap().take();
            Box::pin(async move {
                log.lock().unwrap().push("serve".to_string());
                if let Some(ready) = ready {
                    let _ = ready.send(());
                }
                futures_util::future::pending::<crate::Result<()>>().await
            })
        }
    }

    struct PendingTransport {
        log: Arc<Mutex<Vec<String>>>,
        ready: Mutex<Option<oneshot::Sender<()>>>,
    }

    impl PendingTransport {
        fn new(log: Arc<Mutex<Vec<String>>>, ready: oneshot::Sender<()>) -> Self {
            Self {
                log,
                ready: Mutex::new(Some(ready)),
            }
        }
    }

    impl MessageTransport for PendingTransport {
        type Output = ();

        fn build(&self, _app: BootApplication) -> crate::Result<Self::Output> {
            Ok(())
        }

        fn serve(&self, _app: BootApplication) -> BoxFuture<'static, crate::Result<()>> {
            let log = Arc::clone(&self.log);
            let ready = self.ready.lock().unwrap().take();
            Box::pin(async move {
                log.lock().unwrap().push("microservice".to_string());
                if let Some(ready) = ready {
                    let _ = ready.send(());
                }
                futures_util::future::pending::<crate::Result<()>>().await
            })
        }
    }

    #[tokio::test]
    async fn listen_with_shutdown_hooks_closes_with_signal_when_signal_wins() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = oneshot::channel();
        let mut app = BootFactory::create(ShutdownHookModule {
            log: Arc::clone(&log),
        })
        .unwrap();
        app.enable_shutdown_hooks([ShutdownSignal::Sigterm]);

        app.listen_with_shutdown_signal_future(
            &PendingAdapter::new(Arc::clone(&log), ready_tx),
            ([127, 0, 0, 1], 0).into(),
            async move {
                ready_rx.await.map_err(|error| {
                    BootError::Internal(format!("pending adapter did not start: {error}"))
                })?;
                Ok(ShutdownSignal::Sigterm)
            },
        )
        .await
        .unwrap();

        assert!(!app.is_initialized());
        assert_eq!(
            log.lock().unwrap().as_slice(),
            [
                "init",
                "bootstrap",
                "serve",
                "destroy:SIGTERM",
                "before:SIGTERM",
                "shutdown:SIGTERM"
            ]
        );
    }

    #[tokio::test]
    async fn microservice_shutdown_hooks_close_with_signal_when_signal_wins() {
        let log = Arc::new(Mutex::new(Vec::new()));
        let (ready_tx, ready_rx) = oneshot::channel();
        let mut microservice = BootFactory::create_microservice(
            ShutdownHookModule {
                log: Arc::clone(&log),
            },
            PendingTransport::new(Arc::clone(&log), ready_tx),
        )
        .unwrap();
        microservice.enable_shutdown_hooks([ShutdownSignal::Sigterm]);

        microservice
            .listen_with_shutdown_signal_future(async move {
                ready_rx.await.map_err(|error| {
                    BootError::Internal(format!("pending transport did not start: {error}"))
                })?;
                Ok(ShutdownSignal::Sigterm)
            })
            .await
            .unwrap();

        assert!(!microservice.is_initialized());
        assert_eq!(
            log.lock().unwrap().as_slice(),
            [
                "init",
                "bootstrap",
                "microservice",
                "destroy:SIGTERM",
                "before:SIGTERM",
                "shutdown:SIGTERM"
            ]
        );
    }
}
