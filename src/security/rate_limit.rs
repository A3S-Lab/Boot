use crate::{BootError, BootRequest, BoxFuture, ExecutionContext, Guard, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const DEFAULT_POLICY_ID: &str = "global";
const MAX_POLICY_ID_BYTES: usize = 128;

/// Application-wide rate limit settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitOptions {
    policy_id: String,
    max_requests: u32,
    window: Duration,
    key_headers: Vec<String>,
    use_bearer_token: bool,
    anonymous_key: String,
}

impl Default for RateLimitOptions {
    fn default() -> Self {
        Self {
            policy_id: DEFAULT_POLICY_ID.to_string(),
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

    /// Set a stable policy identifier shared by every process using the same provider policy.
    pub fn with_policy_id(mut self, policy_id: impl Into<String>) -> Self {
        self.policy_id = policy_id.into();
        self
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

    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }

    pub fn max_requests(&self) -> u32 {
        self.max_requests
    }

    pub fn window(&self) -> Duration {
        self.window
    }

    fn validate(&self) -> Result<()> {
        if self.max_requests == 0 {
            return Err(BootError::Internal(
                "rate limit max_requests must be greater than zero".to_string(),
            ));
        }
        if self.window.is_zero() {
            return Err(BootError::Internal(
                "rate limit window must be greater than zero".to_string(),
            ));
        }
        if !valid_policy_id(&self.policy_id) {
            return Err(BootError::Internal(format!(
                "rate limit policy_id must be 1 to {MAX_POLICY_ID_BYTES} ASCII identifier bytes"
            )));
        }
        Ok(())
    }
}

/// One atomic request to a [`RateLimitProvider`].
///
/// The subject is a policy-scoped, domain-separated SHA-256 digest. Header values and bearer
/// credentials never cross the provider boundary in plaintext.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RateLimitRequest {
    policy_id: String,
    subject_hash: String,
    max_requests: u32,
    window: Duration,
}

impl RateLimitRequest {
    pub fn policy_id(&self) -> &str {
        &self.policy_id
    }

    pub fn subject_hash(&self) -> &str {
        &self.subject_hash
    }

    pub fn max_requests(&self) -> u32 {
        self.max_requests
    }

    pub fn window(&self) -> Duration {
        self.window
    }
}

/// Result of an atomic rate limit acquisition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateLimitDecision {
    /// The provider consumed one request from the policy budget.
    Allowed,
    /// The provider rejected the request because its policy budget is exhausted.
    Limited,
}

impl RateLimitDecision {
    pub fn is_allowed(self) -> bool {
        matches!(self, Self::Allowed)
    }
}

/// Provider-neutral atomic rate limit boundary.
///
/// A distributed implementation can use Redis, PostgreSQL, or another shared service without
/// exposing that backend to Boot. Implementations must atomically consume one request for the
/// `(policy_id, subject_hash)` pair. Every client of a stable policy identifier must use the same
/// request limit and window. Providers must reject conflicting settings, and returning any error
/// rejects the guarded request.
pub trait RateLimitProvider: Send + Sync + 'static {
    fn acquire(&self, request: RateLimitRequest) -> BoxFuture<'static, Result<RateLimitDecision>>;
}

impl<T> RateLimitProvider for Arc<T>
where
    T: RateLimitProvider + ?Sized,
{
    fn acquire(&self, request: RateLimitRequest) -> BoxFuture<'static, Result<RateLimitDecision>> {
        self.as_ref().acquire(request)
    }
}

/// Process-local fixed-window provider used by [`RateLimitGuard::with_options`].
#[derive(Debug, Clone, Default)]
pub struct InMemoryRateLimitProvider {
    state: Arc<Mutex<InMemoryRateLimitState>>,
}

#[derive(Debug, Default)]
struct InMemoryRateLimitState {
    policies: BTreeMap<String, RateLimitPolicy>,
    buckets: BTreeMap<(String, String), RateLimitBucket>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RateLimitPolicy {
    max_requests: u32,
    window: Duration,
}

#[derive(Debug, Clone)]
struct RateLimitBucket {
    window_started_at: Instant,
    count: u32,
}

impl InMemoryRateLimitProvider {
    pub fn new() -> Self {
        Self::default()
    }
}

impl RateLimitProvider for InMemoryRateLimitProvider {
    fn acquire(&self, request: RateLimitRequest) -> BoxFuture<'static, Result<RateLimitDecision>> {
        let state = Arc::clone(&self.state);
        Box::pin(async move {
            let now = Instant::now();
            let mut state = state.lock().map_err(|_| {
                BootError::Internal("rate limit state lock is poisoned".to_string())
            })?;
            let requested_policy = RateLimitPolicy {
                max_requests: request.max_requests,
                window: request.window,
            };
            if let Some(policy) = state.policies.get(&request.policy_id) {
                if *policy != requested_policy {
                    return Err(BootError::Internal(
                        "rate limit policy settings conflict across provider clients".to_string(),
                    ));
                }
            } else {
                state
                    .policies
                    .insert(request.policy_id.clone(), requested_policy);
            }

            let InMemoryRateLimitState { policies, buckets } = &mut *state;
            buckets.retain(|(policy_id, _), bucket| {
                policies.get(policy_id).is_some_and(|policy| {
                    now.duration_since(bucket.window_started_at) < policy.window
                })
            });

            let key = (request.policy_id.clone(), request.subject_hash.clone());
            let bucket = buckets.entry(key).or_insert_with(|| RateLimitBucket {
                window_started_at: now,
                count: 0,
            });
            let elapsed = now.duration_since(bucket.window_started_at);
            if elapsed >= requested_policy.window {
                bucket.window_started_at = now;
                bucket.count = 0;
            }
            if bucket.count >= requested_policy.max_requests {
                return Ok(RateLimitDecision::Limited);
            }

            bucket.count += 1;
            Ok(RateLimitDecision::Allowed)
        })
    }
}

