use crate::{
    BootRequest, BootResponse, BoxFuture, ExecutionContext, Interceptor, Middleware,
    MiddlewareOutcome, Module, ProviderDefinition, ProviderToken, Result,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};

/// Log severity understood by Boot's provider-backed logger.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
}

impl LogLevel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Trace => "trace",
            Self::Debug => "debug",
            Self::Info => "info",
            Self::Warn => "warn",
            Self::Error => "error",
        }
    }
}

impl fmt::Display for LogLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Structured fields attached to a log record.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LogFields {
    fields: BTreeMap<String, Value>,
}

impl LogFields {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_value(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.fields.insert(key.into(), value.into());
        self
    }

    pub fn with<T>(mut self, key: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.fields.insert(
            key.into(),
            serde_json::to_value(value)
                .map_err(|error| crate::BootError::Internal(error.to_string()))?,
        );
        Ok(self)
    }

    pub fn insert_value(&mut self, key: impl Into<String>, value: impl Into<Value>) {
        self.fields.insert(key.into(), value.into());
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }

    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &Value)> {
        self.fields.iter().map(|(key, value)| (key.as_str(), value))
    }

    pub fn into_inner(self) -> BTreeMap<String, Value> {
        self.fields
    }
}

impl From<BTreeMap<String, Value>> for LogFields {
    fn from(fields: BTreeMap<String, Value>) -> Self {
        Self { fields }
    }
}

/// Immutable structured log record.
#[derive(Debug, Clone, PartialEq)]
pub struct LogRecord {
    pub level: LogLevel,
    pub target: String,
    pub message: String,
    pub fields: LogFields,
}

impl LogRecord {
    pub fn field(&self, key: &str) -> Option<&Value> {
        self.fields.get(key)
    }
}

/// Backend abstraction used by [`Logger`].
pub trait LogSink: Send + Sync + 'static {
    fn log(&self, record: LogRecord) -> Result<()>;
}

/// Logger provider that writes structured records to a pluggable sink.
#[derive(Clone)]
pub struct Logger {
    sink: Arc<dyn LogSink>,
    target: String,
    fields: LogFields,
}

impl fmt::Debug for Logger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Logger")
            .field("target", &self.target)
            .field("fields", &self.fields)
            .finish_non_exhaustive()
    }
}

impl Logger {
    pub fn new<S>(sink: S) -> Self
    where
        S: LogSink,
    {
        Self::from_sink_arc(Arc::new(sink))
    }

    pub fn from_sink_arc(sink: Arc<dyn LogSink>) -> Self {
        Self {
            sink,
            target: "a3s.boot".to_string(),
            fields: LogFields::new(),
        }
    }

    pub fn noop() -> Self {
        Self::new(NoopLogSink)
    }

    pub fn in_memory() -> Self {
        Self::new(InMemoryLogSink::new())
    }

    pub fn target(&self) -> &str {
        &self.target
    }

    pub fn with_target(mut self, target: impl Into<String>) -> Self {
        self.target = target.into();
        self
    }

    pub fn child(&self, target: impl Into<String>) -> Self {
        let mut logger = self.clone();
        logger.target = target.into();
        logger
    }

    pub fn with_default_field<T>(mut self, key: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.fields = self.fields.with(key, value)?;
        Ok(self)
    }

    pub fn log(&self, level: LogLevel, message: impl Into<String>) -> Result<()> {
        self.log_with_fields(level, message, LogFields::new())
    }

    pub fn log_with_fields(
        &self,
        level: LogLevel,
        message: impl Into<String>,
        fields: LogFields,
    ) -> Result<()> {
        self.sink.log(LogRecord {
            level,
            target: self.target.clone(),
            message: message.into(),
            fields: merge_fields(&self.fields, fields),
        })
    }

    pub fn trace(&self, message: impl Into<String>) -> Result<()> {
        self.log(LogLevel::Trace, message)
    }

    pub fn debug(&self, message: impl Into<String>) -> Result<()> {
        self.log(LogLevel::Debug, message)
    }

    pub fn info(&self, message: impl Into<String>) -> Result<()> {
        self.log(LogLevel::Info, message)
    }

    pub fn warn(&self, message: impl Into<String>) -> Result<()> {
        self.log(LogLevel::Warn, message)
    }

    pub fn error(&self, message: impl Into<String>) -> Result<()> {
        self.log(LogLevel::Error, message)
    }
}

fn merge_fields(defaults: &LogFields, fields: LogFields) -> LogFields {
    let mut merged = defaults.clone().into_inner();
    for (key, value) in fields.into_inner() {
        merged.insert(key, value);
    }
    merged.into()
}

