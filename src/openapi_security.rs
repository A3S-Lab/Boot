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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub flows: Option<OpenApiOAuthFlows>,
    #[serde(rename = "openIdConnectUrl", skip_serializing_if = "Option::is_none")]
    pub open_id_connect_url: Option<String>,
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
            flows: None,
            open_id_connect_url: None,
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
            flows: None,
            open_id_connect_url: None,
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

    pub fn oauth2(flows: OpenApiOAuthFlows) -> Self {
        Self {
            ty: "oauth2".to_string(),
            scheme: None,
            bearer_format: None,
            location: None,
            name: None,
            description: None,
            flows: Some(flows),
            open_id_connect_url: None,
        }
    }

    pub fn open_id_connect(url: impl Into<String>) -> Self {
        Self {
            ty: "openIdConnect".to_string(),
            scheme: None,
            bearer_format: None,
            location: None,
            name: None,
            description: None,
            flows: None,
            open_id_connect_url: Some(url.into()),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// OpenAPI OAuth flow map.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct OpenApiOAuthFlows {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub implicit: Option<OpenApiOAuthFlow>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub password: Option<OpenApiOAuthFlow>,
    #[serde(rename = "clientCredentials", skip_serializing_if = "Option::is_none")]
    pub client_credentials: Option<OpenApiOAuthFlow>,
    #[serde(rename = "authorizationCode", skip_serializing_if = "Option::is_none")]
    pub authorization_code: Option<OpenApiOAuthFlow>,
}

impl OpenApiOAuthFlows {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn implicit<I, K, V>(authorization_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self::new().with_implicit(OpenApiOAuthFlow::implicit(authorization_url, scopes))
    }

    pub fn password<I, K, V>(token_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self::new().with_password(OpenApiOAuthFlow::password(token_url, scopes))
    }

    pub fn client_credentials<I, K, V>(token_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self::new().with_client_credentials(OpenApiOAuthFlow::client_credentials(token_url, scopes))
    }

    pub fn authorization_code<I, K, V>(
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        scopes: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self::new().with_authorization_code(OpenApiOAuthFlow::authorization_code(
            authorization_url,
            token_url,
            scopes,
        ))
    }

    pub fn with_implicit(mut self, flow: OpenApiOAuthFlow) -> Self {
        self.implicit = Some(flow);
        self
    }

    pub fn with_password(mut self, flow: OpenApiOAuthFlow) -> Self {
        self.password = Some(flow);
        self
    }

    pub fn with_client_credentials(mut self, flow: OpenApiOAuthFlow) -> Self {
        self.client_credentials = Some(flow);
        self
    }

    pub fn with_authorization_code(mut self, flow: OpenApiOAuthFlow) -> Self {
        self.authorization_code = Some(flow);
        self
    }
}

/// OpenAPI OAuth flow metadata.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiOAuthFlow {
    #[serde(rename = "authorizationUrl", skip_serializing_if = "Option::is_none")]
    pub authorization_url: Option<String>,
    #[serde(rename = "tokenUrl", skip_serializing_if = "Option::is_none")]
    pub token_url: Option<String>,
    #[serde(rename = "refreshUrl", skip_serializing_if = "Option::is_none")]
    pub refresh_url: Option<String>,
    pub scopes: BTreeMap<String, String>,
}

impl OpenApiOAuthFlow {
    pub fn implicit<I, K, V>(authorization_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            authorization_url: Some(authorization_url.into()),
            token_url: None,
            refresh_url: None,
            scopes: oauth_scopes(scopes),
        }
    }

    pub fn password<I, K, V>(token_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            authorization_url: None,
            token_url: Some(token_url.into()),
            refresh_url: None,
            scopes: oauth_scopes(scopes),
        }
    }

    pub fn client_credentials<I, K, V>(token_url: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            authorization_url: None,
            token_url: Some(token_url.into()),
            refresh_url: None,
            scopes: oauth_scopes(scopes),
        }
    }

    pub fn authorization_code<I, K, V>(
        authorization_url: impl Into<String>,
        token_url: impl Into<String>,
        scopes: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            authorization_url: Some(authorization_url.into()),
            token_url: Some(token_url.into()),
            refresh_url: None,
            scopes: oauth_scopes(scopes),
        }
    }

    pub fn with_refresh_url(mut self, refresh_url: impl Into<String>) -> Self {
        self.refresh_url = Some(refresh_url.into());
        self
    }
}

fn oauth_scopes<I, K, V>(scopes: I) -> BTreeMap<String, String>
where
    I: IntoIterator<Item = (K, V)>,
    K: Into<String>,
    V: Into<String>,
{
    scopes
        .into_iter()
        .map(|(scope, description)| (scope.into(), description.into()))
        .collect()
}

/// Location for OpenAPI API key security schemes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenApiApiKeyLocation {
    Query,
    Header,
    Cookie,
}
