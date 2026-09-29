//! Bounded concurrency (bulkhead) for outbound calls.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Bulkhead configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkheadOptions {
    pub max_concurrent: usize,
}

impl Default for BulkheadOptions {
    fn default() -> Self {
        Self { max_concurrent: 64 }
    }
}

impl BulkheadOptions {
    pub fn new(max_concurrent: usize) -> Self {
        Self { max_concurrent }
    }
}

/// Rejected because the concurrency limit was reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BulkheadError;

impl std::fmt::Display for BulkheadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bulkhead capacity exhausted")
    }
}

impl std::error::Error for BulkheadError {}

/// Permit that releases capacity on drop.
#[derive(Debug)]
pub struct BulkheadPermit {
    in_flight: Arc<AtomicUsize>,
}

impl Drop for BulkheadPermit {
    fn drop(&mut self) {
        self.in_flight.fetch_sub(1, Ordering::AcqRel);
    }
}

/// Fixed-size concurrency limiter.
#[derive(Debug, Clone)]
pub struct Bulkhead {
    options: BulkheadOptions,
    in_flight: Arc<AtomicUsize>,
}

impl Bulkhead {
    pub fn new(options: BulkheadOptions) -> Self {
        Self {
            options,
            in_flight: Arc::new(AtomicUsize::new(0)),
        }
    }

    pub fn try_acquire(&self) -> Result<BulkheadPermit, BulkheadError> {
        loop {
            let current = self.in_flight.load(Ordering::Acquire);
            if current >= self.options.max_concurrent {
                return Err(BulkheadError);
            }
            if self
                .in_flight
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(BulkheadPermit {
                    in_flight: self.in_flight.clone(),
                });
            }
        }
    }

    pub fn in_flight(&self) -> usize {
        self.in_flight.load(Ordering::Acquire)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_when_full_and_releases_on_drop() {
        let bulkhead = Bulkhead::new(BulkheadOptions::new(1));
        let permit = bulkhead.try_acquire().unwrap();
        assert!(bulkhead.try_acquire().is_err());
        drop(permit);
        assert!(bulkhead.try_acquire().is_ok());
    }
}
