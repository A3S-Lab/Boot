use std::any::type_name;
use std::fmt;

/// Stable lookup key for an injectable provider.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProviderToken(String);

impl ProviderToken {
    pub fn named(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    pub fn of<T>() -> Self
    where
        T: Send + Sync + 'static,
    {
        Self(type_name::<T>().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ProviderToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}
