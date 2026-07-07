use crate::BootRequest;
use serde_json::Value;
use std::fmt;

/// Application-wide API version extraction strategy.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiVersioningStrategy {
    /// Extract versions from path segments such as `/v1/cats`.
    Uri { prefix: String },
    /// Extract versions from a request header such as `x-api-version: 1`.
    Header { header: String },
    /// Extract versions from media type parameters such as `Accept: application/json;v=1`.
    MediaType { key: String },
}

/// Adapter-neutral API versioning configuration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApiVersioning {
    strategy: ApiVersioningStrategy,
    default_version: Option<String>,
}

impl ApiVersioning {
    pub fn uri() -> Self {
        Self::uri_with_prefix("v")
    }

    pub fn uri_with_prefix(prefix: impl Into<String>) -> Self {
        Self::new(ApiVersioningStrategy::Uri {
            prefix: prefix.into(),
        })
    }

    pub fn header(header: impl Into<String>) -> Self {
        Self::new(ApiVersioningStrategy::Header {
            header: header.into(),
        })
    }

    pub fn media_type() -> Self {
        Self::media_type_with_key("v")
    }

    pub fn media_type_with_key(key: impl Into<String>) -> Self {
        Self::new(ApiVersioningStrategy::MediaType { key: key.into() })
    }

    pub fn new(strategy: ApiVersioningStrategy) -> Self {
        Self {
            strategy,
            default_version: None,
        }
    }

    pub fn with_default_version(mut self, version: impl Into<String>) -> Self {
        self.default_version = Some(normalize_version(version));
        self
    }

    pub fn strategy(&self) -> &ApiVersioningStrategy {
        &self.strategy
    }

    pub fn default_version(&self) -> Option<&str> {
        self.default_version.as_deref()
    }

    pub(crate) fn request_candidates(&self, request: &BootRequest) -> Vec<ApiVersionCandidate> {
        match &self.strategy {
            ApiVersioningStrategy::Uri { prefix } => uri_candidates(request.path(), prefix),
            ApiVersioningStrategy::Header { header } => vec![ApiVersionCandidate {
                path: request.path().to_string(),
                version: request
                    .header(header)
                    .map(normalize_version)
                    .filter(|version| !version.is_empty()),
            }],
            ApiVersioningStrategy::MediaType { key } => vec![ApiVersionCandidate {
                path: request.path().to_string(),
                version: media_type_version(request.header_values("accept"), key),
            }],
        }
    }

    pub(crate) fn path_candidates(&self, path: &str) -> Vec<ApiVersionCandidate> {
        match &self.strategy {
            ApiVersioningStrategy::Uri { prefix } => uri_candidates(path, prefix),
            ApiVersioningStrategy::Header { .. } | ApiVersioningStrategy::MediaType { .. } => {
                vec![ApiVersionCandidate {
                    path: path.to_string(),
                    version: None,
                }]
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApiVersionCandidate {
    pub path: String,
    pub version: Option<String>,
}

/// Version metadata attached to a route or controller.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum RouteVersioning {
    #[default]
    Unspecified,
    Versions(Vec<String>),
    Neutral,
}

impl RouteVersioning {
    pub fn versions<I, V>(versions: I) -> Self
    where
        I: IntoIterator<Item = V>,
        V: Into<String>,
    {
        let mut values = Vec::new();
        for version in versions {
            let version = normalize_version(version);
            if !version.is_empty() && !values.contains(&version) {
                values.push(version);
            }
        }
        Self::Versions(values)
    }

    pub fn version(version: impl Into<String>) -> Self {
        Self::versions([version])
    }

    pub fn neutral() -> Self {
        Self::Neutral
    }

    pub fn is_unspecified(&self) -> bool {
        matches!(self, Self::Unspecified)
    }

    pub fn matches(&self, requested: Option<&str>, default_version: Option<&str>) -> bool {
        match self {
            Self::Neutral => true,
            Self::Unspecified => match (requested, default_version) {
                (Some(requested), Some(default)) => requested == default,
                (Some(_), None) => false,
                (None, _) => true,
            },
            Self::Versions(versions) => {
                let requested = requested.or(default_version);
                requested
                    .map(|requested| versions.iter().any(|version| version == requested))
                    .unwrap_or(false)
            }
        }
    }

    pub fn overlaps(&self, other: &Self, default_version: Option<&str>) -> bool {
        match (self, other) {
            (Self::Neutral, _) | (_, Self::Neutral) => true,
            (Self::Unspecified, Self::Unspecified) => true,
            (Self::Unspecified, Self::Versions(versions))
            | (Self::Versions(versions), Self::Unspecified) => default_version
                .map(|default| versions.iter().any(|version| version == default))
                .unwrap_or(false),
            (Self::Versions(left), Self::Versions(right)) => {
                left.iter().any(|version| right.contains(version))
            }
        }
    }
}

impl fmt::Display for RouteVersioning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unspecified => f.write_str("unspecified"),
            Self::Neutral => f.write_str("neutral"),
            Self::Versions(versions) => f.write_str(&versions.join(",")),
        }
    }
}

fn uri_candidates(path: &str, prefix: &str) -> Vec<ApiVersionCandidate> {
    let segments = path
        .trim_start_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>();

    for (index, segment) in segments.iter().enumerate() {
        let Some(version) = segment.strip_prefix(prefix) else {
            continue;
        };
        if version.is_empty() {
            continue;
        }

        let mut remaining = segments.clone();
        remaining.remove(index);
        let path = if remaining.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", remaining.join("/"))
        };

        return vec![ApiVersionCandidate {
            path,
            version: Some(normalize_version(version)),
        }];
    }

    vec![ApiVersionCandidate {
        path: path.to_string(),
        version: None,
    }]
}

fn media_type_version(values: Vec<&str>, key: &str) -> Option<String> {
    for value in values {
        for media_range in value.split(',') {
            for parameter in media_range.split(';').skip(1) {
                let Some((name, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if name.trim().eq_ignore_ascii_case(key) {
                    let version = normalize_version(value.trim().trim_matches('"'));
                    if !version.is_empty() {
                        return Some(version);
                    }
                }
            }
        }
    }
    None
}

fn normalize_version(version: impl Into<String>) -> String {
    let version = version.into();
    match serde_json::from_str::<Value>(&version) {
        Ok(Value::Number(number)) => number.to_string(),
        _ => version.trim().to_string(),
    }
}
