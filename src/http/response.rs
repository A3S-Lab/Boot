use super::header::{
    get_header, is_json_media_type, matches_media_type, normalize_header_name, normalize_headers,
    parse_content_length, strict_content_length_values, validate_header_name,
    validate_header_value,
};
use crate::{BootError, Result, SseEvent, SseStream};
use futures_core::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::{Arc, Mutex};

/// Framework-neutral HTTP response returned by Boot route handlers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BootResponse {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub appended_headers: Vec<(String, String)>,
    pub body: Vec<u8>,
    stream: Option<SharedSseStream>,
}

#[derive(Clone)]
struct SharedSseStream {
    inner: Arc<Mutex<Option<SseStream>>>,
}

impl SharedSseStream {
    fn new(stream: SseStream) -> Self {
        Self {
            inner: Arc::new(Mutex::new(Some(stream))),
        }
    }

    fn take(&self) -> Option<SseStream> {
        self.inner.lock().ok()?.take()
    }
}

impl fmt::Debug for SharedSseStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SharedSseStream").finish_non_exhaustive()
    }
}

impl PartialEq for SharedSseStream {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.inner, &other.inner)
    }
}

impl Eq for SharedSseStream {}

impl Default for BootResponse {
    fn default() -> Self {
        Self::new(200, Vec::<u8>::new())
    }
}

impl BootResponse {
    pub fn new(status: u16, body: impl Into<Vec<u8>>) -> Self {
        Self {
            status,
            headers: BTreeMap::new(),
            appended_headers: Vec::new(),
            body: body.into(),
            stream: None,
        }
    }

    pub fn status(&self) -> u16 {
        self.status
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn into_body(self) -> Vec<u8> {
        self.body
    }

    pub fn empty(status: u16) -> Self {
        Self::new(status, Vec::<u8>::new())
    }

    pub fn no_content() -> Self {
        Self::empty(204)
    }

    pub fn redirect(location: impl Into<String>) -> Self {
        Self::redirect_with_status(302, location)
    }

    pub fn see_other(location: impl Into<String>) -> Self {
        Self::redirect_with_status(303, location)
    }

    pub fn temporary_redirect(location: impl Into<String>) -> Self {
        Self::redirect_with_status(307, location)
    }

    pub fn permanent_redirect(location: impl Into<String>) -> Self {
        Self::redirect_with_status(308, location)
    }

    pub fn redirect_with_status(status: u16, location: impl Into<String>) -> Self {
        Self::empty(status).with_location(location)
    }

    pub fn text(body: impl Into<String>) -> Self {
        Self::text_with_status(200, body)
    }

    pub fn text_with_status(status: u16, body: impl Into<String>) -> Self {
        Self::new(status, body.into()).with_header("content-type", "text/plain; charset=utf-8")
    }

    pub fn json<T>(body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        Self::json_with_status(200, body)
    }

    pub fn json_with_status<T>(status: u16, body: &T) -> Result<Self>
    where
        T: Serialize,
    {
        let body = serde_json::to_vec(body).map_err(|err| BootError::Internal(err.to_string()))?;
        Ok(Self::new(status, body).with_header("content-type", "application/json"))
    }

    pub fn sse<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<SseEvent>> + Send + 'static,
    {
        Self::empty(200)
            .with_header("content-type", "text/event-stream; charset=utf-8")
            .with_header("cache-control", "no-cache")
            .with_header("connection", "keep-alive")
            .with_sse_stream(stream)
    }

    pub fn from_error(error: &BootError) -> Self {
        Self::text_with_status(error.http_status_code(), error.http_response_message())
    }

    pub fn body_text(&self) -> Result<String> {
        if self.is_streaming() {
            return Err(BootError::Internal(
                "streaming response body cannot be read as text".to_string(),
            ));
        }
        String::from_utf8(self.body.clone()).map_err(|err| BootError::Internal(err.to_string()))
    }

    pub fn body_json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        if self.is_streaming() {
            return Err(BootError::Internal(
                "streaming response body cannot be read as JSON".to_string(),
            ));
        }
        serde_json::from_slice(&self.body).map_err(|err| BootError::Internal(err.to_string()))
    }

    pub fn with_status(mut self, status: u16) -> Self {
        self.status = status;
        self
    }

    pub fn with_content_type(self, content_type: impl Into<String>) -> Self {
        self.with_header("content-type", content_type)
    }

    pub fn with_content_length(self, content_length: u64) -> Self {
        self.with_header("content-length", content_length.to_string())
    }

    pub fn with_location(self, location: impl Into<String>) -> Self {
        self.with_header("location", location)
    }

