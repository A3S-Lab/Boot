use crate::openapi_security::{OpenApiSecurityRequirement, OpenApiSecurityScheme};
use crate::{BootError, HttpMethod, Result, RouteDefinition};
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
    #[serde(skip)]
    pub servers: Vec<OpenApiServer>,
    #[serde(skip)]
    pub external_docs: Option<OpenApiExternalDocs>,
    #[serde(skip)]
    pub tags: Vec<OpenApiTag>,
}

impl OpenApiInfo {
    pub fn new(title: impl Into<String>, version: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            version: version.into(),
            description: None,
            servers: Vec::new(),
            external_docs: None,
            tags: Vec::new(),
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_server(mut self, url: impl Into<String>) -> Self {
        self.servers.push(OpenApiServer::new(url));
        self
    }

    pub fn with_server_description(
        mut self,
        url: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.servers
            .push(OpenApiServer::new(url).with_description(description));
        self
    }

    pub fn with_external_docs(
        mut self,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.external_docs = Some(OpenApiExternalDocs::new(description, url));
        self
    }

    pub fn with_tag_description(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let tag = OpenApiTag::new(name.clone()).with_description(description);
        if let Some(existing) = self.tags.iter_mut().find(|tag| tag.name == name) {
            *existing = tag;
        } else {
            self.tags.push(tag);
        }
        self
    }

    pub fn with_tag_external_docs(
        mut self,
        name: impl Into<String>,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        let name = name.into();
        let external_docs = OpenApiExternalDocs::new(description, url);
        if let Some(existing) = self.tags.iter_mut().find(|tag| tag.name == name) {
            existing.external_docs = Some(external_docs);
        } else {
            self.tags
                .push(OpenApiTag::new(name).with_external_docs_object(external_docs));
        }
        self
    }
}

/// OpenAPI server entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiServer {
    pub url: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl OpenApiServer {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// OpenAPI external documentation link.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiExternalDocs {
    pub description: String,
    pub url: String,
}

impl OpenApiExternalDocs {
    pub fn new(description: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            url: url.into(),
        }
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
    pub servers: Vec<OpenApiServer>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<OpenApiTag>,
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<OpenApiExternalDocs>,
}

impl OpenApiDocument {
    pub fn from_routes(info: OpenApiInfo, routes: &[RouteDefinition]) -> Self {
        let mut tags = info
            .tags
            .iter()
            .map(|tag| (tag.name.clone(), tag.clone()))
            .collect::<BTreeMap<_, _>>();
        let servers = info.servers.clone();
        let external_docs = info.external_docs.clone();
        let mut paths = BTreeMap::<String, OpenApiPathItem>::new();
        let mut components = OpenApiComponents::default();

        for route in routes {
            if route.openapi().hidden {
                continue;
            }

            let operation = OpenApiOperation::from_route(route);
            for tag in &operation.tags {
                tags.entry(tag.clone())
                    .or_insert_with(|| OpenApiTag::new(tag.clone()));
            }
            components.merge_schemas(route.openapi().schema_components.clone());
            components.merge_responses(route.openapi().response_components.clone());
            components.merge_parameters(route.openapi().parameter_components.clone());
            components.merge_examples(route.openapi().example_components.clone());
            components.merge_request_bodies(route.openapi().request_body_components.clone());
            components.merge_headers(route.openapi().header_components.clone());
            components.merge_security_schemes(route.openapi().security_schemes.clone());

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
            servers,
            tags: tags.into_values().collect(),
            external_docs,
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

fn escape_openapi_component_name(name: &str) -> String {
    name.replace('~', "~0").replace('/', "~1")
}

/// OpenAPI components generated or registered by routes.
#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct OpenApiComponents {
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub schemas: BTreeMap<String, OpenApiSchema>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub responses: BTreeMap<String, OpenApiResponse>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub parameters: BTreeMap<String, OpenApiParameter>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub examples: BTreeMap<String, OpenApiExample>,
    #[serde(rename = "requestBodies", skip_serializing_if = "BTreeMap::is_empty")]
    pub request_bodies: BTreeMap<String, OpenApiRequestBody>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, OpenApiHeader>,
    #[serde(rename = "securitySchemes", skip_serializing_if = "BTreeMap::is_empty")]
    pub security_schemes: BTreeMap<String, OpenApiSecurityScheme>,
}

impl OpenApiComponents {
    pub fn is_empty(&self) -> bool {
        self.schemas.is_empty()
            && self.responses.is_empty()
            && self.parameters.is_empty()
            && self.examples.is_empty()
            && self.request_bodies.is_empty()
            && self.headers.is_empty()
            && self.security_schemes.is_empty()
    }

    pub fn merge_schemas(&mut self, schemas: BTreeMap<String, OpenApiSchema>) {
        self.schemas.extend(schemas);
    }

    pub fn merge_responses(&mut self, responses: BTreeMap<String, OpenApiResponse>) {
        self.responses.extend(responses);
    }

    pub fn merge_parameters(&mut self, parameters: BTreeMap<String, OpenApiParameter>) {
        self.parameters.extend(parameters);
    }

    pub fn merge_examples(&mut self, examples: BTreeMap<String, OpenApiExample>) {
        self.examples.extend(examples);
    }

    pub fn merge_request_bodies(&mut self, request_bodies: BTreeMap<String, OpenApiRequestBody>) {
        self.request_bodies.extend(request_bodies);
    }

    pub fn merge_headers(&mut self, headers: BTreeMap<String, OpenApiHeader>) {
        self.headers.extend(headers);
    }

    pub fn merge_security_schemes(
        &mut self,
        security_schemes: BTreeMap<String, OpenApiSecurityScheme>,
    ) {
        self.security_schemes.extend(security_schemes);
    }
}

/// OpenAPI Reference Object.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiRef {
    #[serde(rename = "$ref")]
    pub reference: String,
    #[serde(skip)]
    parameter_location: Option<OpenApiParameterLocation>,
    #[serde(skip)]
    parameter_name: Option<String>,
}

impl OpenApiRef {
    pub fn new(reference: impl Into<String>) -> Self {
        Self {
            reference: reference.into(),
            parameter_location: None,
            parameter_name: None,
        }
    }

