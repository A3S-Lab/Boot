use crate::{
    BootError, BoxFuture, HttpMethod, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

/// Settings shared by [`HttpService`] requests.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HttpClientOptions {
    base_url: Option<String>,
    headers: BTreeMap<String, String>,
    timeout: Option<Duration>,
}

impl HttpClientOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_base_url(mut self, base_url: impl Into<String>) -> Self {
        self.base_url = Some(base_url.into());
        self
    }

    pub fn without_base_url(mut self) -> Self {
        self.base_url = None;
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(name), value.into());
        self
    }

    pub fn with_headers<I, K, V>(mut self, headers: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        for (name, value) in headers {
            self = self.with_header(name, value);
        }
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn without_timeout(mut self) -> Self {
        self.timeout = None;
        self
    }

    pub fn base_url(&self) -> Option<&str> {
        self.base_url.as_deref()
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&normalize_header_name(name))
            .map(String::as_str)
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }
}

/// Outbound HTTP request sent by [`HttpService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpClientRequest {
    method: HttpMethod,
    url: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
    timeout: Option<Duration>,
}

impl HttpClientRequest {
    pub fn new(method: HttpMethod, url: impl Into<String>) -> Self {
        Self {
            method,
            url: url.into(),
            headers: BTreeMap::new(),
            body: Vec::new(),
            timeout: None,
        }
    }

    pub fn get(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Get, url)
    }

    pub fn post(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Post, url)
    }

    pub fn put(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Put, url)
    }

    pub fn patch(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Patch, url)
    }

    pub fn delete(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Delete, url)
    }

    pub fn head(url: impl Into<String>) -> Self {
        Self::new(HttpMethod::Head, url)
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn url(&self) -> &str {
        &self.url
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&normalize_header_name(name))
            .map(String::as_str)
    }

    pub fn headers(&self) -> &BTreeMap<String, String> {
        &self.headers
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn timeout(&self) -> Option<Duration> {
        self.timeout
    }

    pub fn with_url(mut self, url: impl Into<String>) -> Self {
        self.url = url.into();
        self
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(name), value.into());
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn with_text(self, body: impl Into<String>) -> Self {
        self.with_body(body.into())
            .with_header("content-type", "text/plain; charset=utf-8")
    }

    pub fn with_json<T>(self, body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let body = serde_json::to_vec(body).map_err(|error| {
            BootError::Internal(format!("failed to serialize HTTP request body: {error}"))
        })?;
        Ok(self
            .with_body(body)
            .with_header("content-type", "application/json"))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    fn apply_options(mut self, options: &HttpClientOptions) -> Result<Self> {
        self.url = resolve_url(options.base_url(), &self.url)?;
        for (name, value) in &options.headers {
            self.headers
                .entry(name.clone())
                .or_insert_with(|| value.clone());
        }
        if self.timeout.is_none() {
            self.timeout = options.timeout;
        }
        Ok(self)
    }
}

/// Outbound HTTP response returned by [`HttpService`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpClientResponse {
    status: u16,
    headers: BTreeMap<String, Vec<String>>,
    body: Vec<u8>,
}

impl HttpClientResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            body: body.into(),
        }
    }

    pub fn json<T>(body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Self::json_with_status(200, body)
    }

    pub fn json_with_status<T>(status: u16, body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let body = serde_json::to_vec(body).map_err(|error| {
            BootError::Internal(format!("failed to serialize HTTP response body: {error}"))
        })?;
        Ok(Self::new(status, body).with_header("content-type", "application/json"))
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn headers(&self) -> &BTreeMap<String, Vec<String>> {
        &self.headers
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        self.header_values(name).into_iter().next()
    }

    pub fn header_values(&self, name: &str) -> Vec<&str> {
        self.headers
            .get(&normalize_header_name(name))
            .map(|values| values.iter().map(String::as_str).collect())
            .unwrap_or_default()
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    pub fn body_text(&self) -> Result<String> {
        String::from_utf8(self.body.clone()).map_err(|error| {
            BootError::Internal(format!("invalid HTTP response text body: {error}"))
        })
    }

    pub fn body_json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|error| {
            BootError::Internal(format!("invalid HTTP response JSON body: {error}"))
        })
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn error_for_status(self) -> Result<Self> {
        if self.is_success() {
            return Ok(self);
        }

        Err(BootError::BadRequest(format!(
            "HTTP request returned status {}",
            self.status
        )))
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .entry(normalize_header_name(name))
            .or_default()
            .push(value.into());
        self
    }
}

/// Backend used by [`HttpService`] to execute outbound requests.
pub trait HttpClientBackend: Send + Sync + 'static {
    fn send(&self, request: HttpClientRequest) -> BoxFuture<'static, Result<HttpClientResponse>>;
}

