use super::definition::RouteDefinition;
use crate::{BootResponse, BoxFuture, ExecutionContext, Interceptor, Result};

impl RouteDefinition {
    pub fn with_response_header(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.with_interceptor(StaticResponseHeader {
            name: name.into(),
            value: value.into(),
        })
    }

    pub fn with_redirect(self, location: impl Into<String>) -> Self {
        self.with_redirect_status(302, location)
    }

    pub fn with_redirect_status(self, status: u16, location: impl Into<String>) -> Self {
        self.with_interceptor(StaticRedirect {
            status,
            location: location.into(),
        })
    }
}

struct StaticResponseHeader {
    name: String,
    value: String,
}

impl Interceptor for StaticResponseHeader {
    fn after(
        &self,
        _context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let name = self.name.clone();
        let value = self.value.clone();
        Box::pin(async move { Ok(response.with_header(name, value)) })
    }
}

struct StaticRedirect {
    status: u16,
    location: String,
}

impl Interceptor for StaticRedirect {
    fn after(
        &self,
        _context: ExecutionContext,
        _response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let status = self.status;
        let location = self.location.clone();
        Box::pin(async move { Ok(BootResponse::redirect_with_status(status, location)) })
    }
}
