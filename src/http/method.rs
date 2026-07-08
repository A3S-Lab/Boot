use crate::BootError;
use std::fmt;
use std::str::FromStr;

/// HTTP method understood by Boot route definitions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum HttpMethod {
    /// Route-definition wildcard that matches every standard HTTP method.
    All,
    Get,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
}

impl HttpMethod {
    pub const STANDARD: [Self; 7] = [
        Self::Get,
        Self::Post,
        Self::Put,
        Self::Patch,
        Self::Delete,
        Self::Options,
        Self::Head,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::All => "ALL",
            Self::Get => "GET",
            Self::Post => "POST",
            Self::Put => "PUT",
            Self::Patch => "PATCH",
            Self::Delete => "DELETE",
            Self::Options => "OPTIONS",
            Self::Head => "HEAD",
        }
    }

    pub fn standard_methods() -> &'static [Self] {
        &Self::STANDARD
    }

    pub fn is_wildcard(self) -> bool {
        matches!(self, Self::All)
    }

    pub fn is_standard(self) -> bool {
        !self.is_wildcard()
    }

    pub fn matches(self, request_method: Self) -> bool {
        self == request_method || (self.is_wildcard() && request_method.is_standard())
    }
}

impl fmt::Display for HttpMethod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for HttpMethod {
    type Err = BootError;

    fn from_str(method: &str) -> std::result::Result<Self, Self::Err> {
        match method {
            "GET" => Ok(Self::Get),
            "POST" => Ok(Self::Post),
            "PUT" => Ok(Self::Put),
            "PATCH" => Ok(Self::Patch),
            "DELETE" => Ok(Self::Delete),
            "OPTIONS" => Ok(Self::Options),
            "HEAD" => Ok(Self::Head),
            method => Err(BootError::MethodNotAllowed(method.to_string())),
        }
    }
}