    pub fn component(component: impl Into<String>, name: impl AsRef<str>) -> Self {
        Self::new(format!(
            "#/components/{}/{}",
            component.into(),
            escape_openapi_component_name(name.as_ref())
        ))
    }

    pub fn schema(name: impl AsRef<str>) -> Self {
        Self::component("schemas", name)
    }

    pub fn response(name: impl AsRef<str>) -> Self {
        Self::component("responses", name)
    }

    pub fn parameter(name: impl AsRef<str>) -> Self {
        Self::component("parameters", name)
    }

    pub fn example(name: impl AsRef<str>) -> Self {
        Self::component("examples", name)
    }

    pub fn request_body(name: impl AsRef<str>) -> Self {
        Self::component("requestBodies", name)
    }

    pub fn header(name: impl AsRef<str>) -> Self {
        Self::component("headers", name)
    }

    pub fn security_scheme(name: impl AsRef<str>) -> Self {
        Self::component("securitySchemes", name)
    }

    pub fn with_parameter_metadata(
        mut self,
        location: OpenApiParameterLocation,
        name: impl Into<String>,
    ) -> Self {
        self.parameter_location = Some(location);
        self.parameter_name = Some(name.into());
        self
    }

    pub(crate) fn parameter_identity(&self) -> Option<(OpenApiParameterLocation, &str)> {
        Some((self.parameter_location?, self.parameter_name.as_deref()?))
    }
}

/// Either an inline OpenAPI object or a Reference Object.
#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OpenApiReferenceOr<T> {
    Reference(OpenApiRef),
    Value(T),
}

impl<T> OpenApiReferenceOr<T> {
    pub fn reference(reference: OpenApiRef) -> Self {
        Self::Reference(reference)
    }

