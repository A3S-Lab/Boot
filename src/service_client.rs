//! Resilient, discovery-aware outbound service client.
//!
//! Composes [`HttpService`] with discovery, load balancing, circuit breaking,
//! bulkhead admission, retry, and degrade/fallback policies.

use crate::resilience::{
    Bulkhead, BulkheadOptions, CircuitBreaker, CircuitBreakerOptions, DegradePolicy,
    FallbackResponse, RetryOptions, RetryPolicy,
};
use crate::service_discovery::{ServiceDiscovery, ServiceLoadBalancer};
use crate::{
    BootError, HttpClientBackend, HttpClientOptions, HttpClientRequest, HttpClientResponse,
    HttpMethod, HttpService, Module, ModuleRef, ProviderDefinition, ProviderToken, Result,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt;
use std::sync::Arc;
use std::time::Duration;

fn degraded_response(fallback: &FallbackResponse) -> HttpClientResponse {
    HttpClientResponse::new(fallback.status, fallback.body.clone())
        .with_header("content-type", &fallback.content_type)
        .with_header("x-a3s-degraded", "1")
}

/// Policy bundle for [`ServiceClient`] calls.
#[derive(Debug, Clone)]
pub struct ServiceClientOptions {
    pub circuit_breaker: CircuitBreakerOptions,
    pub retry: RetryOptions,
    pub bulkhead: BulkheadOptions,
    pub degrade: Option<DegradePolicy>,
    pub request_timeout: Option<Duration>,
}

impl Default for ServiceClientOptions {
    fn default() -> Self {
        Self {
            circuit_breaker: CircuitBreakerOptions::default(),
            retry: RetryOptions::default(),
            bulkhead: BulkheadOptions::default(),
            degrade: Some(DegradePolicy::new(FallbackResponse::json(
                503,
                br#"{"error":"service degraded"}"#.to_vec(),
            ))),
            request_timeout: Some(Duration::from_secs(5)),
        }
    }
}

impl ServiceClientOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn without_degrade(mut self) -> Self {
        self.degrade = None;
        self
    }

    pub fn with_degrade(mut self, degrade: DegradePolicy) -> Self {
        self.degrade = Some(degrade);
        self
    }

    pub fn with_retry(mut self, retry: RetryOptions) -> Self {
        self.retry = retry;
        self
    }

    pub fn with_circuit_breaker(mut self, circuit_breaker: CircuitBreakerOptions) -> Self {
        self.circuit_breaker = circuit_breaker;
        self
    }

    pub fn with_bulkhead(mut self, bulkhead: BulkheadOptions) -> Self {
        self.bulkhead = bulkhead;
        self
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = Some(timeout);
        self
    }
}

/// Nest-style injectable client: discover → balance → resilient HTTP.
#[derive(Clone)]
pub struct ServiceClient {
    http: HttpService,
    balancer: ServiceLoadBalancer,
    breaker: CircuitBreaker,
    bulkhead: Bulkhead,
    retry: RetryPolicy,
    degrade: Option<DegradePolicy>,
    request_timeout: Option<Duration>,
}

impl fmt::Debug for ServiceClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServiceClient")
            .field("request_timeout", &self.request_timeout)
            .finish_non_exhaustive()
    }
}

impl ServiceClient {
    pub fn new(
        http: HttpService,
        discovery: Arc<dyn ServiceDiscovery>,
        options: ServiceClientOptions,
    ) -> Self {
        Self {
            http,
            balancer: ServiceLoadBalancer::new(discovery),
            breaker: CircuitBreaker::new(options.circuit_breaker),
            bulkhead: Bulkhead::new(options.bulkhead),
            retry: RetryPolicy::new(options.retry),
            degrade: options.degrade,
            request_timeout: options.request_timeout,
        }
    }

    pub fn circuit_state(&self) -> crate::resilience::CircuitState {
        self.breaker.state()
    }

    pub async fn call(
        &self,
        service: &str,
        method: HttpMethod,
        path: &str,
    ) -> Result<HttpClientResponse> {
        self.call_request(service, HttpClientRequest::new(method, path))
            .await
    }

    pub async fn get(&self, service: &str, path: &str) -> Result<HttpClientResponse> {
        self.call(service, HttpMethod::Get, path).await
    }

    pub async fn get_json<T>(&self, service: &str, path: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.get(service, path).await?.body_json()
    }

    pub async fn post_json<T, B>(&self, service: &str, path: &str, body: &B) -> Result<T>
    where
        T: DeserializeOwned,
        B: Serialize,
    {
        let response = self
            .call_request(service, HttpClientRequest::post(path).with_json(body)?)
            .await?;
        response.body_json()
    }