/// Sink that discards records. Useful as a default or for tests that only need
/// a logger provider to exist.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoopLogSink;

impl LogSink for NoopLogSink {
    fn log(&self, _record: LogRecord) -> Result<()> {
        Ok(())
    }
}

/// In-memory log sink intended for tests and local diagnostics.
#[derive(Debug, Clone, Default)]
pub struct InMemoryLogSink {
    records: Arc<RwLock<Vec<LogRecord>>>,
}

impl InMemoryLogSink {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn records(&self) -> Result<Vec<LogRecord>> {
        Ok(self.read_records()?.clone())
    }

    pub fn clear(&self) -> Result<()> {
        self.write_records()?.clear();
        Ok(())
    }

    fn read_records(&self) -> Result<std::sync::RwLockReadGuard<'_, Vec<LogRecord>>> {
        self.records
            .read()
            .map_err(|_| crate::BootError::Internal("log sink lock is poisoned".to_string()))
    }

    fn write_records(&self) -> Result<std::sync::RwLockWriteGuard<'_, Vec<LogRecord>>> {
        self.records
            .write()
            .map_err(|_| crate::BootError::Internal("log sink lock is poisoned".to_string()))
    }
}

impl LogSink for InMemoryLogSink {
    fn log(&self, record: LogRecord) -> Result<()> {
        self.write_records()?.push(record);
        Ok(())
    }
}

/// Middleware that logs an incoming request before it reaches guards,
/// interceptors, pipes, and handlers.
#[derive(Debug, Clone)]
pub struct RequestLoggingMiddleware {
    logger: Arc<Logger>,
}

impl RequestLoggingMiddleware {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self { logger }
    }
}

impl Middleware for RequestLoggingMiddleware {
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        let logger = Arc::clone(&self.logger);
        Box::pin(async move {
            logger.log_with_fields(
                LogLevel::Info,
                "request received",
                request_fields(&request)
                    .with_value("body_bytes", Value::from(request.body().len() as u64)),
            )?;
            Ok(MiddlewareOutcome::next(request))
        })
    }
}

/// Interceptor that logs route execution before and after the handler.
#[derive(Debug, Clone)]
pub struct RequestLoggingInterceptor {
    logger: Arc<Logger>,
}

impl RequestLoggingInterceptor {
    pub fn new(logger: Arc<Logger>) -> Self {
        Self { logger }
    }
}

impl Interceptor for RequestLoggingInterceptor {
    fn before(&self, context: ExecutionContext) -> BoxFuture<'static, Result<()>> {
        let logger = Arc::clone(&self.logger);
        Box::pin(async move {
            logger.log_with_fields(
                LogLevel::Info,
                "request started",
                execution_fields(&context),
            )
        })
    }

    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let logger = Arc::clone(&self.logger);
        Box::pin(async move {
            logger.log_with_fields(
                LogLevel::Info,
                "request completed",
                execution_fields(&context).with_value("status", Value::from(response.status())),
            )?;
            Ok(response)
        })
    }
}

fn request_fields(request: &BootRequest) -> LogFields {
    let mut fields = LogFields::new()
        .with_value("method", Value::from(request.method().as_str()))
        .with_value("path", Value::from(request.path()));
    if let Some(query) = request.query_string() {
        fields.insert_value("query", Value::from(query));
    }
    fields
}

fn execution_fields(context: &ExecutionContext) -> LogFields {
    let mut fields = LogFields::new()
        .with_value("method", Value::from(context.method.as_str()))
        .with_value("request_path", Value::from(context.request_path.clone()))
        .with_value("route_path", Value::from(context.route_path.clone()));

    if let Some(module_name) = &context.module_name {
        fields.insert_value("module", Value::from(module_name.clone()));
    }
    if let Some(controller_prefix) = &context.controller_prefix {
        fields.insert_value("controller", Value::from(controller_prefix.clone()));
    }

    fields
}

/// Module that registers and exports a [`Logger`] provider.
#[derive(Clone)]
pub struct LoggingModule {
    name: &'static str,
    token: ProviderToken,
    logger: Arc<Logger>,
    global: bool,
}

impl fmt::Debug for LoggingModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LoggingModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("logger", &self.logger)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl LoggingModule {
    pub fn noop(name: &'static str) -> Self {
        Self::from_logger(name, Logger::noop())
    }

    pub fn in_memory(name: &'static str) -> Self {
        Self::from_logger(name, Logger::in_memory())
    }

    pub fn from_logger(name: &'static str, logger: Logger) -> Self {
        Self {
            name,
            token: ProviderToken::of::<Logger>(),
            logger: Arc::new(logger),
            global: false,
        }
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

impl Module for LoggingModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.logger),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}