    pub fn with_www_authenticate(self, challenge: impl Into<String>) -> Self {
        self.with_header("www-authenticate", challenge)
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

    pub fn append_www_authenticate(self, challenge: impl Into<String>) -> Self {
        self.append_header("www-authenticate", challenge)
    }

    pub fn is_streaming(&self) -> bool {
        self.stream.is_some()
    }

    pub fn is_event_stream(&self) -> bool {
        self.is_content_type("text/event-stream")
    }

    pub fn into_sse_stream(self) -> Option<SseStream> {
        self.stream.and_then(|stream| stream.take())
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
            validate_response_header(name, value)?;
        }

        Ok(())
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

    pub fn content_type(&self) -> Option<&str> {
        self.header_values("content-type").into_iter().next()
    }

    pub fn location(&self) -> Option<&str> {
        self.header_values("location").into_iter().next()
    }

    pub fn www_authenticate(&self) -> Option<&str> {
        self.header_values("www-authenticate").into_iter().next()
    }

    pub fn www_authenticate_values(&self) -> Vec<&str> {
        self.header_values("www-authenticate")
    }

    pub fn content_length(&self) -> Result<Option<u64>> {
        let Some(content_length) = self.header_values("content-length").into_iter().next() else {
            return Ok(None);
        };

        parse_content_length(content_length)
            .map(Some)
            .ok_or_else(|| {
                BootError::Internal(format!("invalid content-length header: {content_length}"))
            })
    }

    pub fn strict_content_length(&self) -> Result<Option<u64>> {
        strict_content_length_values(
            self.header_values("content-length"),
            |content_length| {
                BootError::Internal(format!(
                    "invalid response content-length header: {content_length}"
                ))
            },
            |expected_content_length, content_length| {
                BootError::Internal(format!(
                    "conflicting response content-length headers: {expected_content_length} != {content_length}"
                ))
            },
        )
    }

    pub fn validate_content_length(&self) -> Result<()> {
        let Some(content_length) = self.strict_content_length()? else {
            return Ok(());
        };
        if self.is_streaming() {
            return Err(BootError::Internal(
                "streaming responses must not include a content-length header".to_string(),
            ));
        }

        let actual_body_length = self.body.len() as u64;
        if actual_body_length == content_length {
            return Ok(());
        }

        Err(BootError::Internal(format!(
            "response content-length header does not match response body length: expected {content_length}, got {actual_body_length}"
        )))
    }

    pub fn is_content_type(&self, media_type: &str) -> bool {
        self.content_type()
            .is_some_and(|content_type| matches_media_type(content_type, media_type))
    }

    pub fn is_json_content_type(&self) -> bool {
        self.content_type().is_some_and(is_json_media_type)
    }

    pub fn has_body(&self) -> bool {
        self.is_streaming() || !self.body.is_empty()
    }

    pub fn allows_body(&self) -> bool {
        !(self.is_informational() || self.status == 204 || self.status == 304)
    }

    pub fn validate_body_allowed(&self) -> Result<()> {
        if !self.has_body() || self.allows_body() {
            return Ok(());
        }

        Err(BootError::Internal(format!(
            "response status {} must not include a body",
            self.status
        )))
    }

    pub fn is_informational(&self) -> bool {
        (100..200).contains(&self.status)
    }

    pub fn is_success(&self) -> bool {
        (200..300).contains(&self.status)
    }

    pub fn is_redirection(&self) -> bool {
        (300..400).contains(&self.status)
    }

    pub fn is_client_error(&self) -> bool {
        (400..500).contains(&self.status)
    }

    pub fn is_server_error(&self) -> bool {
        (500..600).contains(&self.status)
    }

    pub fn is_error(&self) -> bool {
        self.is_client_error() || self.is_server_error()
    }

    pub fn is_valid_status(&self) -> bool {
        (100..1000).contains(&self.status)
    }

    pub fn validate_status(&self) -> Result<()> {
        if self.is_valid_status() {
            return Ok(());
        }

        Err(BootError::Internal(format!(
            "invalid response status {}",
            self.status
        )))
    }

    pub fn validate(&self) -> Result<()> {
        self.validate_status()?;
        self.validate_content_length()?;
        self.validate_body_allowed()?;
        self.validate_headers()
    }

    fn with_sse_stream<S>(mut self, stream: S) -> Self
    where
        S: Stream<Item = Result<SseEvent>> + Send + 'static,
    {
        self.stream = Some(SharedSseStream::new(Box::pin(stream)));
        self
    }
}

fn validate_response_header(name: &str, value: &str) -> Result<()> {
    validate_header_name(name).map_err(|message| {
        BootError::Internal(format!("invalid response header name {name:?}: {message}"))
    })?;
    validate_header_value(value).map_err(|message| {
        BootError::Internal(format!(
            "invalid response header value for {name:?}: {message}"
        ))
    })
}
