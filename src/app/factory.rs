use super::{BootApplication, BootApplicationBuilder};
use crate::{BoxFuture, HttpAdapter, MessageTransport, Module, ModuleRef, Result};
#[cfg(feature = "shutdown-hooks")]
use futures_util::StreamExt;
#[cfg(feature = "shutdown-hooks")]
use std::collections::BTreeSet;
#[cfg(feature = "shutdown-hooks")]
use std::fmt;
#[cfg(feature = "shutdown-hooks")]
use std::future::Future;
use std::net::SocketAddr;
use std::sync::Arc;

/// Shutdown signal names understood by Nest-style shutdown hooks.
#[cfg(feature = "shutdown-hooks")]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ShutdownSignal {
    Sigint,
    Sigterm,
    Sigquit,
    Sighup,
    Sigusr2,
}

#[cfg(feature = "shutdown-hooks")]
impl ShutdownSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sigint => "SIGINT",
            Self::Sigterm => "SIGTERM",
            Self::Sigquit => "SIGQUIT",
            Self::Sighup => "SIGHUP",
            Self::Sigusr2 => "SIGUSR2",
        }
    }

    pub fn default_signals() -> Vec<Self> {
        vec![Self::Sigint, Self::Sigterm]
    }
}

#[cfg(feature = "shutdown-hooks")]
impl fmt::Display for ShutdownSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Wait for one configured operating-system shutdown signal.
#[cfg(feature = "shutdown-hooks")]
pub async fn wait_for_shutdown_signal<I>(signals: I) -> Result<ShutdownSignal>
where
    I: IntoIterator<Item = ShutdownSignal>,
{
    let signals = normalize_shutdown_signals(signals);
    let mut futures = futures_util::stream::FuturesUnordered::new();
    for signal in signals {
        futures.push(shutdown_signal_future(signal)?);
    }

    futures.next().await.unwrap_or_else(|| {
        Err(crate::BootError::Internal(
            "no shutdown signals configured".into(),
        ))
    })
}

#[cfg(feature = "shutdown-hooks")]
fn normalize_shutdown_signals<I>(signals: I) -> Vec<ShutdownSignal>
where
    I: IntoIterator<Item = ShutdownSignal>,
{
    let signals = signals.into_iter().collect::<BTreeSet<_>>();
    if signals.is_empty() {
        ShutdownSignal::default_signals()
    } else {
        signals.into_iter().collect()
    }
}

#[cfg(feature = "shutdown-hooks")]
fn shutdown_signal_future(
    signal: ShutdownSignal,
) -> Result<BoxFuture<'static, Result<ShutdownSignal>>> {
    match signal {
        ShutdownSignal::Sigint => Ok(Box::pin(async move {
            tokio::signal::ctrl_c().await?;
            Ok(ShutdownSignal::Sigint)
        })),
        #[cfg(unix)]
        ShutdownSignal::Sigterm => unix_shutdown_signal_future(
            ShutdownSignal::Sigterm,
            tokio::signal::unix::SignalKind::terminate(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sigquit => unix_shutdown_signal_future(
            ShutdownSignal::Sigquit,
            tokio::signal::unix::SignalKind::quit(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sighup => unix_shutdown_signal_future(
            ShutdownSignal::Sighup,
            tokio::signal::unix::SignalKind::hangup(),
        ),
        #[cfg(unix)]
        ShutdownSignal::Sigusr2 => unix_shutdown_signal_future(
            ShutdownSignal::Sigusr2,
            tokio::signal::unix::SignalKind::user_defined2(),
        ),
        #[cfg(not(unix))]
        signal => Ok(Box::pin(async move {
            Err(crate::BootError::Internal(format!(
                "shutdown signal {signal} is only supported on Unix platforms"
            )))
        })),
    }
}

#[cfg(all(feature = "shutdown-hooks", unix))]
fn unix_shutdown_signal_future(
    signal: ShutdownSignal,
    kind: tokio::signal::unix::SignalKind,
) -> Result<BoxFuture<'static, Result<ShutdownSignal>>> {
    let mut stream = tokio::signal::unix::signal(kind)?;
    Ok(Box::pin(async move {
        let _ = stream.recv().await;
        Ok(signal)
    }))
}

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
    async fn listen_with_shutdown_signal_future<A, F>(
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

    pub async fn close_with_signal(&mut self, signal: impl Into<String>) -> Result<()> {
        self.handle.close_with_signal(signal).await
    }
}

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
    async fn listen_with_shutdown_signal_future<F>(&mut self, signal_future: F) -> Result<()>
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

#[cfg(all(test, feature = "shutdown-hooks"))]
mod tests {
    use super::*;
    use crate::{BootError, Module};
    use std::sync::{Arc, Mutex};
    use tokio::sync::oneshot;

    struct ShutdownHookModule {
        log: Arc<Mutex<Vec<String>>>,
    }

    impl Module for ShutdownHookModule {
        fn name(&self) -> &'static str {
            "shutdown-hook"
        }

        fn on_module_init(&self, _module_ref: &ModuleRef) -> Result<()> {
            self.log.lock().unwrap().push("init".to_string());
            Ok(())
        }

        fn on_application_bootstrap(
            &self,
            _module_ref: ModuleRef,
        ) -> BoxFuture<'static, Result<()>> {
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
        ) -> BoxFuture<'static, Result<()>> {
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
        ) -> BoxFuture<'static, Result<()>> {
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
        ) -> BoxFuture<'static, Result<()>> {
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

        fn build(&self, _app: BootApplication) -> Result<Self::Output> {
            Ok(())
        }

        fn serve(
            &self,
            _app: BootApplication,
            _addr: SocketAddr,
        ) -> BoxFuture<'static, Result<()>> {
            let log = Arc::clone(&self.log);
            let ready = self.ready.lock().unwrap().take();
            Box::pin(async move {
                log.lock().unwrap().push("serve".to_string());
                if let Some(ready) = ready {
                    let _ = ready.send(());
                }
                futures_util::future::pending::<Result<()>>().await
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

        fn build(&self, _app: BootApplication) -> Result<Self::Output> {
            Ok(())
        }

        fn serve(&self, _app: BootApplication) -> BoxFuture<'static, Result<()>> {
            let log = Arc::clone(&self.log);
            let ready = self.ready.lock().unwrap().take();
            Box::pin(async move {
                log.lock().unwrap().push("microservice".to_string());
                if let Some(ready) = ready {
                    let _ = ready.send(());
                }
                futures_util::future::pending::<Result<()>>().await
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