/// Reqwest-backed outbound HTTP backend.
#[derive(Debug, Clone, Default)]
pub struct ReqwestHttpClientBackend {
    client: reqwest::Client,
}

impl ReqwestHttpClientBackend {
    pub fn new() -> Self {
        Self::default()
    }
}

impl HttpClientBackend for ReqwestHttpClientBackend {
    fn send(&self, request: HttpClientRequest) -> BoxFuture<'static, Result<HttpClientResponse>> {
        let client = self.client.clone();
        Box::pin(async move {
            let method = reqwest_method(request.method)?;
            let mut builder = client.request(method, request.url);
            for (name, value) in request.headers {
                builder = builder.header(name, value);
            }
            if !request.body.is_empty() {
                builder = builder.body(request.body);
            }
            if let Some(timeout) = request.timeout {
                builder = builder.timeout(timeout);
            }

            let response = builder
                .send()
                .await
                .map_err(|error| BootError::Internal(format!("HTTP request failed: {error}")))?;
            let status = response.status().as_u16();
            let headers = response_headers(response.headers())?;
            let body = response
                .bytes()
                .await
                .map_err(|error| BootError::Internal(format!("HTTP response failed: {error}")))?
                .to_vec();

            Ok(HttpClientResponse {
                status,
                headers,
                body,
            })
        })
    }
}

/// Injectable outbound HTTP client, comparable to Nest's `HttpService`.
#[derive(Clone)]
pub struct HttpService {
    backend: Arc<dyn HttpClientBackend>,
    options: HttpClientOptions,
}

impl fmt::Debug for HttpService {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpService")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Default for HttpService {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpService {
    pub fn new() -> Self {
        Self::with_options(HttpClientOptions::default())
    }

    pub fn with_options(options: HttpClientOptions) -> Self {
        Self::from_backend(ReqwestHttpClientBackend::new(), options)
    }

    pub fn from_backend<B>(backend: B, options: HttpClientOptions) -> Self
    where
        B: HttpClientBackend,
    {
        Self {
            backend: Arc::new(backend),
            options,
        }
    }

    pub fn from_backend_arc(
        backend: Arc<dyn HttpClientBackend>,
        options: HttpClientOptions,
    ) -> Self {
        Self { backend, options }
    }

    pub fn options(&self) -> &HttpClientOptions {
        &self.options
    }

    pub async fn request(&self, request: HttpClientRequest) -> Result<HttpClientResponse> {
        self.backend
            .send(request.apply_options(&self.options)?)
            .await
    }

    pub async fn get(&self, url: impl Into<String>) -> Result<HttpClientResponse> {
        self.request(HttpClientRequest::get(url)).await
    }

    pub async fn head(&self, url: impl Into<String>) -> Result<HttpClientResponse> {
        self.request(HttpClientRequest::head(url)).await
    }

    pub async fn delete(&self, url: impl Into<String>) -> Result<HttpClientResponse> {
        self.request(HttpClientRequest::delete(url)).await
    }

    pub async fn post<T>(&self, url: impl Into<String>, body: &T) -> Result<HttpClientResponse>
    where
        T: Serialize,
    {
        self.request(HttpClientRequest::post(url).with_json(body)?)
            .await
    }

    pub async fn put<T>(&self, url: impl Into<String>, body: &T) -> Result<HttpClientResponse>
    where
        T: Serialize,
    {
        self.request(HttpClientRequest::put(url).with_json(body)?)
            .await
    }

    pub async fn patch<T>(&self, url: impl Into<String>, body: &T) -> Result<HttpClientResponse>
    where
        T: Serialize,
    {
        self.request(HttpClientRequest::patch(url).with_json(body)?)
            .await
    }

    pub async fn get_json<T>(&self, url: impl Into<String>) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.get(url).await?.body_json()
    }

    pub async fn post_json<T, R>(&self, url: impl Into<String>, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        self.post(url, body).await?.body_json()
    }

    pub async fn put_json<T, R>(&self, url: impl Into<String>, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        self.put(url, body).await?.body_json()
    }

    pub async fn patch_json<T, R>(&self, url: impl Into<String>, body: &T) -> Result<R>
    where
        T: Serialize,
        R: DeserializeOwned,
    {
        self.patch(url, body).await?.body_json()
    }
}

/// Module that registers and exports an [`HttpService`] provider.
#[derive(Clone)]
pub struct HttpModule {
    name: &'static str,
    token: ProviderToken,
    provider: HttpModuleProvider,
    global: bool,
}

