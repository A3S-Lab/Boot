//! Consecutive-failure circuit breaker (Closed → Open → HalfOpen).

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Circuit breaker configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitBreakerOptions {
    pub failure_threshold: u32,
    pub cooldown: Duration,
    pub success_threshold: u32,
}

impl Default for CircuitBreakerOptions {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            cooldown: Duration::from_secs(30),
            success_threshold: 1,
        }
    }
}

impl CircuitBreakerOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_failure_threshold(mut self, failure_threshold: u32) -> Self {
        self.failure_threshold = failure_threshold;
        self
    }

    pub fn with_cooldown(mut self, cooldown: Duration) -> Self {
        self.cooldown = cooldown;
        self
    }

    pub fn with_success_threshold(mut self, success_threshold: u32) -> Self {
        self.success_threshold = success_threshold;
        self
    }
}

/// Public circuit state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitState {
    Closed,
    Open,
    HalfOpen,
}

/// Rejected because the circuit is open (or half-open probe already in flight).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CircuitTripError {
    pub state: CircuitState,
}

impl std::fmt::Display for CircuitTripError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "circuit breaker is {:?}", self.state)
    }
}

impl std::error::Error for CircuitTripError {}

#[derive(Debug)]
struct Inner {
    state: CircuitState,
    consecutive_failures: u32,
    consecutive_successes: u32,
    opened_at: Option<Instant>,
    probe_in_flight: bool,
}

/// Thread-safe consecutive-failure circuit breaker.
#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    options: CircuitBreakerOptions,
    inner: Arc<Mutex<Inner>>,
}

impl CircuitBreaker {
    pub fn new(options: CircuitBreakerOptions) -> Self {
        Self {
            options,
            inner: Arc::new(Mutex::new(Inner {
                state: CircuitState::Closed,
                consecutive_failures: 0,
                consecutive_successes: 0,
                opened_at: None,
                probe_in_flight: false,
            })),
        }
    }

    pub fn options(&self) -> &CircuitBreakerOptions {
        &self.options
    }

    pub fn state(&self) -> CircuitState {
        let mut inner = self.inner.lock().expect("circuit breaker lock");
        self.maybe_half_open(&mut inner);
        inner.state
    }

    /// Admit a call. Returns `Err` when the circuit rejects the attempt.
    pub fn admit(&self) -> Result<(), CircuitTripError> {
        let mut inner = self.inner.lock().expect("circuit breaker lock");
        self.maybe_half_open(&mut inner);
        match inner.state {
            CircuitState::Closed => Ok(()),
            CircuitState::Open => Err(CircuitTripError {
                state: CircuitState::Open,
            }),
            CircuitState::HalfOpen => {
                if inner.probe_in_flight {
                    Err(CircuitTripError {
                        state: CircuitState::HalfOpen,
                    })
                } else {
                    inner.probe_in_flight = true;
                    Ok(())
                }
            }
        }
    }

    pub fn on_success(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock");
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures = 0;
            }
            CircuitState::HalfOpen => {
                inner.probe_in_flight = false;
                inner.consecutive_successes = inner.consecutive_successes.saturating_add(1);
                if inner.consecutive_successes >= self.options.success_threshold {
                    inner.state = CircuitState::Closed;
                    inner.consecutive_failures = 0;
                    inner.consecutive_successes = 0;
                    inner.opened_at = None;
                }
            }
            CircuitState::Open => {}
        }
    }

    pub fn on_failure(&self) {
        let mut inner = self.inner.lock().expect("circuit breaker lock");
        match inner.state {
            CircuitState::Closed => {
                inner.consecutive_failures = inner.consecutive_failures.saturating_add(1);
                if inner.consecutive_failures >= self.options.failure_threshold {
                    inner.state = CircuitState::Open;
                    inner.opened_at = Some(Instant::now());
                    inner.consecutive_successes = 0;
                }
            }
            CircuitState::HalfOpen => {
                inner.probe_in_flight = false;
                inner.state = CircuitState::Open;
                inner.opened_at = Some(Instant::now());
                inner.consecutive_successes = 0;
                inner.consecutive_failures = self.options.failure_threshold;
            }
            CircuitState::Open => {
                inner.opened_at = Some(Instant::now());
            }
        }
    }

    fn maybe_half_open(&self, inner: &mut Inner) {
        if inner.state != CircuitState::Open {
            return;
        }
        let Some(opened_at) = inner.opened_at else {
            return;
        };
        if opened_at.elapsed() >= self.options.cooldown {
            inner.state = CircuitState::HalfOpen;
            inner.probe_in_flight = false;
            inner.consecutive_successes = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trips_after_threshold_and_recovers_on_probe_success() {
        let breaker = CircuitBreaker::new(
            CircuitBreakerOptions::new()
                .with_failure_threshold(2)
                .with_cooldown(Duration::from_millis(5))
                .with_success_threshold(1),
        );

        breaker.admit().unwrap();
        breaker.on_failure();
        breaker.admit().unwrap();
        breaker.on_failure();
        assert_eq!(breaker.state(), CircuitState::Open);
        assert!(breaker.admit().is_err());

        std::thread::sleep(Duration::from_millis(10));
        assert_eq!(breaker.state(), CircuitState::HalfOpen);
        breaker.admit().unwrap();
        breaker.on_success();
        assert_eq!(breaker.state(), CircuitState::Closed);
    }
}
