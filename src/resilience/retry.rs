//! Retry policy for transient outbound failures.

use std::time::Duration;

/// How to classify a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryClassifier {
    /// Retry network/transport failures and HTTP 408/429/5xx.
    TransientHttp,
    /// Never retry.
    Never,
}

impl Default for RetryClassifier {
    fn default() -> Self {
        Self::TransientHttp
    }
}

/// Retry configuration.
#[derive(Debug, Clone, PartialEq)]
pub struct RetryOptions {
    pub max_retries: u32,
    pub initial_delay: Duration,
    pub multiplier: f64,
    pub max_delay: Duration,
    pub classifier: RetryClassifier,
}

impl Default for RetryOptions {
    fn default() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_millis(50),
            multiplier: 2.0,
            max_delay: Duration::from_secs(2),
            classifier: RetryClassifier::TransientHttp,
        }
    }
}

impl RetryOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }

    pub fn with_initial_delay(mut self, initial_delay: Duration) -> Self {
        self.initial_delay = initial_delay;
        self
    }

    pub fn with_multiplier(mut self, multiplier: f64) -> Self {
        self.multiplier = multiplier;
        self
    }

    pub fn with_max_delay(mut self, max_delay: Duration) -> Self {
        self.max_delay = max_delay;
        self
    }

    pub fn with_classifier(mut self, classifier: RetryClassifier) -> Self {
        self.classifier = classifier;
        self
    }

    pub fn disabled() -> Self {
        Self {
            max_retries: 0,
            ..Self::default()
        }
    }
}

/// Computes retry delays and classifies failures.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    options: RetryOptions,
}

impl RetryPolicy {
    pub fn new(options: RetryOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &RetryOptions {
        &self.options
    }

    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        if attempt == 0 {
            return Duration::ZERO;
        }
        let factor = self.options.multiplier.powi((attempt - 1) as i32);
        let millis = (self.options.initial_delay.as_secs_f64() * 1000.0 * factor).round() as u64;
        Duration::from_millis(millis).min(self.options.max_delay)
    }

    pub fn should_retry_status(&self, status: u16, attempt: u32) -> bool {
        if attempt >= self.options.max_retries {
            return false;
        }
        match self.options.classifier {
            RetryClassifier::Never => false,
            RetryClassifier::TransientHttp => matches!(status, 408 | 429 | 500..=599),
        }
    }

    pub fn should_retry_error(&self, attempt: u32) -> bool {
        if attempt >= self.options.max_retries {
            return false;
        }
        !matches!(self.options.classifier, RetryClassifier::Never)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exponential_delay_caps_at_max() {
        let policy = RetryPolicy::new(
            RetryOptions::new()
                .with_initial_delay(Duration::from_millis(10))
                .with_multiplier(2.0)
                .with_max_delay(Duration::from_millis(30)),
        );
        assert_eq!(policy.delay_for_attempt(1), Duration::from_millis(10));
        assert_eq!(policy.delay_for_attempt(2), Duration::from_millis(20));
        assert_eq!(policy.delay_for_attempt(3), Duration::from_millis(30));
    }
}
