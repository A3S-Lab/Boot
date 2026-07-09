use serde::Serialize;
use std::collections::BTreeMap;

/// OpenAPI security requirement object.
pub type OpenApiSecurityRequirement = BTreeMap<String, Vec<String>>;

/// OpenAPI security scheme metadata registered by routes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiSecurityScheme {
    #[serde(rename = "type")]
    pub ty: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scheme: Option<String>,
    #[serde(rename = "bearerFormat", skip_serializing_if = "Option::is_none")]
    pub bearer_format: Option<String>,
    #[serde(rename = "in", skip_serializing_if = "Option::is_none")]
    pub location: Option<OpenApiApiKeyLocation>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl OpenApiSecurityScheme {
    pub fn http_bearer() -> Self {
        Self {
            ty: "http".to_string(),
            scheme: Some("bearer".to_string()),
            bearer_format: Some("JWT".to_string()),
            location: None,
            name: None,
            description: None,
        }
    }

    pub fn api_key(location: OpenApiApiKeyLocation, name: impl Into<String>) -> Self {
        Self {
            ty: "apiKey".to_string(),
            scheme: None,
            bearer_format: None,
            location: Some(location),
            name: Some(name.into()),
            description: None,
        }
    }

    pub fn api_key_header(name: impl Into<String>) -> Self {
        Self::api_key(OpenApiApiKeyLocation::Header, name)
    }

    pub fn api_key_query(name: impl Into<String>) -> Self {
        Self::api_key(OpenApiApiKeyLocation::Query, name)
    }

    pub fn api_key_cookie(name: impl Into<String>) -> Self {
        Self::api_key(OpenApiApiKeyLocation::Cookie, name)
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// Location for OpenAPI API key security schemes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenApiApiKeyLocation {
    Query,
    Header,
    Cookie,
}
