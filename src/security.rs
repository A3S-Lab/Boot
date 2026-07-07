use crate::{
    BootError, BootRequest, BootResponse, BoxFuture, ExecutionContext, Guard, HttpMethod,
    Interceptor, Middleware, MiddlewareOutcome, Result, RouteHandler,
};
use std::collections::{BTreeMap, BTreeSet};
use std::str::FromStr;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// CORS settings used by [`CorsMiddleware`] and [`CorsResponseInterceptor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CorsOptions {
    allowed_origins: CorsOriginPolicy,
    allowed_methods: BTreeSet<HttpMethod>,
    allowed_headers: CorsHeaderPolicy,
    exposed_headers: BTreeSet<String>,
    allow_credentials: bool,
    max_age: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CorsOriginPolicy {
    Any,
    Exact(BTreeSet<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum CorsHeaderPolicy {
    AnyRequested,
    Exact(BTreeSet<String>),
}

impl Default for CorsOptions {
    fn default() -> Self {
        Self {
            allowed_origins: CorsOriginPolicy::Any,
            allowed_methods: [
                HttpMethod::Get,
                HttpMethod::Head,
                HttpMethod::Post,
                HttpMethod::Put,
                HttpMethod::Patch,
                HttpMethod::Delete,
                HttpMethod::Options,
            ]
            .into_iter()
            .collect(),
            allowed_headers: CorsHeaderPolicy::AnyRequested,
            exposed_headers: BTreeSet::new(),
            allow_credentials: false,
            max_age: None,
        }
    }
}

impl CorsOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn allow_any_origin(mut self) -> Self {
        self.allowed_origins = CorsOriginPolicy::Any;
        self
    }

    pub fn allow_origin(mut self, origin: impl Into<String>) -> Self {
        match &mut self.allowed_origins {
            CorsOriginPolicy::Any => {
                self.allowed_origins = CorsOriginPolicy::Exact(BTreeSet::from([origin.into()]));
            }
            CorsOriginPolicy::Exact(origins) => {
                origins.insert(origin.into());
            }
        }
        self
    }

    pub fn allow_origins<I, S>(mut self, origins: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_origins =
            CorsOriginPolicy::Exact(origins.into_iter().map(Into::into).collect());
        self
    }

    pub fn allow_method(mut self, method: HttpMethod) -> Self {
        self.allowed_methods.insert(method);
        self
    }

    pub fn allow_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = HttpMethod>,
    {
        self.allowed_methods = methods.into_iter().collect();
        self
    }

    pub fn allow_any_header(mut self) -> Self {
        self.allowed_headers = CorsHeaderPolicy::AnyRequested;
        self
    }

    pub fn allow_header(mut self, header: impl Into<String>) -> Self {
        let header = normalize_cors_header_name(header.into());
        match &mut self.allowed_headers {
            CorsHeaderPolicy::AnyRequested => {
                self.allowed_headers = CorsHeaderPolicy::Exact(BTreeSet::from([header]));
            }
            CorsHeaderPolicy::Exact(headers) => {
                headers.insert(header);
            }
        }
        self
    }

    pub fn allow_headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.allowed_headers = CorsHeaderPolicy::Exact(
            headers
                .into_iter()
                .map(Into::into)
                .map(normalize_cors_header_name)
                .collect(),
        );
        self
    }

    pub fn expose_header(mut self, header: impl Into<String>) -> Self {
        self.exposed_headers
            .insert(normalize_cors_header_name(header.into()));
        self
    }

    pub fn expose_headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for header in headers {
            self = self.expose_header(header);
        }
        self
    }

    pub fn allow_credentials(mut self) -> Self {
        self.allow_credentials = true;
        self
    }

    pub fn with_max_age(mut self, seconds: u64) -> Self {
        self.max_age = Some(seconds);
        self
    }

    pub fn allowed_methods(&self) -> impl Iterator<Item = HttpMethod> + '_ {
        self.allowed_methods.iter().copied()
    }

    pub fn allows_credentials(&self) -> bool {
        self.allow_credentials
    }

    pub fn max_age(&self) -> Option<u64> {
        self.max_age
    }
}

/// Middleware that short-circuits CORS preflight requests.
#[derive(Debug, Clone, Default)]
pub struct CorsMiddleware {
    options: CorsOptions,
}

impl CorsMiddleware {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CorsOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CorsOptions {
        &self.options
    }

    pub fn preflight_response(&self, request: &BootRequest) -> Result<Option<BootResponse>> {
        cors_preflight_response(request, &self.options)
    }
}

impl Middleware for CorsMiddleware {
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        let options = self.options.clone();
        Box::pin(async move {
            match cors_preflight_response(&request, &options)? {
                Some(response) => Ok(MiddlewareOutcome::response(response)),
                None => Ok(MiddlewareOutcome::next(request)),
            }
        })
    }
}

/// Handler used by the application builder for generated CORS preflight routes.
#[derive(Debug, Clone, Default)]
pub struct CorsPreflightRoute {
    middleware: CorsMiddleware,
}

