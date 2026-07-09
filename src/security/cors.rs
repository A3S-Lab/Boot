use super::http_methods::{collect_http_methods, insert_http_method};
use crate::{
    BootError, BootRequest, BootResponse, BoxFuture, ExecutionContext, HttpMethod, Interceptor,
    Middleware, MiddlewareOutcome, Result, RouteHandler,
};
use std::collections::BTreeSet;
use std::str::FromStr;

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
        insert_http_method(&mut self.allowed_methods, method);
        self
    }

    pub fn allow_methods<I>(mut self, methods: I) -> Self
    where
        I: IntoIterator<Item = HttpMethod>,
    {
        self.allowed_methods = collect_http_methods(methods);
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
