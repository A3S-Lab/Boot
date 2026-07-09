use crate::{BootError, BootRequest, BoxFuture, ExecutionContext, Guard, Result};
use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
