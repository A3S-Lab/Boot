use std::time::Duration;

/// SameSite attribute used for the session cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCookieSameSite {
    Strict,
    Lax,
    None,
}

impl SessionCookieSameSite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

/// Session settings shared by the manager, middleware, and cookie interceptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    cookie_name: String,
    request_header_name: String,
    ttl: Option<Duration>,
    cookie_path: String,
    cookie_domain: Option<String>,
    http_only: bool,
    secure: bool,
    same_site: Option<SessionCookieSameSite>,
    rolling: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            cookie_name: "a3s.sid".to_string(),
            request_header_name: "x-a3s-session-id".to_string(),
            ttl: Some(Duration::from_secs(60 * 60 * 24)),
            cookie_path: "/".to_string(),
            cookie_domain: None,
            http_only: true,
            secure: false,
            same_site: Some(SessionCookieSameSite::Lax),
            rolling: false,
        }
    }
}

impl SessionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cookie_name(mut self, cookie_name: impl Into<String>) -> Self {
        self.cookie_name = cookie_name.into();
        self
    }

    pub fn with_request_header_name(mut self, header_name: impl Into<String>) -> Self {
        self.request_header_name = header_name.into();
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn without_ttl(mut self) -> Self {
        self.ttl = None;
        self
    }

    pub fn with_cookie_path(mut self, path: impl Into<String>) -> Self {
        self.cookie_path = path.into();
        self
    }

    pub fn with_cookie_domain(mut self, domain: impl Into<String>) -> Self {
        self.cookie_domain = Some(domain.into());
        self
    }

    pub fn without_cookie_domain(mut self) -> Self {
        self.cookie_domain = None;
        self
    }

    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    pub fn with_same_site(mut self, same_site: SessionCookieSameSite) -> Self {
        self.same_site = Some(same_site);
        self
    }

    pub fn without_same_site(mut self) -> Self {
        self.same_site = None;
        self
    }

    pub fn rolling(mut self, rolling: bool) -> Self {
        self.rolling = rolling;
        self
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub fn request_header_name(&self) -> &str {
        &self.request_header_name
    }

    pub fn ttl(&self) -> Option<Duration> {
        self.ttl
    }

    pub fn cookie_path(&self) -> &str {
        &self.cookie_path
    }

    pub fn cookie_domain(&self) -> Option<&str> {
        self.cookie_domain.as_deref()
    }

    pub fn is_http_only(&self) -> bool {
        self.http_only
    }

    pub fn is_secure(&self) -> bool {
        self.secure
    }

    pub fn same_site(&self) -> Option<SessionCookieSameSite> {
        self.same_site
    }

    pub fn is_rolling(&self) -> bool {
        self.rolling
    }
}