    pub fn value(value: T) -> Self {
        Self::Value(value)
    }

    pub fn value_mut(&mut self) -> Option<&mut T> {
        match self {
            Self::Reference(_) => None,
            Self::Value(value) => Some(value),
        }
    }
}

impl<T> From<T> for OpenApiReferenceOr<T> {
    fn from(value: T) -> Self {
        Self::value(value)
    }
}

impl OpenApiReferenceOr<OpenApiParameter> {
    pub(crate) fn parameter_identity(&self) -> Option<(OpenApiParameterLocation, &str)> {
        match self {
            Self::Reference(reference) => reference.parameter_identity(),
            Self::Value(parameter) => Some((parameter.location, parameter.name.as_str())),
        }
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
    pub parameters: Vec<OpenApiReferenceOr<OpenApiParameter>>,
    #[serde(rename = "requestBody", skip_serializing_if = "Option::is_none")]
    pub request_body: Option<OpenApiReferenceOr<OpenApiRequestBody>>,
    pub responses: BTreeMap<String, OpenApiReferenceOr<OpenApiResponse>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub servers: Vec<OpenApiServer>,
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<OpenApiExternalDocs>,
    #[serde(flatten)]
    pub extensions: BTreeMap<String, Value>,
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
                parameter
                    .parameter_identity()
                    .is_some_and(|(location, parameter_name)| {
                        location == OpenApiParameterLocation::Path && parameter_name == name
                    })
            }) {
                parameters.push(OpenApiReferenceOr::value(OpenApiParameter::path(
                    name,
                    OpenApiSchema::string(),
                )));
            }
        }

        let mut responses = metadata.responses.clone();
        if responses.is_empty() {
            responses.insert(
                "200".to_string(),
                OpenApiReferenceOr::value(OpenApiResponse::description("Success")),
            );
        }

        Self {
            tags: metadata.tags.clone(),
            summary: metadata.summary.clone(),
            description: metadata.description.clone(),
            operation_id: metadata.operation_id.clone(),
            parameters,
            request_body: metadata.request_body.clone(),
            responses,
            servers: metadata.servers.clone(),
            external_docs: metadata.external_docs.clone(),
            extensions: metadata.extensions.clone(),
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
    pub parameters: Vec<OpenApiReferenceOr<OpenApiParameter>>,
    pub request_body: Option<OpenApiReferenceOr<OpenApiRequestBody>>,
    pub responses: BTreeMap<String, OpenApiReferenceOr<OpenApiResponse>>,
    pub servers: Vec<OpenApiServer>,
    pub external_docs: Option<OpenApiExternalDocs>,
    pub extensions: BTreeMap<String, Value>,
    pub schema_components: BTreeMap<String, OpenApiSchema>,
    pub response_components: BTreeMap<String, OpenApiResponse>,
    pub parameter_components: BTreeMap<String, OpenApiParameter>,
    pub example_components: BTreeMap<String, OpenApiExample>,
    pub request_body_components: BTreeMap<String, OpenApiRequestBody>,
    pub header_components: BTreeMap<String, OpenApiHeader>,
    pub security_schemes: BTreeMap<String, OpenApiSecurityScheme>,
    pub security: Vec<OpenApiSecurityRequirement>,
    pub deprecated: bool,
    pub hidden: bool,
}

/// OpenAPI tag entry.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct OpenApiTag {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(rename = "externalDocs", skip_serializing_if = "Option::is_none")]
    pub external_docs: Option<OpenApiExternalDocs>,
}

impl OpenApiTag {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: None,
            external_docs: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_external_docs(
        mut self,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.external_docs = Some(OpenApiExternalDocs::new(description, url));
        self
    }

    fn with_external_docs_object(mut self, external_docs: OpenApiExternalDocs) -> Self {
        self.external_docs = Some(external_docs);
        self
    }
}

