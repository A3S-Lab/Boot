use crate::{HttpMethod, RouteDefinition};
use serde::Serialize;
use serde_json::{json, Value};
use std::collections::{BTreeMap, BTreeSet};

/// Basic OpenAPI document information.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiInfo {
    pub title: String,
    pub version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl OpenApiInfo {
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// OpenAPI document generated from resolved Boot routes.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiDocument {
    pub openapi: String,
    pub info: OpenApiInfo,
    pub paths: BTreeMap<String, OpenApiPathItem>,
    #[serde(skip_serializing_if = "OpenApiComponents::is_empty")]
    pub components: OpenApiComponents,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<OpenApiTag>,
}

impl OpenApiDocument {
    pub fn from_routes(info: OpenApiInfo, routes: &[RouteDefinition]) -> Self {
        let mut tag_names = BTreeSet::new();
        let mut paths = BTreeMap::<String, OpenApiPathItem>::new();
        let mut components = OpenApiComponents::default();

        for route in routes {
            if route.openapi().hidden {
                continue;
            }

            let operation = OpenApiOperation::from_route(route);
            for tag in &operation.tags {
                tag_names.insert(tag.clone());
            }
            components.merge_schemas(route.openapi().schema_components.clone());

            let path = paths.entry(openapi_route_path(route.path())).or_default();
            for method in openapi_methods(route.method()) {
                if route.method().is_wildcard() {
                    path.operations
                        .entry(method.to_string())
                        .or_insert_with(|| operation.clone());
                } else {
                    path.operations
                        .insert(method.to_string(), operation.clone());
                }
            }
        }

        Self {
            openapi: "3.0.3".to_string(),
            info,
            paths,
            components,
            tags: tag_names
                .into_iter()
                .map(|name| OpenApiTag { name })
                .collect(),
        }
    }
}

fn openapi_route_path(path: &str) -> String {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return "/".to_string();
    }

    let segments = path
        .split('/')
        .map(|segment| {
            segment
                .strip_prefix("{*")
                .and_then(|name| name.strip_suffix('}'))
                .map(|name| format!("{{{name}}}"))
                .unwrap_or_else(|| segment.to_string())
        })
        .collect::<Vec<_>>();

    format!("/{}", segments.join("/"))
}

/// OpenAPI components generated or registered by routes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenApiComponents {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, OpenApiSchema>,
}

impl OpenApiComponents {
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
    }

    pub fn merge_schemas(&mut self, schemas: BTreeMap<String, OpenApiSchema>) {
        self.schemas.extend(schemas);
    }
}

/// OpenAPI path item containing operations keyed by HTTP method.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenApiPathItem {
    #[serde(flatten)]
    pub operations: BTreeMap<String, OpenApiOperation>,
}

/// OpenAPI operation metadata for one Boot route.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiOperation {
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "operationId", skip_serializing_if = "Option::is_none")]
    pub operation_id: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub parameters: Vec<OpenApiParameter>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<OpenApiRequestBody>,
    pub responses: BTreeMap<String, OpenApiResponse>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub security: Vec<OpenApiSecurityRequirement>,
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
}

impl OpenApiOperation {
    fn from_route(route: &RouteDefinition) -> Self {
        let metadata = route.openapi();
        let mut parameters = metadata.parameters.clone();

        for name in route.path_param_names() {
            if !parameters.iter().any(|parameter| {
                parameter.location == OpenApiParameterLocation::Path && parameter.name == name
            }) {
                parameters.push(OpenApiParameter::path(name, OpenApiSchema::string()));
            }
        }

        let mut responses = metadata.responses.clone();
        if responses.is_empty() {
            responses.insert("200".to_string(), OpenApiResponse::description("Success"));
        }

        Self {
            tags: metadata.tags.clone(),
            summary: metadata.summary.clone(),
            description: metadata.description.clone(),
            operation_id: metadata.operation_id.clone(),
            parameters,
            request_body: metadata.request_body.clone(),
            responses,
            security: metadata.security.clone(),
            deprecated: metadata.deprecated,
        }
    }
}

/// Route-level OpenAPI metadata carried by [`RouteDefinition`].
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OpenApiRouteMetadata {
    pub tags: Vec<String>,
    pub operation_id: Option<String>,
    pub summary: Option<String>,
    pub description: Option<String>,
    pub parameters: Vec<OpenApiParameter>,
    pub request_body: Option<OpenApiRequestBody>,
    pub responses: BTreeMap<String, OpenApiResponse>,
    pub schema_components: BTreeMap<String, OpenApiSchema>,
    pub security: Vec<OpenApiSecurityRequirement>,
    pub deprecated: bool,
    pub hidden: bool,
}

