//! East-west outbound identity plug-in (P1.1).

use crate::http_client::HttpClientRequest;
use crate::Result;

/// Injects service credentials onto outbound Boot HTTP calls.
pub trait OutboundIdentity: Send + Sync + 'static {
    fn authorize(&self, request: HttpClientRequest) -> Result<HttpClientRequest>;
}

/// Bearer token injector for workload/service callers.
#[derive(Debug, Clone)]
pub struct BearerOutboundIdentity {
    token: String,
}

impl BearerOutboundIdentity {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            token: token.into(),
        }
    }
}

impl OutboundIdentity for BearerOutboundIdentity {
    fn authorize(&self, request: HttpClientRequest) -> Result<HttpClientRequest> {
        if self.token.trim().is_empty() {
            return Ok(request);
        }
        Ok(request.with_header("authorization", format!("Bearer {}", self.token)))
    }
}

/// No-op identity (human-facing or open networks).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoopOutboundIdentity;

impl OutboundIdentity for NoopOutboundIdentity {
    fn authorize(&self, request: HttpClientRequest) -> Result<HttpClientRequest> {
        Ok(request)
    }
}
