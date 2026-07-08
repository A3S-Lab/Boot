use super::header::{
    accepts_event_stream_response, accepts_json_response, get_header, is_json_media_type,
    matches_media_type, normalize_header_name, normalize_headers, parse_content_length,
    parse_cookie_header_values, strict_content_length_values, validate_header_name,
    validate_header_value,
};
use super::method::HttpMethod;
use super::query::{parse_query, parse_query_pairs, split_path_query};
use crate::percent::validate_percent_encoding;
use crate::routing::host::normalize_host_header;
#[cfg(feature = "auth")]
use crate::AuthPrincipal;
use crate::{validate_value, BootError, ModuleRef, ProviderToken, Result, Validate};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
#[cfg(feature = "auth")]
use std::sync::RwLock;

/// Framework-neutral HTTP request passed to Boot route handlers.
#[derive(Debug, Clone)]
pub struct BootRequest {
    pub method: HttpMethod,
    pub path: String,
    pub query_string: Option<String>,
    pub query: BTreeMap<String, String>,
    pub params: BTreeMap<String, String>,
    pub host_params: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub appended_headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    module_ref: Option<ModuleRef>,
    #[cfg(feature = "auth")]
    auth_principal: Arc<RwLock<Option<AuthPrincipal>>>,
}

impl PartialEq for BootRequest {
    fn eq(&self, other: &Self) -> bool {
        self.method == other.method
            && self.path == other.path
            && self.query_string == other.query_string
            && self.query == other.query
            && self.params == other.params
            && self.host_params == other.host_params
            && self.headers == other.headers
            && self.appended_headers == other.appended_headers
            && self.body == other.body
    }
}

impl Eq for BootRequest {}

impl BootRequest {
    pub fn new(method: HttpMethod, path: impl Into<String>) -> Self {
        let (path, query_string, query) = split_path_query(path.into());
        Self {
            method,
            path,
            query_string,
            query,
            params: BTreeMap::new(),
            host_params: BTreeMap::new(),
            headers: BTreeMap::new(),
            appended_headers: Vec::new(),
            body: Vec::new(),
            module_ref: None,
            #[cfg(feature = "auth")]
            auth_principal: Arc::new(RwLock::new(None)),
        }
    }

    pub fn method(&self) -> HttpMethod {
        self.method
    }

    pub fn path(&self) -> &str {
        &self.path
    }

    pub fn query_string(&self) -> Option<&str> {
        self.query_string.as_deref()
    }

    pub fn with_query_string(mut self, query_string: impl Into<String>) -> Self {
        let query_string = query_string.into();
        self.query = parse_query(&query_string);
        self.query_string = Some(query_string);
        self
    }

    pub(crate) fn with_matched_path(mut self, path: impl Into<String>) -> Self {
        self.path = path.into();
        self
    }

    pub(crate) fn with_module_ref(mut self, module_ref: ModuleRef) -> Self {
        self.module_ref = Some(module_ref);
        self
    }

    pub fn module_ref(&self) -> Option<&ModuleRef> {
        self.module_ref.as_ref()
    }

