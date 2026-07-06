use super::method::HttpMethod;
use super::query::{parse_query, split_path_query};
use crate::{BootError, Result};
use serde::de::DeserializeOwned;
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
            body: Vec::new(),
        }
    }

    pub fn with_query_string(mut self, query_string: impl Into<String>) -> Self {
        let query_string = query_string.into();
        self.query = parse_query(&query_string);
        self.query_string = Some(query_string);
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

    pub fn with_header(mut self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.headers.insert(name.into(), value.into());
        self
    }

    pub fn with_headers(mut self, headers: BTreeMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.body.clone()).map_err(|err| BootError::Adapter(err.to_string()))
    }

    pub fn param(&self, name: &str) -> Option<&str> {
        self.params.get(name).map(String::as_str)
    }

    pub fn query_param(&self, name: &str) -> Option<&str> {
        self.query.get(name).map(String::as_str)
    }

    pub fn json<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        serde_json::from_slice(&self.body).map_err(|err| BootError::BadRequest(err.to_string()))
    }

    pub fn query<T>(&self) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let query = match self.query_string.as_deref() {
            Some(query) => query.to_string(),
            None => serde_urlencoded::to_string(&self.query)
                .map_err(|err| BootError::BadRequest(err.to_string()))?,
        };
        serde_urlencoded::from_str(&query).map_err(|err| BootError::BadRequest(err.to_string()))
    }
}