/// OpenAPI tag entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiTag {
    pub name: String,
}

/// OpenAPI parameter location.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum OpenApiParameterLocation {
    Path,
    Query,
    Header,
    Cookie,
}

/// OpenAPI parameter metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiParameter {
    pub name: String,
    #[serde(rename = "in")]
    pub location: OpenApiParameterLocation,
    pub required: bool,
    pub schema: OpenApiSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl OpenApiParameter {
    pub fn new(
        location: OpenApiParameterLocation,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        Self {
            name: name.into(),
            location,
            required,
            schema,
            description: None,
        }
    }

    pub fn path(name: impl Into<String>, schema: OpenApiSchema) -> Self {
        Self::new(OpenApiParameterLocation::Path, name, true, schema)
    }

    pub fn query(name: impl Into<String>, required: bool, schema: OpenApiSchema) -> Self {
        Self::new(OpenApiParameterLocation::Query, name, required, schema)
    }

    pub fn header(name: impl Into<String>, required: bool, schema: OpenApiSchema) -> Self {
        Self::new(OpenApiParameterLocation::Header, name, required, schema)
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// OpenAPI request body metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiRequestBody {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub required: bool,
    pub content: BTreeMap<String, OpenApiMediaType>,
}

impl OpenApiRequestBody {
    pub fn json(schema: OpenApiSchema) -> Self {
        let mut content = BTreeMap::new();
        content.insert(
            "application/json".to_string(),
            OpenApiMediaType::new(schema),
        );
        Self {
            description: None,
            required: true,
            content,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }
}

/// OpenAPI response metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiResponse {
    pub description: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub content: BTreeMap<String, OpenApiMediaType>,
}

impl OpenApiResponse {
    pub fn description(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            content: BTreeMap::new(),
        }
    }

    pub fn json(description: impl Into<String>, schema: OpenApiSchema) -> Self {
        let mut content = BTreeMap::new();
        content.insert(
            "application/json".to_string(),
            OpenApiMediaType::new(schema),
        );
        Self {
            description: description.into(),
            content,
        }
    }
}

/// OpenAPI media type metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiMediaType {
    pub schema: OpenApiSchema,
}

impl OpenApiMediaType {
    pub fn new(schema: OpenApiSchema) -> Self {
        Self { schema }
    }
}

/// OpenAPI schema value. This intentionally stays schema-crate neutral.
#[derive(Clone, Debug, PartialEq)]
pub struct OpenApiSchema(Value);

impl OpenApiSchema {
    pub fn from_value(value: Value) -> Self {
        Self(value)
    }

    #[cfg(feature = "openapi-schemas")]
    pub fn json_schema<T>() -> std::result::Result<Self, serde_json::Error>
    where
        T: schemars::JsonSchema,
    {
        let mut value = serde_json::to_value(schemars::schema_for!(T))?;
        if let Value::Object(schema) = &mut value {
            schema.remove("$schema");
        }
        Ok(Self(value))
    }

    pub fn string() -> Self {
        Self(json!({ "type": "string" }))
    }

    pub fn integer() -> Self {
        Self(json!({ "type": "integer" }))
    }

    pub fn number() -> Self {
        Self(json!({ "type": "number" }))
    }

    pub fn boolean() -> Self {
        Self(json!({ "type": "boolean" }))
    }

    pub fn object() -> Self {
        Self(json!({ "type": "object" }))
    }

    pub fn array(items: OpenApiSchema) -> Self {
        Self(json!({ "type": "array", "items": items.0 }))
    }

    pub fn reference(name: impl AsRef<str>) -> Self {
        Self(json!({ "$ref": format!("#/components/schemas/{}", name.as_ref()) }))
    }

    pub fn into_value(self) -> Value {
        self.0
    }
}

pub fn openapi_schema_name<T>() -> String {
    std::any::type_name::<T>()
        .rsplit("::")
        .next()
        .unwrap_or("Schema")
        .to_string()
}

impl Serialize for OpenApiSchema {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.0.serialize(serializer)
    }
}

/// OpenAPI security requirement object.
pub type OpenApiSecurityRequirement = BTreeMap<String, Vec<String>>;

fn openapi_methods(method: HttpMethod) -> &'static [&'static str] {
    match method {
        HttpMethod::All => &["get", "post", "put", "patch", "delete", "options", "head"],
        HttpMethod::Get => &["get"],
        HttpMethod::Post => &["post"],
        HttpMethod::Put => &["put"],
        HttpMethod::Patch => &["patch"],
        HttpMethod::Delete => &["delete"],
        HttpMethod::Options => &["options"],
        HttpMethod::Head => &["head"],
    }
}

fn is_false(value: &bool) -> bool {
    !*value
}
