use super::header::{
    accepts_event_stream_response, accepts_json_response, get_header, is_json_media_type,
    matches_media_type, normalize_header_name, normalize_headers, parse_content_length,
    parse_cookie_header_values, strict_content_length_values, validate_header_name,
    validate_header_value,
};
use super::method::HttpMethod;
use super::query::{parse_query, parse_query_pairs, split_path_query};
use crate::percent::validate_percent_encoding;
use crate::{validate_value, BootError, Result, Validate};
use serde::{de::DeserializeOwned, Serialize};
use std::collections::BTreeMap;

/// Framework-neutral HTTP request passed to Boot route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootRequest {
    pub method: HttpMethod,
    pub path: String,
    pub query_string: Option<String>,
    pub query: BTreeMap<String, String>,
    pub params: BTreeMap<String, String>,
    pub headers: BTreeMap<String, String>,
    pub appended_headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl BootRequest {
    pub fn new(method: HttpMethod, path: impl Into<String>) -> Self {
        let (path, query_string, query) = split_path_query(path.into());
        Self {
            method,
            path,
            query_string,
            query,
            params: BTreeMap::new(),
            headers: BTreeMap::new(),
            appended_headers: Vec::new(),
            body: Vec::new(),
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

    pub fn with_path_params(mut self, params: BTreeMap<String, String>) -> Self {
        self.params = params;
        self
    }

    pub fn with_param(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.params.insert(name.into(), value.into());
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

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn query_value(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .query_pairs()?
            .into_iter()
            .find_map(|(key, value)| (key == name).then_some(value)))
    }

    pub fn query_values(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .query_pairs()?
            .into_iter()
            .filter_map(|(key, value)| (key == name).then_some(value))
            .collect())
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
