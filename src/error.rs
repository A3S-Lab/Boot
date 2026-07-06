use thiserror::Error;

/// Errors returned while building or serving a Boot application.
#[derive(Debug, Error)]
pub enum BootError {
    #[error("module name cannot be empty")]
    EmptyModuleName,
    #[error("route path must start with '/': {0}")]
    InvalidRoutePath(String),
    #[error("provider token is already registered: {0}")]
    DuplicateProvider(String),
    #[error("provider token is not registered: {0}")]
    MissingProvider(String),
    #[error("provider token has a different concrete type: {0}")]
    ProviderTypeMismatch(String),
    #[error("request was forbidden: {0}")]
    Forbidden(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("adapter error: {0}")]
    Adapter(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