impl CorsPreflightRoute {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CorsOptions) -> Self {
        Self {
            middleware: CorsMiddleware::with_options(options),
        }
    }

    pub fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>> {
        let middleware = self.middleware.clone();
        Box::pin(async move {
            if let Some(response) = middleware.preflight_response(&request)? {
                return Ok(response);
            }

            Err(BootError::MethodNotAllowed(format!(
                "{} {}",
                request.method().as_str(),
                request.path()
            )))
        })
    }
}

impl RouteHandler for CorsPreflightRoute {
    fn call(&self, request: BootRequest) -> BoxFuture<'static, Result<BootResponse>> {
        self.handle(request)
    }
}

/// Interceptor that adds CORS headers to normal responses.
#[derive(Debug, Clone, Default)]
pub struct CorsResponseInterceptor {
    options: CorsOptions,
}

impl CorsResponseInterceptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CorsOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CorsOptions {
        &self.options
    }
}

impl Interceptor for CorsResponseInterceptor {
    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let options = self.options.clone();
        Box::pin(async move {
            Ok(add_actual_cors_headers(
                response,
                &context.request,
                &options,
            ))
        })
    }
}

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
        self.protected_methods.insert(method);
        self
    }

    pub fn protect_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = HttpMethod>,
    {
        self.protected_methods = methods.into_iter().collect();
        self
    }

    pub fn skip_method(mut self, method: HttpMethod) -> Self {
        self.protected_methods.remove(&method);
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

/// In-memory rate limit settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitOptions {
    max_requests: u32,
    window: Duration,
    key_headers: Vec<String>,
    use_bearer_token: bool,
    anonymous_key: String,
}

impl Default for RateLimitOptions {
    fn default() -> Self {
        Self {
            max_requests: 60,
            window: Duration::from_secs(60),
            key_headers: vec!["x-forwarded-for".to_string(), "x-real-ip".to_string()],
            use_bearer_token: true,
            anonymous_key: "anonymous".to_string(),
        }
    }
}

impl RateLimitOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_requests(mut self, max_requests: u32) -> Self {
        self.max_requests = max_requests;
        self
    }

    pub fn with_window(mut self, window: Duration) -> Self {
        self.window = window;
        self
    }

    pub fn with_key_header(mut self, header: impl Into<String>) -> Self {
        self.key_headers = vec![header.into()];
        self
    }

    pub fn with_key_headers<I, S>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        self.key_headers = headers.into_iter().map(Into::into).collect();
        self
    }

    pub fn without_bearer_token(mut self) -> Self {
        self.use_bearer_token = false;
        self
    }

    pub fn with_anonymous_key(mut self, key: impl Into<String>) -> Self {
        self.anonymous_key = key.into();
        self
    }

    pub fn max_requests(&self) -> u32 {
        self.max_requests
    }

    pub fn window(&self) -> Duration {
        self.window
    }
}

/// Guard that enforces an in-memory fixed-window rate limit.
#[derive(Debug, Clone)]
pub struct RateLimitGuard {
    options: RateLimitOptions,
    buckets: Arc<Mutex<BTreeMap<String, RateLimitBucket>>>,
}

#[derive(Debug, Clone)]
struct RateLimitBucket {
    window_started_at: Instant,
    count: u32,
}

impl Default for RateLimitGuard {
    fn default() -> Self {
        Self {
            options: RateLimitOptions::default(),
            buckets: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }
}

impl RateLimitGuard {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: RateLimitOptions) -> Self {
        Self {
            options,
            buckets: Arc::new(Mutex::new(BTreeMap::new())),
        }
    }

    pub fn options(&self) -> &RateLimitOptions {
        &self.options
    }
}

impl Guard for RateLimitGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let options = self.options.clone();
        let buckets = Arc::clone(&self.buckets);
        Box::pin(async move {
            let key = rate_limit_key(&context.request, &options);
            let now = Instant::now();
            let mut buckets = buckets.lock().map_err(|_| {
                BootError::Internal("rate limit state lock is poisoned".to_string())
            })?;
            buckets
                .retain(|_, bucket| now.duration_since(bucket.window_started_at) < options.window);

            let bucket = buckets.entry(key).or_insert_with(|| RateLimitBucket {
                window_started_at: now,
                count: 0,
            });

            if now.duration_since(bucket.window_started_at) >= options.window {
                bucket.window_started_at = now;
                bucket.count = 0;
            }

            if bucket.count >= options.max_requests {
                return Err(BootError::TooManyRequests(
                    "rate limit exceeded".to_string(),
                ));
            }

            bucket.count += 1;
            Ok(true)
        })
    }
}

