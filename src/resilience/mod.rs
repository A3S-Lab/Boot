//! Outbound resilience primitives for microservice clients.
//!
//! These policies sit on the **client** side of a call (Boot service → peer).
//! Edge ingress resilience (rate limit, circuit break on upstream, failover)
//! belongs to `a3s-gateway`.

mod bulkhead;
mod circuit_breaker;
mod degrade;
mod retry;

pub use bulkhead::{Bulkhead, BulkheadError, BulkheadOptions};
pub use circuit_breaker::{CircuitBreaker, CircuitBreakerOptions, CircuitState, CircuitTripError};
pub use degrade::{DegradePolicy, FallbackResponse};
pub use retry::{RetryClassifier, RetryOptions, RetryPolicy};
