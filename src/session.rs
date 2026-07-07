use crate::{
    BootError, BootRequest, BootResponse, BoxFuture, ExecutionContext, Interceptor, Middleware,
    MiddlewareOutcome, Module, ProviderDefinition, ProviderToken, Result,
};
use serde::de::DeserializeOwned;
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// SameSite attribute used for the session cookie.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionCookieSameSite {
    Strict,
    Lax,
    None,
}

impl SessionCookieSameSite {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "Strict",
            Self::Lax => "Lax",
            Self::None => "None",
        }
    }
}

/// Session settings shared by the manager, middleware, and cookie interceptor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionOptions {
    cookie_name: String,
    request_header_name: String,
    ttl: Option<Duration>,
    cookie_path: String,
    cookie_domain: Option<String>,
    http_only: bool,
    secure: bool,
    same_site: Option<SessionCookieSameSite>,
    rolling: bool,
}

impl Default for SessionOptions {
    fn default() -> Self {
        Self {
            cookie_name: "a3s.sid".to_string(),
            request_header_name: "x-a3s-session-id".to_string(),
            ttl: Some(Duration::from_secs(60 * 60 * 24)),
            cookie_path: "/".to_string(),
            cookie_domain: None,
            http_only: true,
            secure: false,
            same_site: Some(SessionCookieSameSite::Lax),
            rolling: false,
        }
    }
}

impl SessionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_cookie_name(mut self, cookie_name: impl Into<String>) -> Self {
        self.cookie_name = cookie_name.into();
        self
    }

    pub fn with_request_header_name(mut self, header_name: impl Into<String>) -> Self {
        self.request_header_name = header_name.into();
        self
    }

    pub fn with_ttl(mut self, ttl: Duration) -> Self {
        self.ttl = Some(ttl);
        self
    }

    pub fn without_ttl(mut self) -> Self {
        self.ttl = None;
        self
    }

    pub fn with_cookie_path(mut self, path: impl Into<String>) -> Self {
        self.cookie_path = path.into();
        self
    }

    pub fn with_cookie_domain(mut self, domain: impl Into<String>) -> Self {
        self.cookie_domain = Some(domain.into());
        self
    }

    pub fn without_cookie_domain(mut self) -> Self {
        self.cookie_domain = None;
        self
    }

    pub fn http_only(mut self, http_only: bool) -> Self {
        self.http_only = http_only;
        self
    }

    pub fn secure(mut self, secure: bool) -> Self {
        self.secure = secure;
        self
    }

    pub fn with_same_site(mut self, same_site: SessionCookieSameSite) -> Self {
        self.same_site = Some(same_site);
        self
    }

    pub fn without_same_site(mut self) -> Self {
        self.same_site = None;
        self
    }

    pub fn rolling(mut self, rolling: bool) -> Self {
        self.rolling = rolling;
        self
    }

    pub fn cookie_name(&self) -> &str {
        &self.cookie_name
    }

    pub fn request_header_name(&self) -> &str {
        &self.request_header_name
    }

    pub fn ttl(&self) -> Option<Duration> {
        self.ttl
    }
}

/// Storage backend for session data.
pub trait SessionStore: Send + Sync + 'static {
    fn load(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>>;

    fn save(
        &self,
        session_id: String,
        data: BTreeMap<String, Value>,
        ttl: Option<Duration>,
    ) -> Result<()>;

    fn remove(&self, session_id: &str) -> Result<bool>;

    fn clear(&self) -> Result<()>;
}

/// In-memory session store suitable for tests and single-process services.
#[derive(Debug, Clone, Default)]
pub struct InMemorySessionStore {
    sessions: Arc<RwLock<BTreeMap<String, StoredSession>>>,
}

#[derive(Debug, Clone)]
struct StoredSession {
    data: BTreeMap<String, Value>,
    expires_at: Option<Instant>,
}

impl StoredSession {
    fn is_expired(&self) -> bool {
        self.expires_at
            .is_some_and(|expires_at| Instant::now() >= expires_at)
    }
}

