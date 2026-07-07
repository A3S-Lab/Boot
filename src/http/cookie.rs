use crate::{BootError, Result};
use std::time::Duration;

/// SameSite attribute used for response cookies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CookieSameSite {
    Strict,
    Lax,
    None,
}

impl CookieSameSite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

/// Options used by response cookie helpers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CookieOptions {
    path: Option<String>,
    domain: Option<String>,
    max_age: Option<u64>,
    http_only: bool,
    secure: bool,
    same_site: Option<CookieSameSite>,
}

impl Default for CookieOptions {
    fn default() -> Self {
        Self {
            path: Some("/".to_string()),
            domain: None,
            max_age: None,
            http_only: false,
            secure: false,
            same_site: None,
        }
    }
}

impl CookieOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_path(mut self, path: impl Into<String>) -> Self {
        self.path = Some(path.into());
        self
    }

    pub fn without_path(mut self) -> Self {
        self.path = None;
        self
    }

    pub fn with_domain(mut self, domain: impl Into<String>) -> Self {
        self.domain = Some(domain.into());
        self
    }

    pub fn without_domain(mut self) -> Self {
        self.domain = None;
        self
    }

    pub fn with_max_age(mut self, max_age: Duration) -> Self {
        self.max_age = Some(max_age.as_secs());
        self
    }

    pub fn with_max_age_seconds(mut self, max_age: u64) -> Self {
        self.max_age = Some(max_age);
        self
    }

    pub fn without_max_age(mut self) -> Self {
        self.max_age = None;
        self
    }

    pub fn with_http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn with_secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    pub fn with_same_site(mut self, same_site: CookieSameSite) -> Self {
        self.same_site = Some(same_site);
        self
    }

    pub fn without_same_site(mut self) -> Self {
        self.same_site = None;
        self
    }

    pub fn path(&self) -> Option<&str> {
        self.path.as_deref()
    }

    pub fn domain(&self) -> Option<&str> {
        self.domain.as_deref()
    }

    pub fn max_age_seconds(&self) -> Option<u64> {
        self.max_age
    }

    pub fn http_only(&self) -> bool {
        self.http_only
    }

    pub fn secure(&self) -> bool {
        self.secure
    }

    pub fn same_site(&self) -> Option<CookieSameSite> {
        self.same_site
    }

    pub(crate) fn set_cookie_header(&self, name: &str, value: &str) -> Result<String> {
        validate_cookie_name(name)?;
        validate_cookie_value(value)?;
        self.cookie_header(name, value, self.max_age)
    }

    pub(crate) fn delete_cookie_header(&self, name: &str) -> Result<String> {
        validate_cookie_name(name)?;
        self.cookie_header(name, "", Some(0))
    }

    fn cookie_header(&self, name: &str, value: &str, max_age: Option<u64>) -> Result<String> {
        let mut cookie = format!("{name}={value}");
        if let Some(path) = &self.path {
            validate_cookie_attribute("cookie path", path)?;
            cookie.push_str("; Path=");
            cookie.push_str(path);
        }
        if let Some(domain) = &self.domain {
            validate_cookie_attribute("cookie domain", domain)?;
            cookie.push_str("; Domain=");
            cookie.push_str(domain);
        }
        if let Some(max_age) = max_age {
            cookie.push_str(&format!("; Max-Age={max_age}"));
        }
        if self.http_only {
            cookie.push_str("; HttpOnly");
        }
        if self.secure {
            cookie.push_str("; Secure");
        }
        if let Some(same_site) = self.same_site {
            cookie.push_str("; SameSite=");
            cookie.push_str(same_site.as_str());
        }
        Ok(cookie)
    }
}

fn validate_cookie_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(BootError::Internal(
            "cookie name cannot be empty".to_string(),
        ));
    }

    if name.bytes().all(is_cookie_name_byte) {
        Ok(())
    } else {
        Err(BootError::Internal(format!(
            "invalid cookie name {name:?}: cookie name contains invalid characters"
        )))
    }
}

fn validate_cookie_value(value: &str) -> Result<()> {
    if value.bytes().all(is_cookie_value_byte) {
        Ok(())
    } else {
        Err(BootError::Internal(
            "cookie value contains invalid characters".to_string(),
        ))
    }
}

fn validate_cookie_attribute(name: &str, value: &str) -> Result<()> {
    if value.is_empty() {
        return Err(BootError::Internal(format!("{name} cannot be empty")));
    }

    if value
        .bytes()
        .all(|byte| matches!(byte, 0x20..=0x7e) && byte != b';')
    {
        Ok(())
    } else {
        Err(BootError::Internal(format!(
            "{name} contains invalid characters"
        )))
    }
}

fn is_cookie_name_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'a'..=b'z'
            | b'A'..=b'Z'
    )
}

fn is_cookie_value_byte(byte: u8) -> bool {
    matches!(byte, 0x21 | 0x23..=0x2b | 0x2d..=0x3a | 0x3c..=0x5b | 0x5d..=0x7e)
}