/// OpenAPI parameter location.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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
    #[serde(skip_serializing_if = "is_false")]
    pub deprecated: bool,
    #[serde(rename = "allowReserved", skip_serializing_if = "is_false")]
    pub allow_reserved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub style: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub explode: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub examples: BTreeMap<String, OpenApiReferenceOr<OpenApiExample>>,
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
            deprecated: false,
            allow_reserved: false,
            style: None,
            explode: None,
            example: None,
            examples: BTreeMap::new(),
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

    pub fn cookie(name: impl Into<String>, required: bool, schema: OpenApiSchema) -> Self {
        Self::new(OpenApiParameterLocation::Cookie, name, required, schema)
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn with_deprecated(mut self) -> Self {
        self.deprecated = true;
        self
    }

    pub fn with_allow_reserved(mut self) -> Self {
        self.allow_reserved = true;
        self
    }

    pub fn with_style(mut self, style: impl Into<String>) -> Self {
        self.style = Some(style.into());
        self
    }

    pub fn with_explode(mut self, explode: bool) -> Self {
        self.explode = Some(explode);
        self
    }

    pub fn with_example_value(mut self, example: Value) -> Self {
        self.example = Some(example);
        self.examples.clear();
        self
    }

    pub fn try_with_example<T>(self, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_example_value(serialize_openapi_example(example)?))
    }

    pub fn with_named_example_value(
        mut self,
        name: impl Into<String>,
        example: OpenApiExample,
    ) -> Self {
        self.example = None;
        self.examples
            .insert(name.into(), OpenApiReferenceOr::value(example));
        self
    }

    pub fn try_with_named_example<T>(self, name: impl Into<String>, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_named_example_value(name, OpenApiExample::try_value(example)?))
    }

    pub fn with_named_example_ref(
        mut self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.example = None;
        self.examples.insert(
            name.into(),
            OpenApiReferenceOr::reference(OpenApiRef::example(component_name)),
        );
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
    pub fn content(content_type: impl Into<String>, schema: OpenApiSchema) -> Self {
        let mut content = BTreeMap::new();
        content.insert(content_type.into(), OpenApiMediaType::new(schema));
        Self {
            description: None,
            required: true,
            content,
        }
    }

    pub fn json(schema: OpenApiSchema) -> Self {
        Self::content("application/json", schema)
    }

    pub fn try_content_example<T>(
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        Self::content(content_type.clone(), schema).try_with_content_example(content_type, example)
    }

    pub fn try_json_example<T>(schema: OpenApiSchema, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        Self::json(schema).try_with_json_example(example)
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }

    pub fn optional(mut self) -> Self {
        self.required = false;
        self
    }

    pub fn try_with_content_example<T>(
        mut self,
        content_type: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        let example = serialize_openapi_example(example)?;
        let media = self
            .content
            .entry(content_type)
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()));
        media.example = Some(example);
        media.examples.clear();
        Ok(self)
    }

    pub fn try_with_content_named_example<T>(
        mut self,
        content_type: impl Into<String>,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        self.content
            .entry(content_type)
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()))
            .try_insert_example(name, example)?;
        Ok(self)
    }

    pub fn with_content_named_example_ref(
        mut self,
        content_type: impl Into<String>,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.content
            .entry(content_type.into())
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()))
            .insert_example_ref(name, component_name);
        self
    }

    pub fn try_with_json_example<T>(self, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.try_with_content_example("application/json", example)
    }

    pub fn try_with_json_named_example<T>(self, name: impl Into<String>, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.try_with_content_named_example("application/json", name, example)
    }

    pub fn with_json_named_example_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_content_named_example_ref("application/json", name, component_name)
    }
}

/// OpenAPI response metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiResponse {
    pub description: String,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub headers: BTreeMap<String, OpenApiReferenceOr<OpenApiHeader>>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub content: BTreeMap<String, OpenApiMediaType>,
}

