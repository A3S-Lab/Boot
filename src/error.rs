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
    #[error("http exception {status}: {message}")]
    HttpException { status: u16, message: String },
    #[error("request was forbidden: {0}")]
    Forbidden(String),
    #[error("request was unauthorized: {0}")]
    Unauthorized(String),
    #[error("bad request: {0}")]
    BadRequest(String),
    #[error("request timed out: {0}")]
    RequestTimeout(String),
    #[error("resource conflict: {0}")]
    Conflict(String),
    #[error("resource is gone: {0}")]
    Gone(String),
    #[error("precondition failed: {0}")]
    PreconditionFailed(String),
    #[error("payload is too large: {0}")]
    PayloadTooLarge(String),
    #[error("unsupported media type: {0}")]
    UnsupportedMediaType(String),
    #[error("not acceptable: {0}")]
    NotAcceptable(String),
    #[error("I am a teapot: {0}")]
    ImATeapot(String),
    #[error("unprocessable entity: {0}")]
    UnprocessableEntity(String),
    #[error("too many requests: {0}")]
    TooManyRequests(String),
    #[error("internal server error: {0}")]
    InternalServerError(String),
    #[error("not implemented: {0}")]
    NotImplemented(String),
    #[error("bad gateway: {0}")]
    BadGateway(String),
    #[error("service unavailable: {0}")]
    ServiceUnavailable(String),
    #[error("gateway timeout: {0}")]
    GatewayTimeout(String),
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
    HttpException,
    Forbidden,
    Unauthorized,
    BadRequest,
    RequestTimeout,
    Conflict,
    Gone,
    PreconditionFailed,
    PayloadTooLarge,
    UnsupportedMediaType,
    NotAcceptable,
    ImATeapot,
    UnprocessableEntity,
    TooManyRequests,
    InternalServerError,
    NotImplemented,
    BadGateway,
    ServiceUnavailable,
    GatewayTimeout,
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
            Self::HttpException { .. } => BootErrorKind::HttpException,
            Self::Forbidden(_) => BootErrorKind::Forbidden,
            Self::Unauthorized(_) => BootErrorKind::Unauthorized,
            Self::BadRequest(_) => BootErrorKind::BadRequest,
            Self::RequestTimeout(_) => BootErrorKind::RequestTimeout,
            Self::Conflict(_) => BootErrorKind::Conflict,
            Self::Gone(_) => BootErrorKind::Gone,
            Self::PreconditionFailed(_) => BootErrorKind::PreconditionFailed,
            Self::PayloadTooLarge(_) => BootErrorKind::PayloadTooLarge,
            Self::UnsupportedMediaType(_) => BootErrorKind::UnsupportedMediaType,
            Self::NotAcceptable(_) => BootErrorKind::NotAcceptable,
            Self::ImATeapot(_) => BootErrorKind::ImATeapot,
            Self::UnprocessableEntity(_) => BootErrorKind::UnprocessableEntity,
            Self::TooManyRequests(_) => BootErrorKind::TooManyRequests,
            Self::InternalServerError(_) => BootErrorKind::InternalServerError,
            Self::NotImplemented(_) => BootErrorKind::NotImplemented,
            Self::BadGateway(_) => BootErrorKind::BadGateway,
            Self::ServiceUnavailable(_) => BootErrorKind::ServiceUnavailable,
            Self::GatewayTimeout(_) => BootErrorKind::GatewayTimeout,
            Self::Adapter(_) => BootErrorKind::Adapter,
            Self::Internal(_) => BootErrorKind::Internal,
            Self::Io(_) => BootErrorKind::Io,
        }
    }

    /// HTTP status code that adapters should use for this error.
    pub fn http_status_code(&self) -> u16 {
        match self {
            Self::HttpException { status, .. } => *status,
            Self::NotFound(_) => 404,
            Self::MethodNotAllowed(_) => 405,
            Self::Unauthorized(_) => 401,
            Self::Forbidden(_) => 403,
            Self::BadRequest(_) => 400,
            Self::RequestTimeout(_) => 408,
            Self::Conflict(_) => 409,
            Self::Gone(_) => 410,
            Self::PreconditionFailed(_) => 412,
            Self::PayloadTooLarge(_) => 413,
            Self::UnsupportedMediaType(_) => 415,
            Self::NotAcceptable(_) => 406,
            Self::ImATeapot(_) => 418,
            Self::UnprocessableEntity(_) => 422,
            Self::TooManyRequests(_) => 429,
            Self::InternalServerError(_) => 500,
            Self::NotImplemented(_) => 501,
            Self::BadGateway(_) => 502,
            Self::ServiceUnavailable(_) => 503,
            Self::GatewayTimeout(_) => 504,
            _ => 500,
        }
    }

    /// Text response body that adapters should use for this error.
    pub fn http_response_message(&self) -> String {
        match self {
            Self::HttpException { message, .. }
            | Self::RequestTimeout(message)
            | Self::Conflict(message)
            | Self::Gone(message)
            | Self::PreconditionFailed(message)
            | Self::ImATeapot(message)
            | Self::UnprocessableEntity(message)
            | Self::InternalServerError(message)
            | Self::NotImplemented(message)
            | Self::BadGateway(message)
            | Self::ServiceUnavailable(message)
            | Self::GatewayTimeout(message)
            | Self::TooManyRequests(message)
            | Self::NotAcceptable(message)
            | Self::UnsupportedMediaType(message)
            | Self::PayloadTooLarge(message)
            | Self::BadRequest(message)
            | Self::Forbidden(message)
            | Self::Unauthorized(message)
            | Self::MethodNotAllowed(message)
            | Self::NotFound(message) => message.clone(),
            error => error.to_string(),
        }
    }

    pub fn http_exception(status: u16, message: impl Into<String>) -> crate::Result<Self> {
        if (100..600).contains(&status) {
            return Ok(Self::from_http_status(status, message));
        }

        Err(Self::Internal(format!(
            "invalid HTTP exception status {status}"
        )))
    }

    pub fn from_http_status(status: u16, message: impl Into<String>) -> Self {
        let message = message.into();
        match status {
            400 => Self::BadRequest(message),
            401 => Self::Unauthorized(message),
            403 => Self::Forbidden(message),
            404 => Self::NotFound(message),
            405 => Self::MethodNotAllowed(message),
            406 => Self::NotAcceptable(message),
            408 => Self::RequestTimeout(message),
            409 => Self::Conflict(message),
            410 => Self::Gone(message),
            412 => Self::PreconditionFailed(message),
            413 => Self::PayloadTooLarge(message),
            415 => Self::UnsupportedMediaType(message),
            418 => Self::ImATeapot(message),
            422 => Self::UnprocessableEntity(message),
            429 => Self::TooManyRequests(message),
            500 => Self::InternalServerError(message),
            501 => Self::NotImplemented(message),
            502 => Self::BadGateway(message),
            503 => Self::ServiceUnavailable(message),
            504 => Self::GatewayTimeout(message),
            status => Self::HttpException { status, message },
        }
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::NotFound(message.into())
    }

    pub fn method_not_allowed(message: impl Into<String>) -> Self {
        Self::MethodNotAllowed(message.into())
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::Unauthorized(message.into())
    }

    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::Forbidden(message.into())
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::BadRequest(message.into())
    }

    pub fn request_timeout(message: impl Into<String>) -> Self {
        Self::RequestTimeout(message.into())
    }

    pub fn conflict(message: impl Into<String>) -> Self {
        Self::Conflict(message.into())
    }

    pub fn gone(message: impl Into<String>) -> Self {
        Self::Gone(message.into())
    }

    pub fn precondition_failed(message: impl Into<String>) -> Self {
        Self::PreconditionFailed(message.into())
    }

    pub fn payload_too_large(message: impl Into<String>) -> Self {
        Self::PayloadTooLarge(message.into())
    }

    pub fn unsupported_media_type(message: impl Into<String>) -> Self {
        Self::UnsupportedMediaType(message.into())
    }

    pub fn not_acceptable(message: impl Into<String>) -> Self {
        Self::NotAcceptable(message.into())
    }

    pub fn im_a_teapot(message: impl Into<String>) -> Self {
        Self::ImATeapot(message.into())
    }

    pub fn unprocessable_entity(message: impl Into<String>) -> Self {
        Self::UnprocessableEntity(message.into())
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::TooManyRequests(message.into())
    }

    pub fn internal_server_error(message: impl Into<String>) -> Self {
        Self::InternalServerError(message.into())
    }

    pub fn not_implemented(message: impl Into<String>) -> Self {
        Self::NotImplemented(message.into())
    }

    pub fn bad_gateway(message: impl Into<String>) -> Self {
        Self::BadGateway(message.into())
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::ServiceUnavailable(message.into())
    }

    pub fn gateway_timeout(message: impl Into<String>) -> Self {
        Self::GatewayTimeout(message.into())
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::Internal(message.into())
    }

    pub(crate) fn clone_for_filter(&self) -> Self {
        match self {
            Self::HttpException { status, message } => Self::HttpException {
                status: *status,
                message: message.clone(),
            },
            Self::NotFound(message)
            | Self::MethodNotAllowed(message)
            | Self::Forbidden(message)
            | Self::Unauthorized(message)
            | Self::BadRequest(message)
            | Self::RequestTimeout(message)
            | Self::Conflict(message)
            | Self::Gone(message)
            | Self::PreconditionFailed(message)
            | Self::PayloadTooLarge(message)
            | Self::UnsupportedMediaType(message)
            | Self::NotAcceptable(message)
            | Self::ImATeapot(message)
            | Self::UnprocessableEntity(message)
            | Self::TooManyRequests(message)
            | Self::InternalServerError(message)
            | Self::NotImplemented(message)
            | Self::BadGateway(message)
            | Self::ServiceUnavailable(message)
            | Self::GatewayTimeout(message)
            | Self::Adapter(message)
            | Self::Internal(message)
            | Self::InvalidRoutePath(message)
            | Self::InvalidHostPattern(message)
            | Self::DuplicateRoute(message)
            | Self::DuplicateProvider(message)
            | Self::MissingProvider(message)
            | Self::ProviderTypeMismatch(message) => {
                Self::from_kind_and_message(self.kind(), message)
            }
            Self::EmptyModuleName => Self::EmptyModuleName,
            Self::Io(error) => Self::Io(std::io::Error::new(error.kind(), error.to_string())),
        }
    }

    fn from_kind_and_message(kind: BootErrorKind, message: &str) -> Self {
        match kind {
            BootErrorKind::EmptyModuleName => Self::EmptyModuleName,
            BootErrorKind::InvalidRoutePath => Self::InvalidRoutePath(message.to_string()),
            BootErrorKind::InvalidHostPattern => Self::InvalidHostPattern(message.to_string()),
            BootErrorKind::DuplicateRoute => Self::DuplicateRoute(message.to_string()),
            BootErrorKind::NotFound => Self::NotFound(message.to_string()),
            BootErrorKind::MethodNotAllowed => Self::MethodNotAllowed(message.to_string()),
            BootErrorKind::DuplicateProvider => Self::DuplicateProvider(message.to_string()),
            BootErrorKind::MissingProvider => Self::MissingProvider(message.to_string()),
            BootErrorKind::ProviderTypeMismatch => Self::ProviderTypeMismatch(message.to_string()),
            BootErrorKind::HttpException => Self::HttpException {
                status: 500,
                message: message.to_string(),
            },
            BootErrorKind::Forbidden => Self::Forbidden(message.to_string()),
            BootErrorKind::Unauthorized => Self::Unauthorized(message.to_string()),
            BootErrorKind::BadRequest => Self::BadRequest(message.to_string()),
            BootErrorKind::RequestTimeout => Self::RequestTimeout(message.to_string()),
            BootErrorKind::Conflict => Self::Conflict(message.to_string()),
            BootErrorKind::Gone => Self::Gone(message.to_string()),
            BootErrorKind::PreconditionFailed => Self::PreconditionFailed(message.to_string()),
            BootErrorKind::PayloadTooLarge => Self::PayloadTooLarge(message.to_string()),
            BootErrorKind::UnsupportedMediaType => Self::UnsupportedMediaType(message.to_string()),
            BootErrorKind::NotAcceptable => Self::NotAcceptable(message.to_string()),
            BootErrorKind::ImATeapot => Self::ImATeapot(message.to_string()),
            BootErrorKind::UnprocessableEntity => Self::UnprocessableEntity(message.to_string()),
            BootErrorKind::TooManyRequests => Self::TooManyRequests(message.to_string()),
            BootErrorKind::InternalServerError => Self::InternalServerError(message.to_string()),
            BootErrorKind::NotImplemented => Self::NotImplemented(message.to_string()),
            BootErrorKind::BadGateway => Self::BadGateway(message.to_string()),
            BootErrorKind::ServiceUnavailable => Self::ServiceUnavailable(message.to_string()),
            BootErrorKind::GatewayTimeout => Self::GatewayTimeout(message.to_string()),
            BootErrorKind::Adapter => Self::Adapter(message.to_string()),
            BootErrorKind::Internal => Self::Internal(message.to_string()),
            BootErrorKind::Io => Self::Io(std::io::Error::other(message.to_string())),
        }
    }
}