impl InMemorySessionStore {
    pub fn new() -> Self {
        Self::default()
    }

    fn write_sessions(
        &self,
    ) -> Result<std::sync::RwLockWriteGuard<'_, BTreeMap<String, StoredSession>>> {
        self.sessions
            .write()
            .map_err(|_| BootError::Internal("session store lock is poisoned".to_string()))
    }
}

impl SessionStore for InMemorySessionStore {
    fn load(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>> {
        let mut sessions = self.write_sessions()?;
        let Some(session) = sessions.get(session_id) else {
            return Ok(None);
        };

        if session.is_expired() {
            sessions.remove(session_id);
            return Ok(None);
        }

        Ok(Some(session.data.clone()))
    }

    fn save(
        &self,
        session_id: String,
        data: BTreeMap<String, Value>,
        ttl: Option<Duration>,
    ) -> Result<()> {
        let expires_at = ttl.map(|ttl| Instant::now() + ttl);
        self.write_sessions()?
            .insert(session_id, StoredSession { data, expires_at });
        Ok(())
    }

    fn remove(&self, session_id: &str) -> Result<bool> {
        Ok(self.write_sessions()?.remove(session_id).is_some())
    }

    fn clear(&self) -> Result<()> {
        self.write_sessions()?.clear();
        Ok(())
    }
}

/// Provider-backed session manager.
#[derive(Clone)]
pub struct SessionManager {
    store: Arc<dyn SessionStore>,
    options: SessionOptions,
}

impl fmt::Debug for SessionManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionManager")
            .field("options", &self.options)
            .finish_non_exhaustive()
    }
}

impl SessionManager {
    pub fn new<S>(store: S) -> Self
    where
        S: SessionStore,
    {
        Self::from_store_arc(Arc::new(store))
    }

    pub fn from_store_arc(store: Arc<dyn SessionStore>) -> Self {
        Self {
            store,
            options: SessionOptions::default(),
        }
    }

    pub fn in_memory(options: SessionOptions) -> Self {
        Self::new(InMemorySessionStore::new()).with_options(options)
    }

    pub fn with_options(mut self, options: SessionOptions) -> Self {
        self.options = options;
        self
    }

    pub fn options(&self) -> &SessionOptions {
        &self.options
    }

    pub fn session_id(&self, request: &BootRequest) -> Result<Option<String>> {
        if let Some(session_id) = request.header(self.options.request_header_name()) {
            return Ok(Some(validate_session_id(session_id.to_string())?));
        }

        self.cookie_session_id(request)
    }

    pub fn require_session_id(&self, request: &BootRequest) -> Result<String> {
        self.session_id(request)?
            .ok_or_else(|| BootError::Unauthorized("missing session id".to_string()))
    }

    pub fn cookie_session_id(&self, request: &BootRequest) -> Result<Option<String>> {
        request
            .cookie(self.options.cookie_name())?
            .map(validate_session_id)
            .transpose()
    }

    pub fn create_session_id(&self) -> Result<String> {
        generate_session_id()
    }

    pub fn get<T>(&self, session_id: &str, key: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.get_value(session_id, key)? else {
            return Ok(None);
        };
        serde_json::from_value(value)
            .map(Some)
            .map_err(|error| BootError::Internal(format!("invalid session value `{key}`: {error}")))
    }

    pub fn get_value(&self, session_id: &str, key: &str) -> Result<Option<Value>> {
        Ok(self
            .load_data(session_id)?
            .and_then(|data| data.get(key).cloned()))
    }

