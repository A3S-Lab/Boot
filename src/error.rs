use thiserror::Error;

/// Errors returned while building or serving a Boot application.
#[derive(Debug, Error)]
pub enum BootError {
    #[error("module name cannot be empty")]
    EmptyModuleName,
    #[error("route path must start with '/': {0}")]
    InvalidRoutePath(String),
    #[error("host pattern is invalid: {0}")]
    InvalidHostPattern(String),
    #[error("route is already registered: {0}")]
    DuplicateRoute(String),
    #[error("route was not found: {0}")]
    NotFound(String),
    #[error("method is not allowed: {0}")]
    MethodNotAllowed(String),
    #[error("provider token is already registered: {0}")]
    DuplicateProvider(String),
    #[error("provider token is not registered: {0}")]
    MissingProvider(String),
    #[error("provider token has a different concrete type: {0}")]
    ProviderTypeMismatch(String),
    #[error("request was forbidden: {0}")]
    Forbidden(String),
    #[error("request was unauthorized: {0}")]
    Unauthorized(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("payload is too large: {0}")]
    PayloadTooLarge(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("not acceptable: {0}")]
    NotAcceptable(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    #[error("adapter error: {0}")]
    Adapter(String),
    #[error("internal error: {0}")]
    Internal(String),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

/// Stable category for a [`BootError`], useful for Nest-style catch filters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum BootErrorKind {
    EmptyModuleName,
    InvalidRoutePath,
    InvalidHostPattern,
    DuplicateRoute,
    NotFound,
    MethodNotAllowed,
    DuplicateProvider,
    MissingProvider,
    ProviderTypeMismatch,
    Forbidden,
    Unauthorized,
    BadRequest,
    PayloadTooLarge,
    UnsupportedMediaType,
    NotAcceptable,
    TooManyRequests,
    Adapter,
    Internal,
    Io,
}

impl BootError {
    /// Stable category for this error.
    pub fn kind(&self) -> BootErrorKind {
        match self {
            Self::EmptyModuleName => BootErrorKind::EmptyModuleName,
            Self::InvalidRoutePath(_) => BootErrorKind::InvalidRoutePath,
            Self::InvalidHostPattern(_) => BootErrorKind::InvalidHostPattern,
            Self::DuplicateRoute(_) => BootErrorKind::DuplicateRoute,
            Self::NotFound(_) => BootErrorKind::NotFound,
            Self::MethodNotAllowed(_) => BootErrorKind::MethodNotAllowed,
            Self::DuplicateProvider(_) => BootErrorKind::DuplicateProvider,
            Self::MissingProvider(_) => BootErrorKind::MissingProvider,
            Self::ProviderTypeMismatch(_) => BootErrorKind::ProviderTypeMismatch,
            Self::Forbidden(_) => BootErrorKind::Forbidden,
            Self::Unauthorized(_) => BootErrorKind::Unauthorized,
            Self::BadRequest(_) => BootErrorKind::BadRequest,
            Self::PayloadTooLarge(_) => BootErrorKind::PayloadTooLarge,
            Self::UnsupportedMediaType(_) => BootErrorKind::UnsupportedMediaType,
            Self::NotAcceptable(_) => BootErrorKind::NotAcceptable,
            Self::TooManyRequests(_) => BootErrorKind::TooManyRequests,
            Self::Adapter(_) => BootErrorKind::Adapter,
            Self::Internal(_) => BootErrorKind::Internal,
            Self::Io(_) => BootErrorKind::Io,
        }
    }

    /// HTTP status code that adapters should use for this error.
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::NotFound(_) => 404,
            Self::MethodNotAllowed(_) => 405,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::BadRequest(_) => 400,
            Self::PayloadTooLarge(_) => 413,
            Self::UnsupportedMediaType(_) => 415,
            Self::NotAcceptable(_) => 406,
            Self::TooManyRequests(_) => 429,
            _ => 500,
        }
    }

    /// Text response body that adapters should use for this error.
    pub fn http_response_message(&self) -> String {
        match self {
            Self::NotFound(message)
            | Self::MethodNotAllowed(message)
            | Self::Unauthorized(message)
            | Self::Forbidden(message)
            | Self::BadRequest(message)
            | Self::PayloadTooLarge(message)
            | Self::UnsupportedMediaType(message)
            | Self::NotAcceptable(message)
            | Self::TooManyRequests(message) => message.clone(),
            error => error.to_string(),
        }
    }

    pub(crate) fn clone_for_filter(&self) -> Self {
        match self {
            Self::EmptyModuleName => Self::EmptyModuleName,
            Self::InvalidRoutePath(message) => Self::InvalidRoutePath(message.clone()),
            Self::InvalidHostPattern(message) => Self::InvalidHostPattern(message.clone()),
            Self::DuplicateRoute(message) => Self::DuplicateRoute(message.clone()),
            Self::NotFound(message) => Self::NotFound(message.clone()),
            Self::MethodNotAllowed(message) => Self::MethodNotAllowed(message.clone()),
            Self::DuplicateProvider(message) => Self::DuplicateProvider(message.clone()),
            Self::MissingProvider(message) => Self::MissingProvider(message.clone()),
            Self::ProviderTypeMismatch(message) => Self::ProviderTypeMismatch(message.clone()),
            Self::Forbidden(message) => Self::Forbidden(message.clone()),
            Self::Unauthorized(message) => Self::Unauthorized(message.clone()),
            Self::BadRequest(message) => Self::BadRequest(message.clone()),
            Self::PayloadTooLarge(message) => Self::PayloadTooLarge(message.clone()),
            Self::UnsupportedMediaType(message) => Self::UnsupportedMediaType(message.clone()),
            Self::NotAcceptable(message) => Self::NotAcceptable(message.clone()),
            Self::TooManyRequests(message) => Self::TooManyRequests(message.clone()),
            Self::Adapter(message) => Self::Adapter(message.clone()),
            Self::Internal(message) => Self::Internal(message.clone()),
            Self::Io(error) => Self::Io(std::io::Error::new(error.kind(), error.to_string())),
        }
    }
}
