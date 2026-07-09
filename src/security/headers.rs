use crate::{BootResponse, BoxFuture, ExecutionContext, Interceptor, Result};
use std::collections::BTreeMap;

/// Security response headers, similar to a small Helmet setup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecurityHeadersOptions {
    content_type_options: Option<String>,
    frame_options: Option<String>,
    referrer_policy: Option<String>,
    cross_origin_opener_policy: Option<String>,
    cross_origin_resource_policy: Option<String>,
    content_security_policy: Option<String>,
    strict_transport_security: Option<String>,
    extra_headers: BTreeMap<String, String>,
}

impl Default for SecurityHeadersOptions {
    fn default() -> Self {
        Self {
            content_type_options: Some("nosniff".to_string()),
            frame_options: Some("DENY".to_string()),
            referrer_policy: Some("no-referrer".to_string()),
            cross_origin_opener_policy: Some("same-origin".to_string()),
            cross_origin_resource_policy: Some("same-origin".to_string()),
            content_security_policy: None,
            strict_transport_security: None,
            extra_headers: BTreeMap::new(),
        }
    }
}

impl SecurityHeadersOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_content_type_options(mut self, value: impl Into<String>) -> Self {
        self.content_type_options = Some(value.into());
        self
    }

    pub fn disable_content_type_options(mut self) -> Self {
        self.content_type_options = None;
        self
    }

    pub fn with_frame_options(mut self, value: impl Into<String>) -> Self {
        self.frame_options = Some(value.into());
        self
    }

    pub fn disable_frame_options(mut self) -> Self {
        self.frame_options = None;
        self
    }

    pub fn with_referrer_policy(mut self, value: impl Into<String>) -> Self {
        self.referrer_policy = Some(value.into());
        self
    }

    pub fn disable_referrer_policy(mut self) -> Self {
        self.referrer_policy = None;
        self
    }

    pub fn with_cross_origin_opener_policy(mut self, value: impl Into<String>) -> Self {
        self.cross_origin_opener_policy = Some(value.into());
        self
    }

    pub fn disable_cross_origin_opener_policy(mut self) -> Self {
        self.cross_origin_opener_policy = None;
        self
    }

    pub fn with_cross_origin_resource_policy(mut self, value: impl Into<String>) -> Self {
        self.cross_origin_resource_policy = Some(value.into());
        self
    }

    pub fn disable_cross_origin_resource_policy(mut self) -> Self {
        self.cross_origin_resource_policy = None;
        self
    }

    pub fn with_content_security_policy(mut self, value: impl Into<String>) -> Self {
        self.content_security_policy = Some(value.into());
        self
    }

    pub fn disable_content_security_policy(mut self) -> Self {
        self.content_security_policy = None;
        self
    }

    pub fn with_strict_transport_security(mut self, value: impl Into<String>) -> Self {
        self.strict_transport_security = Some(value.into());
        self
    }

    pub fn disable_strict_transport_security(mut self) -> Self {
        self.strict_transport_security = None;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.extra_headers.insert(name.into(), value.into());
        self
    }
}

/// Interceptor that adds security response headers when handlers do not set them.
#[derive(Debug, Clone, Default)]
pub struct SecurityHeadersInterceptor {
    options: SecurityHeadersOptions,
}

impl SecurityHeadersInterceptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: SecurityHeadersOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &SecurityHeadersOptions {
        &self.options
    }
}

impl Interceptor for SecurityHeadersInterceptor {
    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let options = self.options.clone();
        Box::pin(async move { Ok(add_security_headers(response, &options)) })
    }
}

fn add_security_headers(
    mut response: BootResponse,
    options: &SecurityHeadersOptions,
) -> BootResponse {
    response = set_header_if_missing(
        response,
        "x-content-type-options",
        &options.content_type_options,
    );
    response = set_header_if_missing(response, "x-frame-options", &options.frame_options);
    response = set_header_if_missing(response, "referrer-policy", &options.referrer_policy);
    response = set_header_if_missing(
        response,
        "cross-origin-opener-policy",
        &options.cross_origin_opener_policy,
    );
    response = set_header_if_missing(
        response,
        "cross-origin-resource-policy",
        &options.cross_origin_resource_policy,
    );
    response = set_header_if_missing(
        response,
        "content-security-policy",
        &options.content_security_policy,
    );
    response = set_header_if_missing(
        response,
        "strict-transport-security",
        &options.strict_transport_security,
    );

    for (name, value) in &options.extra_headers {
        if response.header(name).is_none() {
            response = response.with_header(name, value);
        }
    }

    response
}

fn set_header_if_missing(
    response: BootResponse,
    name: &str,
    value: &Option<String>,
) -> BootResponse {
    match value {
        Some(value) if response.header(name).is_none() => response.with_header(name, value),
        _ => response,
    }
}