    pub fn set<T>(&self, session_id: &str, key: impl Into<String>, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!("failed to serialize session value: {error}"))
        })?;
        self.set_value(session_id, key, value)
    }

    pub fn set_value(&self, session_id: &str, key: impl Into<String>, value: Value) -> Result<()> {
        let session_id = validate_session_id(session_id.to_string())?;
        let mut data = self.load_data(&session_id)?.unwrap_or_default();
        data.insert(key.into(), value);
        self.save_data(session_id, data)
    }

    pub fn remove_key(&self, session_id: &str, key: &str) -> Result<bool> {
        let session_id = validate_session_id(session_id.to_string())?;
        let Some(mut data) = self.load_data(&session_id)? else {
            return Ok(false);
        };
        let removed = data.remove(key).is_some();
        if data.is_empty() {
            self.store.remove(&session_id)?;
        } else {
            self.save_data(session_id, data)?;
        }
        Ok(removed)
    }

    pub fn destroy(&self, session_id: &str) -> Result<bool> {
        self.store
            .remove(&validate_session_id(session_id.to_string())?)
    }

    pub fn clear(&self) -> Result<()> {
        self.store.clear()
    }

    pub fn has_data(&self, session_id: &str) -> Result<bool> {
        Ok(self
            .load_data(&validate_session_id(session_id.to_string())?)?
            .is_some_and(|data| !data.is_empty()))
    }

    fn load_data(&self, session_id: &str) -> Result<Option<BTreeMap<String, Value>>> {
        self.store.load(session_id)
    }

    fn save_data(&self, session_id: String, data: BTreeMap<String, Value>) -> Result<()> {
        if data.is_empty() {
            self.store.remove(&session_id)?;
            return Ok(());
        }
        self.store.save(session_id, data, self.options.ttl)
    }
}

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
                manager.options.request_header_name(),
                session_id,
            )))
        })
    }
}

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

/// Module that registers and exports a [`SessionManager`] provider.
#[derive(Clone)]
pub struct SessionModule {
    name: &'static str,
    token: ProviderToken,
    manager: SessionManager,
    global: bool,
}

impl fmt::Debug for SessionModule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SessionModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("manager", &self.manager)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl SessionModule {
    pub fn in_memory(name: &'static str) -> Self {
        Self::from_manager(name, SessionManager::in_memory(SessionOptions::new()))
    }

    pub fn in_memory_with_options(name: &'static str, options: SessionOptions) -> Self {
        Self::from_manager(name, SessionManager::in_memory(options))
    }

    pub fn from_manager(name: &'static str, manager: SessionManager) -> Self {
        Self {
            name,
            token: ProviderToken::of::<SessionManager>(),
            manager,
            global: false,
        }
    }

    pub fn manager(&self) -> SessionManager {
        self.manager.clone()
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl Module for SessionModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::new(self.manager.clone()),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
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

    if has_data && (!has_cookie || manager.options.rolling) {
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
        options.cookie_name, session_id, options.cookie_path
    );
    if let Some(ttl) = options.ttl {
        cookie.push_str(&format!("; Max-Age={}", ttl.as_secs()));
    }
    append_cookie_attributes(&mut cookie, options);
    cookie
}

fn expired_session_cookie_header(manager: &SessionManager) -> String {
    let options = manager.options();
    let mut cookie = format!(
        "{}=; Path={}; Max-Age=0",
        options.cookie_name, options.cookie_path
    );
    append_cookie_attributes(&mut cookie, options);
    cookie
}

fn append_cookie_attributes(cookie: &mut String, options: &SessionOptions) {
    if let Some(domain) = &options.cookie_domain {
        cookie.push_str(&format!("; Domain={domain}"));
    }
    if options.http_only {
        cookie.push_str("; HttpOnly");
    }
    if options.secure {
        cookie.push_str("; Secure");
    }
    if let Some(same_site) = options.same_site {
        cookie.push_str(&format!("; SameSite={}", same_site.as_str()));
    }
}

fn generate_session_id() -> Result<String> {
    let mut bytes = [0_u8; 32];
    getrandom::getrandom(&mut bytes)
        .map_err(|error| BootError::Internal(format!("failed to generate session id: {error}")))?;
    Ok(bytes
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>())
}

fn validate_session_id(session_id: String) -> Result<String> {
    let session_id = session_id.trim().to_string();
    if session_id.is_empty()
        || session_id.contains(char::is_whitespace)
        || session_id.contains([';', ',', '='])
    {
        return Err(BootError::BadRequest(format!(
            "invalid session id: {session_id:?}"
        )));
    }
    Ok(session_id)
}