    pub async fn call_request(
        &self,
        service: &str,
        request: HttpClientRequest,
    ) -> Result<HttpClientResponse> {
        let _permit = match self.bulkhead.try_acquire() {
            Ok(permit) => permit,
            Err(_) => {
                return self.degrade_or_err(
                    true,
                    BootError::TooManyRequests("service client bulkhead full".to_string()),
                );
            }
        };

        if let Err(error) = self.breaker.admit() {
            return self.degrade_or_err(
                self.degrade
                    .as_ref()
                    .is_some_and(|policy| policy.on_circuit_open()),
                BootError::ServiceUnavailable(format!(
                    "circuit open for service '{service}': {error}"
                )),
            );
        }

        let mut attempt = 0u32;
        loop {
            let instance = match self.balancer.select(service) {
                Ok(instance) => instance,
                Err(error) => {
                    self.breaker.on_failure();
                    return self.degrade_or_err(
                        self.degrade
                            .as_ref()
                            .is_some_and(|policy| policy.on_exhausted_retries()),
                        error,
                    );
                }
            };

            let mut outbound = request
                .clone()
                .with_url(join_url(&instance.base_url(), request.url())?);
            if outbound.timeout().is_none() {
                if let Some(timeout) = self.request_timeout {
                    outbound = outbound.with_timeout(timeout);
                }
            }
            outbound = inject_propagation_headers(outbound);

            match self.http.request(outbound).await {
                Ok(response) if response.is_success() => {
                    self.breaker.on_success();
                    return Ok(response);
                }
                Ok(response) => {
                    let status = response.status();
                    if self.retry.should_retry_status(status, attempt) {
                        attempt = attempt.saturating_add(1);
                        self.breaker.on_failure();
                        tokio::time::sleep(self.retry.delay_for_attempt(attempt)).await;
                        continue;
                    }
                    self.breaker.on_failure();
                    return self.degrade_or_err(
                        self.degrade
                            .as_ref()
                            .is_some_and(|policy| policy.on_exhausted_retries()),
                        BootError::BadGateway(format!(
                            "service '{service}' returned status {status}"
                        )),
                    );
                }
                Err(error) => {
                    if self.retry.should_retry_error(attempt) {
                        attempt = attempt.saturating_add(1);
                        self.breaker.on_failure();
                        tokio::time::sleep(self.retry.delay_for_attempt(attempt)).await;
                        continue;
                    }
                    self.breaker.on_failure();
                    return self.degrade_or_err(
                        self.degrade
                            .as_ref()
                            .is_some_and(|policy| policy.on_exhausted_retries()),
                        error,
                    );
                }
            }
        }
    }

    fn degrade_or_err(&self, allow_degrade: bool, error: BootError) -> Result<HttpClientResponse> {
        if allow_degrade {
            if let Some(policy) = &self.degrade {
                return Ok(degraded_response(policy.fallback()));
            }
        }
        Err(error)
    }
}

fn join_url(base: &str, path: &str) -> Result<String> {
    if path.starts_with("http://") || path.starts_with("https://") {
        return Ok(path.to_string());
    }
    let base = base.trim_end_matches('/');
    if path.starts_with('/') {
        Ok(format!("{base}{path}"))
    } else {
        Ok(format!("{base}/{path}"))
    }
}

/// Inject W3C/`x-request-id`/`x-correlation-id` from the inbound request context.
fn inject_propagation_headers(mut request: HttpClientRequest) -> HttpClientRequest {
    #[cfg(feature = "request-context")]
    if let Some(ctx) = crate::RequestContext::try_current() {
        if request.header("traceparent").is_none() {
            if let Some(value) = ctx.header("traceparent") {
                request = request.with_header("traceparent", value);
            }
        }
        if request.header("tracestate").is_none() {
            if let Some(value) = ctx.header("tracestate") {
                request = request.with_header("tracestate", value);
            }
        }
        let request_id = ctx
            .request_id()
            .or_else(|| ctx.header("x-request-id"))
            .map(str::to_owned);
        if request.header("x-request-id").is_none() {
            if let Some(value) = request_id.clone() {
                request = request.with_header("x-request-id", value);
            }
        }
        if request.header("x-correlation-id").is_none() {
            if let Some(value) = ctx
                .header("x-correlation-id")
                .map(str::to_owned)
                .or(request_id)
            {
                request = request.with_header("x-correlation-id", value);
            }
        }
    }
    request
}

/// Nest-style module that exports a shared [`ServiceClient`].
pub struct ServiceClientModule {
    name: &'static str,
    token: ProviderToken,
    discovery: Arc<dyn ServiceDiscovery>,
    http_backend: Option<Arc<dyn HttpClientBackend>>,
    http_options: HttpClientOptions,
    client_options: ServiceClientOptions,
    global: bool,
}

impl ServiceClientModule {
    pub fn new(name: &'static str, discovery: Arc<dyn ServiceDiscovery>) -> Self {
        Self {
            name,
            token: ProviderToken::of::<ServiceClient>(),
            discovery,
            http_backend: None,
            http_options: HttpClientOptions::default(),
            client_options: ServiceClientOptions::default(),
            global: false,
        }
    }

    pub fn with_backend<B>(mut self, backend: B) -> Self
    where
        B: HttpClientBackend,
    {
        self.http_backend = Some(Arc::new(backend));
        self
    }

    pub fn with_http_options(mut self, options: HttpClientOptions) -> Self {
        self.http_options = options;
        self
    }

    pub fn with_client_options(mut self, options: ServiceClientOptions) -> Self {
        self.client_options = options;
        self
    }

    pub fn named(mut self, name: impl Into<String>) -> Self {
        self.token = ProviderToken::named(name);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for ServiceClientModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        let discovery = self.discovery.clone();
        let http_backend = self.http_backend.clone();
        let http_options = self.http_options.clone();
        let client_options = self.client_options.clone();
        let token = self.token.as_str().to_string();

        Ok(vec![ProviderDefinition::named_factory(
            token,
            move |_module: &ModuleRef| -> Result<ServiceClient> {
                let http = match &http_backend {
                    Some(backend) => {
                        HttpService::from_backend_arc(backend.clone(), http_options.clone())
                    }
                    None => HttpService::with_options(http_options.clone()),
                };
                Ok(ServiceClient::new(
                    http,
                    discovery.clone(),
                    client_options.clone(),
                ))
            },
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}