/// Guard that enforces a fixed-window policy through a [`RateLimitProvider`].
#[derive(Clone)]
pub struct RateLimitGuard {
    options: RateLimitOptions,
    provider: Arc<dyn RateLimitProvider>,
}

impl fmt::Debug for RateLimitGuard {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("RateLimitGuard")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl Default for RateLimitGuard {
    fn default() -> Self {
        Self::with_options(RateLimitOptions::default())
    }
}

impl RateLimitGuard {
    pub fn new() -> Self {
        Self::default()
    }

    /// Construct a guard with the process-local provider.
    pub fn with_options(options: RateLimitOptions) -> Self {
        Self::with_provider(options, InMemoryRateLimitProvider::new())
    }

    /// Construct a guard with an application-supplied provider.
    pub fn with_provider<P>(options: RateLimitOptions, provider: P) -> Self
    where
        P: RateLimitProvider,
    {
        Self {
            options,
            provider: Arc::new(provider),
        }
    }

    pub fn options(&self) -> &RateLimitOptions {
        &self.options
    }
}

impl Guard for RateLimitGuard {
    fn can_activate(&self, context: ExecutionContext) -> BoxFuture<'static, Result<bool>> {
        let options = self.options.clone();
        let provider = Arc::clone(&self.provider);
        Box::pin(async move {
            options.validate()?;
            let request = rate_limit_request(&context.request, &options);
            let decision = provider.acquire(request).await?;
            if decision.is_allowed() {
                Ok(true)
            } else {
                Err(BootError::TooManyRequests(
                    "rate limit exceeded".to_string(),
                ))
            }
        })
    }
}

fn rate_limit_request(request: &BootRequest, options: &RateLimitOptions) -> RateLimitRequest {
    RateLimitRequest {
        policy_id: options.policy_id.clone(),
        subject_hash: rate_limit_subject_hash(request, options),
        max_requests: options.max_requests,
        window: options.window,
    }
}

fn rate_limit_subject_hash(request: &BootRequest, options: &RateLimitOptions) -> String {
    for header in &options.key_headers {
        if let Some(value) = request
            .header(header)
            .and_then(|value| value.split(',').next())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return hash_subject(&[
                options.policy_id(),
                "header",
                &header.to_ascii_lowercase(),
                value,
            ]);
        }
    }

    if options.use_bearer_token {
        if let Some(token) = request.bearer_token() {
            return hash_subject(&[options.policy_id(), "bearer", token]);
        }
    }

    hash_subject(&[options.policy_id(), "anonymous", &options.anonymous_key])
}

fn hash_subject(parts: &[&str]) -> String {
    let mut digest = Sha256::new();
    digest.update(b"a3s-boot-rate-limit-subject-v1\0");
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part.as_bytes());
    }
    format!("{:x}", digest.finalize())
}

fn valid_policy_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_POLICY_ID_BYTES
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b':' | b'/')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn subject_hash_is_stable_and_does_not_embed_input() {
        let options = RateLimitOptions::new().with_key_header("x-user-id");
        let request = BootRequest::new(crate::HttpMethod::Get, "/")
            .with_header("x-user-id", "tenant-user-secret-material");
        let first = rate_limit_subject_hash(&request, &options);
        let second = rate_limit_subject_hash(&request, &options);

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert!(!first.contains("tenant-user-secret-material"));
    }

    #[test]
    fn subject_hash_is_scoped_to_the_policy() {
        let request = BootRequest::new(crate::HttpMethod::Get, "/")
            .with_header("authorization", "Bearer tenant-secret-token");
        let first = rate_limit_subject_hash(
            &request,
            &RateLimitOptions::new().with_policy_id("public-api"),
        );
        let second = rate_limit_subject_hash(
            &request,
            &RateLimitOptions::new().with_policy_id("admin-api"),
        );

        assert_ne!(first, second);
    }

    #[test]
    fn invalid_policy_settings_fail_before_provider_use() {
        assert!(RateLimitOptions::new()
            .with_policy_id("invalid policy")
            .validate()
            .is_err());
        assert!(RateLimitOptions::new()
            .with_max_requests(0)
            .validate()
            .is_err());
        assert!(RateLimitOptions::new()
            .with_window(Duration::ZERO)
            .validate()
            .is_err());
    }
}