impl OpenApiResponse {
    pub fn description(description: impl Into<String>) -> Self {
        Self {
            description: description.into(),
            headers: BTreeMap::new(),
            content: BTreeMap::new(),
        }
    }

    pub fn content(
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
    ) -> Self {
        let mut content = BTreeMap::new();
        content.insert(content_type.into(), OpenApiMediaType::new(schema));
        Self {
            description: description.into(),
            headers: BTreeMap::new(),
            content,
        }
    }

    pub fn json(description: impl Into<String>, schema: OpenApiSchema) -> Self {
        Self::content(description, "application/json", schema)
    }

    pub fn try_content_example<T>(
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        Self::content(description, content_type.clone(), schema)
            .try_with_content_example(content_type, example)
    }

    pub fn try_json_example<T>(
        description: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Self::json(description, schema).try_with_json_example(example)
    }

    pub fn try_with_content_example<T>(
        mut self,
        content_type: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        let example = serialize_openapi_example(example)?;
        let media = self
            .content
            .entry(content_type)
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()));
        media.example = Some(example);
        media.examples.clear();
        Ok(self)
    }

    pub fn try_with_content_named_example<T>(
        mut self,
        content_type: impl Into<String>,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        self.content
            .entry(content_type)
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()))
            .try_insert_example(name, example)?;
        Ok(self)
    }

    pub fn with_content_named_example_ref(
        mut self,
        content_type: impl Into<String>,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.content
            .entry(content_type.into())
            .or_insert_with(|| OpenApiMediaType::new(OpenApiSchema::object()))
            .insert_example_ref(name, component_name);
        self
    }

    pub fn try_with_json_example<T>(self, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.try_with_content_example("application/json", example)
    }

    pub fn try_with_json_named_example<T>(self, name: impl Into<String>, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        self.try_with_content_named_example("application/json", name, example)
    }

    pub fn with_json_named_example_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_content_named_example_ref("application/json", name, component_name)
    }

    pub fn with_header(mut self, name: impl Into<String>, header: OpenApiHeader) -> Self {
        self.headers
            .insert(name.into(), OpenApiReferenceOr::value(header));
        self
    }

    pub fn with_header_ref(
        mut self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.headers.insert(
            name.into(),
            OpenApiReferenceOr::reference(OpenApiRef::header(component_name)),
        );
        self
    }
}

/// OpenAPI response header metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiHeader {
    pub schema: OpenApiSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl OpenApiHeader {
    pub fn new(schema: OpenApiSchema) -> Self {
        Self {
            schema,
            description: None,
        }
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

/// OpenAPI media type metadata.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiMediaType {
    pub schema: OpenApiSchema,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub example: Option<Value>,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub examples: BTreeMap<String, OpenApiReferenceOr<OpenApiExample>>,
}

impl OpenApiMediaType {
    pub fn new(schema: OpenApiSchema) -> Self {
        Self {
            schema,
            example: None,
            examples: BTreeMap::new(),
        }
    }

    pub fn with_example_value(mut self, example: Value) -> Self {
        self.example = Some(example);
        self.examples.clear();
        self
    }

    pub fn try_with_example<T>(self, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_example_value(serialize_openapi_example(example)?))
    }

    pub fn with_named_example_value(
        mut self,
        name: impl Into<String>,
        example: OpenApiExample,
    ) -> Self {
        self.example = None;
        self.examples
            .insert(name.into(), OpenApiReferenceOr::value(example));
        self
    }

    pub fn with_named_example_ref(
        mut self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.example = None;
        self.examples.insert(
            name.into(),
            OpenApiReferenceOr::reference(OpenApiRef::example(component_name)),
        );
        self
    }

    pub fn try_with_named_example<T>(self, name: impl Into<String>, example: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_named_example_value(name, OpenApiExample::try_value(example)?))
    }

    fn try_insert_example<T>(&mut self, name: impl Into<String>, example: T) -> Result<()>
    where
        T: Serialize,
    {
        self.example = None;
        self.examples.insert(
            name.into(),
            OpenApiReferenceOr::value(OpenApiExample::try_value(example)?),
        );
        Ok(())
    }

    fn insert_example_ref(&mut self, name: impl Into<String>, component_name: impl AsRef<str>) {
        self.example = None;
        self.examples.insert(
            name.into(),
            OpenApiReferenceOr::reference(OpenApiRef::example(component_name)),
        );
    }
}

