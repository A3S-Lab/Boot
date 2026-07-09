use super::http_methods::{collect_http_methods, insert_http_method};
use crate::{BootError, BoxFuture, ExecutionContext, Guard, HttpMethod, Result};
use std::collections::BTreeSet;

/// CSRF guard settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CsrfOptions {
    header_name: String,
    cookie_name: String,
    protected_methods: BTreeSet<HttpMethod>,
}

impl Default for CsrfOptions {
    fn default() -> Self {
        Self {
            header_name: "x-csrf-token".to_string(),
            cookie_name: "csrf-token".to_string(),
            protected_methods: [
                HttpMethod::Post,
                HttpMethod::Put,
                HttpMethod::Patch,
                HttpMethod::Delete,
            ]
            .into_iter()
            .collect(),
        }
    }
}

impl CsrfOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_header_name(mut self, header_name: impl Into<String>) -> Self {
        self.header_name = header_name.into();
        self
    }

    pub fn with_cookie_name(mut self, cookie_name: impl Into<String>) -> Self {
        self.cookie_name = cookie_name.into();
        self
    }

    pub fn protect_method(mut self, method: HttpMethod) -> Self {
        insert_http_method(&mut self.protected_methods, method);
        self
    }

    pub fn protect_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = HttpMethod>,
    {
        self.protected_methods = collect_http_methods(methods);
        self
    }

    pub fn skip_method(mut self, method: HttpMethod) -> Self {
        if method.is_wildcard() {
            self.protected_methods.clear();
        } else {
            self.protected_methods.remove(&method);
        }
        self
    }

    pub fn protects_method(&self, method: HttpMethod) -> bool {
        self.protected_methods.contains(&method)
    }
}

/// Guard that compares a CSRF header token with a cookie token.
#[derive(Debug, Clone, Default)]
pub struct CsrfGuard {
    options: CsrfOptions,
}

impl CsrfGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CsrfOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CsrfOptions {
        &self.options
    }
}

impl Guard for CsrfGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let options = self.options.clone();
        Box::pin(async move {
            if !options.protects_method(context.method) {
                return Ok(true);
            }

            let header_token = context
                .request
                .header(&options.header_name)
                .map(str::trim)
                .filter(|token| !token.is_empty());
            let cookie_token = context
                .request
                .cookie(&options.cookie_name)?
                .map(|token| token.trim().to_string())
                .filter(|token| !token.is_empty());

            match (header_token, cookie_token.as_deref()) {
                (Some(header_token), Some(cookie_token)) if header_token == cookie_token => {
                    Ok(true)
                }
                _ => Err(BootError::Forbidden("invalid CSRF token".to_string())),
            }
        })
    }
}
