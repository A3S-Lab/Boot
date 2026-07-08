use super::manager::SessionManager;
use super::options::SessionOptions;
use crate::{BootRequest, BootResponse, BoxFuture, ExecutionContext, Interceptor, Result};

/// Interceptor that persists or clears the session cookie after handlers run.
#[derive(Debug, Clone)]
pub struct SessionCookieInterceptor {
    manager: SessionManager,
}

impl SessionCookieInterceptor {
    pub fn new(manager: SessionManager) -> Self {
        Self { manager }
    }

    pub fn manager(&self) -> &SessionManager {
        &self.manager
    }
}

impl Interceptor for SessionCookieInterceptor {
    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let manager = self.manager.clone();
        Box::pin(async move { apply_session_cookie(response, &context.request, &manager) })
    }
}

fn apply_session_cookie(
    mut response: BootResponse,
    request: &BootRequest,
    manager: &SessionManager,
) -> Result<BootResponse> {
    let Some(session_id) = manager.session_id(request)? else {
        return Ok(response);
    };
    let cookie_session_id = manager.cookie_session_id(request)?;
    let has_cookie = cookie_session_id.as_deref() == Some(session_id.as_str());
    let has_data = manager.has_data(&session_id)?;

    if has_data && (!has_cookie || manager.options().is_rolling()) {
        response =
            response.append_header("set-cookie", session_cookie_header(manager, &session_id));
    } else if !has_data && has_cookie {
        response = response.append_header("set-cookie", expired_session_cookie_header(manager));
    }

    Ok(response)
}

fn session_cookie_header(manager: &SessionManager, session_id: &str) -> String {
    let options = manager.options();
    let mut cookie = format!(
        "{}={}; Path={}",
        options.cookie_name(),
        session_id,
        options.cookie_path()
    );
    if let Some(ttl) = options.ttl() {
        cookie.push_str(&format!("; Max-Age={}", ttl.as_secs()));
    }
    append_cookie_attributes(&mut cookie, options);
    cookie
}

fn expired_session_cookie_header(manager: &SessionManager) -> String {
    let options = manager.options();
    let mut cookie = format!(
        "{}=; Path={}; Max-Age=0",
        options.cookie_name(),
        options.cookie_path()
    );
    append_cookie_attributes(&mut cookie, options);
    cookie
}

fn append_cookie_attributes(cookie: &mut String, options: &SessionOptions) {
    if let Some(domain) = options.cookie_domain() {
        cookie.push_str(&format!("; Domain={domain}"));
    }
    if options.is_http_only() {
        cookie.push_str("; HttpOnly");
    }
    if options.is_secure() {
        cookie.push_str("; Secure");
    }
    if let Some(same_site) = options.same_site() {
        cookie.push_str(&format!("; SameSite={}", same_site.as_str()));
    }
}