    pub fn get<T>(&self) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref
            .as_ref()
            .ok_or_else(|| BootError::MissingProvider(ProviderToken::of::<T>().to_string()))?
            .get::<T>()
    }

    pub fn get_named<T>(&self, token: &str) -> Result<Arc<T>>
    where
        T: Send + Sync + 'static,
    {
        self.module_ref
            .as_ref()
            .ok_or_else(|| BootError::MissingProvider(ProviderToken::named(token).to_string()))?
            .get_named::<T>(token)
    }

    pub fn get_optional<T>(&self) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        match &self.module_ref {
            Some(module_ref) => module_ref.get_optional::<T>(),
            None => Ok(None),
        }
    }

    pub fn get_optional_named<T>(&self, token: &str) -> Result<Option<Arc<T>>>
    where
        T: Send + Sync + 'static,
    {
        match &self.module_ref {
            Some(module_ref) => module_ref.get_optional_named::<T>(token),
            None => Ok(None),
        }
    }

    #[cfg(feature = "auth")]
    pub fn with_auth_principal(mut self, principal: AuthPrincipal) -> Self {
        self.auth_principal = Arc::new(RwLock::new(Some(principal)));
        self
    }

    #[cfg(feature = "auth")]
    pub fn set_auth_principal(&self, principal: AuthPrincipal) -> Result<()> {
        *self
            .auth_principal
            .write()
            .map_err(|_| BootError::Internal("auth principal lock is poisoned".to_string()))? =
            Some(principal);
        Ok(())
    }

    #[cfg(feature = "auth")]
    pub fn clear_auth_principal(&self) -> Result<()> {
        *self
            .auth_principal
            .write()
            .map_err(|_| BootError::Internal("auth principal lock is poisoned".to_string()))? =
            None;
        Ok(())
    }

    #[cfg(feature = "auth")]
    pub fn auth_principal(&self) -> Result<Option<AuthPrincipal>> {
        Ok(self
            .auth_principal
            .read()
            .map_err(|_| BootError::Internal("auth principal lock is poisoned".to_string()))?
            .clone())
    }

    #[cfg(feature = "auth")]
    pub fn require_auth_principal(&self) -> Result<AuthPrincipal> {
        self.auth_principal()?
            .ok_or_else(|| BootError::Unauthorized("missing authenticated principal".to_string()))
    }

    #[cfg(feature = "session")]
    pub fn session(&self) -> Result<crate::Session> {
        let manager = self.get::<crate::SessionManager>()?;
        let session_id = manager.require_session_id(self)?;
        crate::Session::from_manager_arc(manager, session_id)
    }

    #[cfg(feature = "session")]
    pub fn optional_session(&self) -> Result<Option<crate::Session>> {
        let Some(manager) = self.get_optional::<crate::SessionManager>()? else {
            return Ok(None);
        };
        let Some(session_id) = manager.session_id(self)? else {
            return Ok(None);
        };
        crate::Session::from_manager_arc(manager, session_id).map(Some)
    }

    pub fn with_path_params(mut self, params: BTreeMap<String, String>) -> Self {
        self.params = params;
        self
    }

    pub fn with_host_params(mut self, params: BTreeMap<String, String>) -> Self {
        self.host_params = params;
        self
    }

    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(name.into(), value.into());
        self
    }

    pub fn with_host_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.host_params.insert(name.into(), value.into());
        self
    }

    pub fn with_query_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.query.insert(name.into(), value.into());
        self.query_string = None;
        self
    }

    pub fn with_body(mut self, body: impl Into<Vec<u8>>) -> Self {
        self.body = body.into();
        self
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    pub fn with_text(self, body: impl Into<String>) -> Self {
        self.with_body(body.into())
            .with_header("content-type", "text/plain; charset=utf-8")
    }

    pub fn with_json<T>(self, body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let body = serde_json::to_vec(body).map_err(|err| BootError::Internal(err.to_string()))?;
        Ok(self
            .with_body(body)
            .with_header("content-type", "application/json"))
    }

    pub fn with_content_type(self, content_type: impl Into<String>) -> Self {
        self.with_header("content-type", content_type)
    }

    pub fn with_content_length(self, content_length: u64) -> Self {
        self.with_header("content-length", content_length.to_string())
    }

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers
            .insert(normalize_header_name(name), value.into());
        self
    }

    pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = normalize_headers(headers);
        self
    }

    pub fn append_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.appended_headers
            .push((normalize_header_name(name), value.into()));
        self
    }

    pub fn header_entries(&self) -> impl Iterator<Item = (&str, &str)> {
        self.headers
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .chain(
                self.appended_headers
                    .iter()
                    .map(|(name, value)| (name.as_str(), value.as_str())),
            )
    }

    pub fn validate_headers(&self) -> Result<()> {
        for (name, value) in self.header_entries() {
            validate_request_header(name, value)?;
        }

        Ok(())
    }

    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone()).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn param_as<T>(&self, name: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(
            self.param(name).map(ToString::to_string),
            "path parameter",
            name,
        )
    }

    pub fn optional_param_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(
            self.param(name).map(ToString::to_string),
            "path parameter",
            name,
        )
    }

    pub fn host_param(&self, name: &str) -> Option<&str> {
        self.host_params.get(name).map(String::as_str)
    }

    pub fn host_param_as<T>(&self, name: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(
            self.host_param(name).map(ToString::to_string),
            "host parameter",
            name,
        )
    }

    pub fn optional_host_param_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(
            self.host_param(name).map(ToString::to_string),
            "host parameter",
            name,
        )
    }

    pub fn params<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let params = serde_urlencoded::to_string(&self.params)
            .map_err(|err| BootError::BadRequest(err.to_string()))?;
        serde_urlencoded::from_str(&params).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn validated_params<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        let value = self.params()?;
        validate_value(value)
    }

    pub fn host_params<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let params = serde_urlencoded::to_string(&self.host_params)
            .map_err(|err| BootError::BadRequest(err.to_string()))?;
        serde_urlencoded::from_str(&params).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn validated_host_params<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        let value = self.host_params()?;
        validate_value(value)
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn query_value(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .query_pairs()?
            .into_iter()
            .find_map(|(key, value)| (key == name).then_some(value)))
    }

    pub fn query_value_as<T>(&self, name: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(self.query_value(name)?, "query parameter", name)
    }

    pub fn optional_query_value_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(self.query_value(name)?, "query parameter", name)
    }

    pub fn query_values(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .query_pairs()?
            .into_iter()
            .filter_map(|(key, value)| (key == name).then_some(value))
            .collect())
    }

    pub fn query_values_as<T>(&self, name: &str) -> Result<Vec<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        self.query_values(name)?
            .into_iter()
            .map(|value| parse_value(value, "query parameter", name))
            .collect()
    }

    pub fn query_pairs(&self) -> Result<Vec<(String, String)>> {
        match self.query_string.as_deref() {
            Some(query) => parse_query_pairs(query),
            None => Ok(self
                .query
                .iter()
                .map(|(key, value)| (key.clone(), value.clone()))
                .collect()),
        }
    }

    pub fn header(&self, name: &str) -> Option<&str> {
        get_header(&self.headers, name)
    }

    pub fn header_as<T>(&self, name: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(self.header(name).map(ToString::to_string), "header", name)
    }

    pub fn optional_header_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(self.header(name).map(ToString::to_string), "header", name)
    }

    pub fn host(&self) -> Option<&str> {
        self.header("host").and_then(normalize_host_header)
    }

    pub fn ip(&self) -> Option<String> {
        self.forwarded_for_ip()
            .or_else(|| self.forwarded_header_ip("x-forwarded-for"))
            .or_else(|| self.forwarded_header_ip("x-real-ip"))
    }

    pub fn ip_as<T>(&self) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(self.ip(), "IP address", "ip")
    }

    pub fn optional_ip_as<T>(&self) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(self.ip(), "IP address", "ip")
    }

    pub fn header_values(&self, name: &str) -> Vec<&str> {
        let mut values = self.header(name).into_iter().collect::<Vec<_>>();
        values.extend(
            self.appended_headers
                .iter()
                .filter(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value.as_str()),
        );
        values
    }

    pub fn authorization(&self) -> Option<&str> {
        self.header_values("authorization").into_iter().next()
    }

    fn forwarded_header_ip(&self, name: &str) -> Option<String> {
        self.header_values(name)
            .into_iter()
            .flat_map(|value| value.split(','))
            .map(str::trim)
            .find(|value| !value.is_empty())
            .map(ToString::to_string)
    }

    fn forwarded_for_ip(&self) -> Option<String> {
        self.header_values("forwarded")
            .into_iter()
            .flat_map(|value| value.split(','))
            .flat_map(|entry| entry.split(';'))
            .filter_map(|part| part.trim().split_once('='))
            .find_map(|(key, value)| {
                key.trim().eq_ignore_ascii_case("for").then(|| {
                    value
                        .trim()
                        .trim_matches('"')
                        .trim_matches(['[', ']'])
                        .to_string()
                })
            })
            .filter(|value| !value.is_empty())
    }

    pub fn bearer_token(&self) -> Option<&str> {
        let authorization = self.authorization()?.trim();
        let mut parts = authorization.splitn(2, char::is_whitespace);
        let scheme = parts.next()?;
        let token = parts.next()?.trim();

        if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
            Some(token)
        } else {
            None
        }
    }

    pub fn require_bearer_token(&self) -> Result<&str> {
        self.bearer_token()
            .ok_or_else(|| BootError::Unauthorized("missing bearer token".to_string()))
    }

    pub fn cookie_pairs(&self) -> Result<Vec<(String, String)>> {
        parse_cookie_header_values(&self.header_values("cookie"))
    }

    pub fn cookie(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .cookie_pairs()?
            .into_iter()
            .find_map(|(key, value)| (key == name).then_some(value)))
    }

    pub fn require_cookie(&self, name: &str) -> Result<String> {
        self.cookie(name)?
            .ok_or_else(|| BootError::Unauthorized(format!("missing cookie: {name}")))
    }

    pub fn cookie_values(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .cookie_pairs()?
            .into_iter()
            .filter_map(|(key, value)| (key == name).then_some(value))
            .collect())
    }

    pub fn cookies(&self) -> Result<BTreeMap<String, String>> {
        let mut cookies = BTreeMap::new();
        for (name, value) in self.cookie_pairs()? {
            cookies.entry(name).or_insert(value);
        }
        Ok(cookies)
    }

    pub fn content_type(&self) -> Option<&str> {
        self.header_values("content-type").into_iter().next()
    }

    pub fn content_length(&self) -> Result<Option<u64>> {
        let Some(content_length) = self.header_values("content-length").into_iter().next() else {
            return Ok(None);
        };

        parse_content_length(content_length)
            .map(Some)
            .ok_or_else(|| {
                BootError::BadRequest(format!("invalid content-length header: {content_length}"))
            })
    }

    pub fn strict_content_length(&self) -> Result<Option<u64>> {
        strict_content_length_values(
            self.header_values("content-length"),
            |content_length| {
                BootError::BadRequest(format!("invalid content-length header: {content_length}"))
            },
            |expected_content_length, content_length| {
                BootError::BadRequest(format!(
                    "conflicting content-length headers: {expected_content_length} != {content_length}"
                ))
            },
        )
    }

    pub fn validate_content_length(&self) -> Result<()> {
        let Some(content_length) = self.strict_content_length()? else {
            return Ok(());
        };
        let actual_body_length = self.body.len() as u64;
        if actual_body_length == content_length {
            return Ok(());
        }

        Err(BootError::BadRequest(format!(
            "content-length header does not match request body length: expected {content_length}, got {actual_body_length}"
        )))
    }

    pub fn validate_body_limit(&self, body_limit: usize) -> Result<()> {
        if self
            .strict_content_length()?
            .is_some_and(|content_length| content_length > body_limit as u64)
            || self.body.len() > body_limit
        {
            return Err(BootError::PayloadTooLarge(format!(
                "request body exceeds {body_limit} bytes"
            )));
        }

        Ok(())
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_headers()?;
        self.validate_content_length()
    }

    pub fn validate_with_body_limit(&self, body_limit: usize) -> Result<()> {
        self.validate_headers()?;
        self.validate_body_limit(body_limit)?;
        self.validate_content_length()
    }

    pub fn is_content_type(&self, media_type: &str) -> bool {
        self.content_type()
            .is_some_and(|content_type| matches_media_type(content_type, media_type))
    }

    pub fn is_json_content_type(&self) -> bool {
        self.content_type().is_some_and(is_json_media_type)
    }

    pub fn require_json_content_type(&self) -> Result<()> {
        if self.is_json_content_type() {
            return Ok(());
        }

        let message = match self.content_type() {
            Some(content_type) => format!("expected JSON content type, got {content_type}"),
            None => "expected JSON content type".to_string(),
        };
        Err(BootError::UnsupportedMediaType(message))
    }

    pub fn accepts_json(&self) -> bool {
        accepts_json_response(&self.header_values("accept"))
    }

    pub fn require_accepts_json(&self) -> Result<()> {
        if self.accepts_json() {
            return Ok(());
        }

        Err(BootError::NotAcceptable(
            "expected client to accept JSON response".to_string(),
        ))
    }

    pub fn accepts_event_stream(&self) -> bool {
        accepts_event_stream_response(&self.header_values("accept"))
    }

    pub fn require_accepts_event_stream(&self) -> Result<()> {
        if self.accepts_event_stream() {
            return Ok(());
        }

        Err(BootError::NotAcceptable(
            "expected client to accept text/event-stream response".to_string(),
        ))
    }

    pub fn json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn validated_json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        let value = self.json()?;
        validate_value(value)
    }

    pub fn json_with_content_type<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        self.require_json_content_type()?;
        self.json()
    }

    pub fn validated_json_with_content_type<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        self.require_json_content_type()?;
        self.validated_json()
    }

    pub fn query<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let query = match self.query_string.as_deref() {
            Some(query) => {
                validate_percent_encoding(query)?;
                query.to_string()
            }
            None => serde_urlencoded::to_string(&self.query)
                .map_err(|err| BootError::BadRequest(err.to_string()))?,
        };
        serde_urlencoded::from_str(&query).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn validated_query<T>(&self) -> Result<T>
    where
        T: DeserializeOwned + Validate,
    {
        let value = self.query()?;
        validate_value(value)
    }
}

fn parse_required_value<T>(value: Option<String>, label: &str, name: &str) -> Result<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    let Some(value) = value else {
        return Err(BootError::BadRequest(format!("missing {label}: {name}")));
    };
    parse_value(value, label, name)
}

fn parse_optional_value<T>(value: Option<String>, label: &str, name: &str) -> Result<Option<T>>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    value
        .map(|value| parse_value(value, label, name))
        .transpose()
}

fn parse_value<T>(value: String, label: &str, name: &str) -> Result<T>
where
    T: FromStr,
    T::Err: fmt::Display,
{
    value
        .parse::<T>()
        .map_err(|error| BootError::BadRequest(format!("invalid {label} {name}: {error}")))
}

fn validate_request_header(name: &str, value: &str) -> Result<()> {
    validate_header_name(name).map_err(|message| {
        BootError::BadRequest(format!("invalid request header name {name:?}: {message}"))
    })?;
    validate_header_value(value).map_err(|message| {
        BootError::BadRequest(format!(
            "invalid request header value for {name:?}: {message}"
        ))
    })
}
