//! Feature-flag client surface (P4.2).

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

/// Evaluates boolean feature flags for a process.
pub trait FeatureFlagClient: Send + Sync + 'static {
    fn enabled(&self, key: &str) -> bool;
}

/// Static in-memory flag map (default / tests).
#[derive(Debug, Default, Clone)]
pub struct StaticFeatureFlags {
    flags: Arc<RwLock<BTreeMap<String, bool>>>,
}

impl StaticFeatureFlags {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set(&self, key: impl Into<String>, enabled: bool) {
        if let Ok(mut guard) = self.flags.write() {
            guard.insert(key.into(), enabled);
        }
    }
}

impl FeatureFlagClient for StaticFeatureFlags {
    fn enabled(&self, key: &str) -> bool {
        self.flags
            .read()
            .ok()
            .and_then(|guard| guard.get(key).copied())
            .unwrap_or(false)
    }
}