fn cors_preflight_response(
    request: &BootRequest,
    options: &CorsOptions,
) -> Result<Option<BootResponse>> {
    if request.method() != HttpMethod::Options
        || request.header("origin").is_none()
        || request.header("access-control-request-method").is_none()
    {
        return Ok(None);
    }

    let requested_method = requested_preflight_method(request)?;
    if !options.allowed_methods.contains(&requested_method) {
        return Err(BootError::Forbidden(
            "CORS request method is not allowed".to_string(),
        ));
    }

    let Some(allowed_origin) = allowed_origin_header(request, options) else {
        return Err(BootError::Forbidden(
            "CORS origin is not allowed".to_string(),
        ));
    };
    let allowed_headers = allowed_request_headers(request, options)?;

    let mut response = BootResponse::no_content()
        .with_header("access-control-allow-origin", allowed_origin)
        .with_header(
            "access-control-allow-methods",
            cors_methods_header(&options.allowed_methods),
        );

    if options.allow_credentials {
        response = response.with_header("access-control-allow-credentials", "true");
    }
    if let Some(max_age) = options.max_age {
        response = response.with_header("access-control-max-age", max_age.to_string());
    }
    if let Some(headers) = allowed_headers {
        response = response.with_header("access-control-allow-headers", headers);
    }

    Ok(Some(ensure_vary_headers(
        response,
        &[
            "origin",
            "access-control-request-method",
            "access-control-request-headers",
        ],
    )))
}

fn add_actual_cors_headers(
    mut response: BootResponse,
    request: &BootRequest,
    options: &CorsOptions,
) -> BootResponse {
    let Some(allowed_origin) = allowed_origin_header(request, options) else {
        return response;
    };

    let varies_by_origin = allowed_origin != "*";
    response = response.with_header("access-control-allow-origin", allowed_origin);
    if options.allow_credentials {
        response = response.with_header("access-control-allow-credentials", "true");
    }
    if !options.exposed_headers.is_empty() {
        response = response.with_header(
            "access-control-expose-headers",
            join_sorted_strings(&options.exposed_headers),
        );
    }

    if varies_by_origin {
        response = ensure_vary_headers(response, &["origin"]);
    }

    response
}

fn allowed_origin_header(request: &BootRequest, options: &CorsOptions) -> Option<String> {
    let origin = request.header("origin")?;
    match &options.allowed_origins {
        CorsOriginPolicy::Any if options.allow_credentials => Some(origin.to_string()),
        CorsOriginPolicy::Any => Some("*".to_string()),
        CorsOriginPolicy::Exact(origins) if origins.contains(origin) => Some(origin.to_string()),
        CorsOriginPolicy::Exact(_) => None,
    }
}

fn requested_preflight_method(request: &BootRequest) -> Result<HttpMethod> {
    let method = request
        .header("access-control-request-method")
        .unwrap_or_default()
        .trim()
        .to_ascii_uppercase();
    HttpMethod::from_str(&method)
        .map_err(|_| BootError::BadRequest(format!("invalid CORS request method: {method}")))
}

fn allowed_request_headers(request: &BootRequest, options: &CorsOptions) -> Result<Option<String>> {
    let requested = request
        .header("access-control-request-headers")
        .map(parse_cors_header_list)
        .unwrap_or_default();

    match &options.allowed_headers {
        CorsHeaderPolicy::AnyRequested if requested.is_empty() => Ok(None),
        CorsHeaderPolicy::AnyRequested => Ok(Some(requested.join(","))),
        CorsHeaderPolicy::Exact(headers) => {
            for header in &requested {
                if !headers.contains(header) {
                    return Err(BootError::Forbidden(format!(
                        "CORS request header is not allowed: {header}"
                    )));
                }
            }

            if headers.is_empty() {
                Ok(None)
            } else {
                Ok(Some(join_sorted_strings(headers)))
            }
        }
    }
}

fn cors_methods_header(methods: &BTreeSet<HttpMethod>) -> String {
    let order = [
        HttpMethod::Get,
        HttpMethod::Head,
        HttpMethod::Post,
        HttpMethod::Put,
        HttpMethod::Patch,
        HttpMethod::Delete,
        HttpMethod::Options,
    ];
    order
        .into_iter()
        .filter(|method| methods.contains(method))
        .map(HttpMethod::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn parse_cors_header_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|header| !header.is_empty())
        .map(str::to_ascii_lowercase)
        .collect()
}

fn normalize_cors_header_name(header: String) -> String {
    header.trim().to_ascii_lowercase()
}

fn join_sorted_strings(values: &BTreeSet<String>) -> String {
    values
        .iter()
        .map(String::as_str)
        .collect::<Vec<_>>()
        .join(",")
}

fn ensure_vary_headers(mut response: BootResponse, names: &[&str]) -> BootResponse {
    for name in names {
        response = ensure_vary_header(response, name);
    }
    response
}

fn ensure_vary_header(response: BootResponse, name: &str) -> BootResponse {
    let Some(vary) = response.header("vary").map(str::to_string) else {
        return response.with_header("vary", name);
    };
    let has_name = vary
        .split(',')
        .map(str::trim)
        .any(|value| value == "*" || value.eq_ignore_ascii_case(name));

    if has_name {
        response
    } else {
        response.with_header("vary", format!("{vary}, {name}"))
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

fn rate_limit_key(request: &BootRequest, options: &RateLimitOptions) -> String {
    for header in &options.key_headers {
        if let Some(value) = request
            .header(header)
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return format!("header:{header}:{value}");
        }
    }

    if options.use_bearer_token {
        if let Some(token) = request.bearer_token() {
            return format!("bearer:{token}");
        }
    }

    options.anonymous_key.clone()
}
