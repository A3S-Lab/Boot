//! Explicit degrade / fallback payloads for open circuits and exhausted retries.

/// Static fallback body returned when a call is degraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FallbackResponse {
    pub status: u16,
    pub body: Vec<u8>,
    pub content_type: String,
}

impl FallbackResponse {
    pub fn json(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            body: body.into(),
            content_type: "application/json".to_string(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            body: body.into().into_bytes(),
            content_type: "text/plain; charset=utf-8".to_string(),
        }
    }
}

/// When and how to degrade an outbound call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DegradePolicy {
    fallback: FallbackResponse,
    on_circuit_open: bool,
    on_exhausted_retries: bool,
}

impl DegradePolicy {
    pub fn new(fallback: FallbackResponse) -> Self {
        Self {
            fallback,
            on_circuit_open: true,
            on_exhausted_retries: true,
        }
    }

    pub fn with_on_circuit_open(mut self, enabled: bool) -> Self {
        self.on_circuit_open = enabled;
        self
    }

    pub fn with_on_exhausted_retries(mut self, enabled: bool) -> Self {
        self.on_exhausted_retries = enabled;
        self
    }

    pub fn on_circuit_open(&self) -> bool {
        self.on_circuit_open
    }

    pub fn on_exhausted_retries(&self) -> bool {
        self.on_exhausted_retries
    }

    pub fn fallback(&self) -> &FallbackResponse {
        &self.fallback
    }
}