/// Named OpenAPI media example.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct OpenApiExample {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    pub value: Value,
}

impl OpenApiExample {
    pub fn value(value: Value) -> Self {
        Self {
            summary: None,
            description: None,
            value,
        }
    }

    pub fn try_value<T>(value: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(Self::value(serialize_openapi_example(value)?))
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.summary = Some(summary.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.description = Some(description.into());
        self
    }
}

fn serialize_openapi_example<T>(example: T) -> Result<Value>
where
    T: Serialize,
{
    serde_json::to_value(example).map_err(|error| {
        BootError::Internal(format!("OpenAPI example could not be serialized: {error}"))
    })
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

    pub fn string_enum<I, S>(values: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let values = values.into_iter().map(Into::into).collect::<Vec<_>>();
        Self(json!({ "type": "string", "enum": values }))
    }

    pub fn binary_file() -> Self {
        Self(json!({ "type": "string", "format": "binary" }))
    }

    pub fn all_of<I>(schemas: I) -> Self
    where
        I: IntoIterator<Item = OpenApiSchema>,
    {
        Self(schema_composition("allOf", schemas))
    }

    pub fn one_of<I>(schemas: I) -> Self
    where
        I: IntoIterator<Item = OpenApiSchema>,
    {
        Self(schema_composition("oneOf", schemas))
    }

    pub fn any_of<I>(schemas: I) -> Self
    where
        I: IntoIterator<Item = OpenApiSchema>,
    {
        Self(schema_composition("anyOf", schemas))
    }

    pub fn object_with_properties<P, K, R, S>(properties: P, required: R) -> Self
    where
        P: IntoIterator<Item = (K, OpenApiSchema)>,
        K: Into<String>,
        R: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let properties = properties
            .into_iter()
            .map(|(name, schema)| (name.into(), schema.into_value()))
            .collect::<BTreeMap<String, Value>>();
        let required = required.into_iter().map(Into::into).collect::<Vec<_>>();
        let mut schema = json!({
            "type": "object",
            "properties": properties,
        });

        if !required.is_empty() {
            if let Value::Object(object) = &mut schema {
                object.insert("required".to_string(), json!(required));
            }
        }

        Self(schema)
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        insert_schema_field(&mut self.0, "title", json!(title.into()));
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        insert_schema_field(&mut self.0, "description", json!(description.into()));
        self
    }

    pub fn with_format(mut self, format: impl Into<String>) -> Self {
        insert_schema_field(&mut self.0, "format", json!(format.into()));
        self
    }

    pub fn nullable(mut self) -> Self {
        insert_schema_field(&mut self.0, "nullable", json!(true));
        self
    }

    pub fn with_property(mut self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        let name = name.into();
        let Some(object) = ensure_object_schema(&mut self.0) else {
            return self;
        };
        let properties = object
            .entry("properties".to_string())
            .or_insert_with(|| json!({}));
        if !properties.is_object() {
            *properties = json!({});
        }
        if let Value::Object(properties) = properties {
            properties.insert(name, schema.into_value());
        }
        self
    }

    pub fn with_required(mut self, name: impl Into<String>) -> Self {
        let Some(object) = ensure_object_schema(&mut self.0) else {
            return self;
        };
        let required = object
            .entry("required".to_string())
            .or_insert_with(|| json!([]));
        if !required.is_array() {
            *required = json!([]);
        }
        if let Value::Array(required) = required {
            let name = Value::String(name.into());
            if !required.contains(&name) {
                required.push(name);
            }
        }
        self
    }