type AsyncHttpOptionsFactory =
    dyn Fn(ModuleRef) -> BoxFuture<'static, Result<HttpClientOptions>> + Send + Sync;

#[derive(Clone)]
enum HttpModuleProvider {
    Service(Arc<HttpService>),
    AsyncOptions(Arc<AsyncHttpOptionsFactory>),
}

impl fmt::Debug for HttpModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HttpModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl HttpModule {
    pub fn new(name: &'static str) -> Self {
        Self::with_options(name, HttpClientOptions::default())
    }

    pub fn with_options(name: &'static str, options: HttpClientOptions) -> Self {
        Self::from_service(name, HttpService::with_options(options))
    }

    pub fn with_backend<B>(name: &'static str, backend: B) -> Self
    where
        B: HttpClientBackend,
    {
        Self::with_backend_and_options(name, backend, HttpClientOptions::default())
    }

    pub fn with_backend_and_options<B>(
        name: &'static str,
        backend: B,
        options: HttpClientOptions,
    ) -> Self
    where
        B: HttpClientBackend,
    {
        Self::from_service(name, HttpService::from_backend(backend, options))
    }

    pub fn from_service(name: &'static str, service: HttpService) -> Self {
        let token = ProviderToken::of::<HttpService>();
        Self {
            name,
            provider: HttpModuleProvider::Service(Arc::new(service)),
            token,
            global: false,
        }
    }

    pub fn async_options<F, Fut>(name: &'static str, factory: F) -> Self
    where
        F: Fn(ModuleRef) -> Fut + Send + Sync + 'static,
        Fut: std::future::Future<Output = Result<HttpClientOptions>> + Send + 'static,
    {
        let token = ProviderToken::of::<HttpService>();
        let factory = Arc::new(move |module_ref: ModuleRef| {
            let future = factory(module_ref);
            Box::pin(future) as BoxFuture<'static, Result<HttpClientOptions>>
        });
        Self {
            name,
            provider: HttpModuleProvider::AsyncOptions(factory),
            token,
            global: false,
        }
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for HttpModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let provider = match &self.provider {
            HttpModuleProvider::Service(service) => {
                ProviderDefinition::named_from_arc(self.token.as_str(), Arc::clone(service))
            }
            HttpModuleProvider::AsyncOptions(factory) => {
                let factory = Arc::clone(factory);
                ProviderDefinition::named_async_factory(self.token.as_str(), move |module_ref| {
                    let factory = Arc::clone(&factory);
                    async move { Ok(HttpService::with_options(factory(module_ref).await?)) }
                })
            }
        };
        Ok(vec![provider])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}

fn resolve_url(base_url: Option<&str>, url: &str) -> Result<String> {
    if is_absolute_http_url(url) {
        return Ok(url.to_string());
    }

    let Some(base_url) = base_url else {
        return Err(BootError::BadRequest(format!(
            "relative HTTP client URL `{url}` requires a base URL"
        )));
    };

    let base = base_url.trim_end_matches('/');
    let path = url.trim_start_matches('/');
    if path.is_empty() {
        Ok(base.to_string())
    } else {
        Ok(format!("{base}/{path}"))
    }
}

fn is_absolute_http_url(url: &str) -> bool {
    url.starts_with("http://") || url.starts_with("https://")
}

fn normalize_header_name(name: impl Into<String>) -> String {
    name.into().trim().to_ascii_lowercase()
}

fn reqwest_method(method: HttpMethod) -> Result<reqwest::Method> {
    match method {
        HttpMethod::Get => Ok(reqwest::Method::GET),
        HttpMethod::Post => Ok(reqwest::Method::POST),
        HttpMethod::Put => Ok(reqwest::Method::PUT),
        HttpMethod::Patch => Ok(reqwest::Method::PATCH),
        HttpMethod::Delete => Ok(reqwest::Method::DELETE),
        HttpMethod::Head => Ok(reqwest::Method::HEAD),
        HttpMethod::Options => Ok(reqwest::Method::OPTIONS),
        HttpMethod::All => Err(BootError::BadRequest(
            "HTTP client requests must use a concrete method".to_string(),
        )),
    }
}

fn response_headers(headers: &reqwest::header::HeaderMap) -> Result<BTreeMap<String, Vec<String>>> {
    let mut values = BTreeMap::<String, Vec<String>>::new();
    for (name, value) in headers {
        let value = value.to_str().map_err(|error| {
            BootError::Internal(format!(
                "HTTP response header `{}` is not valid text: {error}",
                name.as_str()
            ))
        })?;
        values
            .entry(name.as_str().to_ascii_lowercase())
            .or_default()
            .push(value.to_string());
    }
    Ok(values)
}
