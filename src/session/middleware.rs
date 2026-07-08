use super::manager::SessionManager;
use crate::{BootRequest, BoxFuture, Middleware, MiddlewareOutcome, Result};

/// Middleware that binds a session id to the request.
#[derive(Debug, Clone)]
pub struct SessionMiddleware {
    manager: SessionManager,
}

impl SessionMiddleware {
    pub fn new(manager: SessionManager) -> Self {
        Self { manager }
    }

    pub fn manager(&self) -> &SessionManager {
        &self.manager
    }
}

impl Middleware for SessionMiddleware {
    fn handle(&self, request: BootRequest) -> BoxFuture<'static, Result<MiddlewareOutcome>> {
        let manager = self.manager.clone();
        Box::pin(async move {
            let session_id = match manager.cookie_session_id(&request)? {
                Some(session_id) => session_id,
                None => manager.create_session_id()?,
            };
            Ok(MiddlewareOutcome::next(request.with_header(
                manager.options().request_header_name(),
                session_id,
            )))
        })
    }
}