    pub fn with_additional_properties(mut self, schema: OpenApiSchema) -> Self {
        let _ = ensure_object_schema(&mut self.0);
        insert_schema_field(&mut self.0, "additionalProperties", schema.into_value());
        self
    }

    pub fn with_discriminator(mut self, property_name: impl Into<String>) -> Self {
        insert_schema_field(
            &mut self.0,
            "discriminator",
            json!({ "propertyName": property_name.into() }),
        );
        self
    }

    pub fn with_discriminator_mapping<I, K, V>(
        mut self,
        property_name: impl Into<String>,
        mapping: I,
    ) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        let mapping = mapping
            .into_iter()
            .map(|(key, value)| (key.into(), Value::String(value.into())))
            .collect::<serde_json::Map<_, _>>();
        insert_schema_field(
            &mut self.0,
            "discriminator",
            json!({
                "propertyName": property_name.into(),
                "mapping": mapping,
            }),
        );
        self
    }

    pub fn with_extension_value(mut self, name: impl Into<String>, value: Value) -> Self {
        insert_schema_field(&mut self.0, &name.into(), value);
        self
    }

    pub fn try_with_extension<T>(self, name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        let value = serde_json::to_value(value).map_err(|error| {
            BootError::Internal(format!(
                "OpenAPI extension could not be serialized: {error}"
            ))
        })?;
        Ok(self.with_extension_value(name, value))
    }

    pub fn partial(mut self) -> Self {
        remove_schema_required(&mut self.0);
        self
    }

    pub fn pick_properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names = names.into_iter().map(Into::into).collect::<BTreeSet<_>>();
        retain_schema_properties(&mut self.0, &names, true);
        self
    }

    pub fn omit_properties<I, S>(mut self, names: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let names = names.into_iter().map(Into::into).collect::<BTreeSet<_>>();
        retain_schema_properties(&mut self.0, &names, false);
        self
    }

    pub fn reference(name: impl AsRef<str>) -> Self {
        Self(json!({ "$ref": OpenApiRef::schema(name).reference }))
    }

    pub fn into_value(self) -> Value {
        self.0
    }
}

fn schema_composition<I>(kind: &str, schemas: I) -> Value
where
    I: IntoIterator<Item = OpenApiSchema>,
{
    let schemas = schemas
        .into_iter()
        .map(OpenApiSchema::into_value)
        .collect::<Vec<_>>();
    let mut schema = serde_json::Map::new();
    schema.insert(kind.to_string(), Value::Array(schemas));
    Value::Object(schema)
}

fn insert_schema_field(schema: &mut Value, key: &str, value: Value) {
    if !schema.is_object() {
        *schema = json!({});
    }
    if let Value::Object(object) = schema {
        object.insert(key.to_string(), value);
    }
}

fn ensure_object_schema(schema: &mut Value) -> Option<&mut serde_json::Map<String, Value>> {
    if !schema.is_object() {
        *schema = json!({ "type": "object" });
    }
    if let Value::Object(object) = schema {
        object
            .entry("type".to_string())
            .or_insert_with(|| json!("object"));
        return Some(object);
    }
    None
}

fn remove_schema_required(schema: &mut Value) {
    if let Value::Object(object) = schema {
        object.remove("required");
    }
}

fn retain_schema_properties(schema: &mut Value, names: &BTreeSet<String>, keep_matches: bool) {
    let Value::Object(object) = schema else {
        return;
    };

    if let Some(Value::Object(properties)) = object.get_mut("properties") {
        properties.retain(|name, _| names.contains(name) == keep_matches);
    }

    if let Some(Value::Array(required)) = object.get_mut("required") {
        required.retain(|value| {
            value
                .as_str()
                .is_some_and(|name| names.contains(name) == keep_matches)
        });
        if required.is_empty() {
            object.remove("required");
        }
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
