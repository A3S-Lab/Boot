use super::definition::RouteDefinition;
#[cfg(feature = "openapi-schemas")]
use crate::{openapi_schema_name, BootError};
use crate::{
    OpenApiApiKeyLocation, OpenApiExample, OpenApiHeader, OpenApiOAuthFlows, OpenApiParameter,
    OpenApiParameterLocation, OpenApiRef, OpenApiReferenceOr, OpenApiRequestBody, OpenApiResponse,
    OpenApiRouteMetadata, OpenApiSchema, OpenApiSecurityRequirement, OpenApiSecurityScheme,
    OpenApiServer, Result,
};
use serde::Serialize;
use serde_json::Value;

impl RouteDefinition {
    pub fn with_openapi(mut self, metadata: OpenApiRouteMetadata) -> Self {
        self.openapi = metadata;
        self
    }

    pub fn with_tag(mut self, tag: impl Into<String>) -> Self {
        let tag = tag.into();
        if !self.openapi.tags.contains(&tag) {
            self.openapi.tags.push(tag);
        }
        self
    }

    pub fn with_operation_id(mut self, operation_id: impl Into<String>) -> Self {
        self.openapi.operation_id = Some(operation_id.into());
        self
    }

    pub fn with_summary(mut self, summary: impl Into<String>) -> Self {
        self.openapi.summary = Some(summary.into());
        self
    }

    pub fn with_description(mut self, description: impl Into<String>) -> Self {
        self.openapi.description = Some(description.into());
        self
    }

    pub fn with_deprecated(mut self) -> Self {
        self.openapi.deprecated = true;
        self
    }

    pub fn with_openapi_server(mut self, url: impl Into<String>) -> Self {
        self.openapi.servers.push(OpenApiServer::new(url));
        self
    }

    pub fn with_openapi_server_description(
        mut self,
        url: impl Into<String>,
        description: impl Into<String>,
    ) -> Self {
        self.openapi
            .servers
            .push(OpenApiServer::new(url).with_description(description));
        self
    }

    pub fn with_openapi_external_docs(
        mut self,
        description: impl Into<String>,
        url: impl Into<String>,
    ) -> Self {
        self.openapi.external_docs = Some(crate::OpenApiExternalDocs::new(description, url));
        self
    }

    pub fn with_openapi_extension_value(mut self, name: impl Into<String>, value: Value) -> Self {
        self.openapi.extensions.insert(name.into(), value);
        self
    }

    pub fn with_openapi_extension_default_value(
        mut self,
        name: impl Into<String>,
        value: Value,
    ) -> Self {
        self.openapi.extensions.entry(name.into()).or_insert(value);
        self
    }

    pub fn try_with_openapi_extension<T>(self, name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        let name = name.into();
        let value = serde_json::to_value(value).map_err(|error| {
            crate::BootError::Internal(format!(
                "OpenAPI extension `{name}` could not be serialized: {error}"
            ))
        })?;
        Ok(self.with_openapi_extension_value(name, value))
    }

    pub fn hide_from_openapi(mut self) -> Self {
        self.openapi.hidden = true;
        self
    }

    pub fn with_parameter(mut self, parameter: OpenApiParameter) -> Self {
        upsert_parameter(
            &mut self.openapi.parameters,
            OpenApiReferenceOr::value(parameter),
        );
        self
    }

    pub fn with_parameter_ref(
        mut self,
        location: OpenApiParameterLocation,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        let name = name.into();
        let reference =
            OpenApiRef::parameter(component_name).with_parameter_metadata(location, name);
        upsert_parameter(
            &mut self.openapi.parameters,
            OpenApiReferenceOr::reference(reference),
        );
        self
    }

    pub fn with_path_parameter(self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        self.with_parameter(OpenApiParameter::path(name, schema))
    }

    pub fn with_path_parameter_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_parameter_ref(OpenApiParameterLocation::Path, name, component_name)
    }

    pub fn with_query_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::query(name, required, schema))
    }

    pub fn with_query_parameter_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_parameter_ref(OpenApiParameterLocation::Query, name, component_name)
    }

    pub fn with_header_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::header(name, required, schema))
    }

    pub fn with_header_parameter_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_parameter_ref(OpenApiParameterLocation::Header, name, component_name)
    }

    pub fn with_cookie_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::cookie(name, required, schema))
    }

    pub fn with_cookie_parameter_ref(
        self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_parameter_ref(OpenApiParameterLocation::Cookie, name, component_name)
    }

    pub fn with_request_body(mut self, request_body: OpenApiRequestBody) -> Self {
        self.openapi.request_body = Some(OpenApiReferenceOr::value(request_body));
        self
    }

    pub fn with_request_body_ref(mut self, component_name: impl AsRef<str>) -> Self {
        self.openapi.request_body = Some(OpenApiReferenceOr::reference(OpenApiRef::request_body(
            component_name,
        )));
        self
    }

    pub fn with_request_body_content_type(
        self,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_request_body(OpenApiRequestBody::content(content_type, schema))
    }

    pub fn with_json_request_body(self, schema: OpenApiSchema) -> Self {
        self.with_request_body(OpenApiRequestBody::json(schema))
    }

    pub fn try_with_request_body_content_type_example<T>(
        self,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(
            self.with_request_body(OpenApiRequestBody::try_content_example(
                content_type,
                schema,
                example,
            )?),
        )
    }

    pub fn try_with_json_request_body_example<T>(
        self,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_request_body(OpenApiRequestBody::try_json_example(schema, example)?))
    }

    pub fn try_with_request_body_content_type_named_example<T>(
        self,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        let request_body = OpenApiRequestBody::content(content_type.clone(), schema)
            .try_with_content_named_example(content_type, name, example)?;
        Ok(self.with_request_body(request_body))
    }

    pub fn try_with_json_request_body_named_example<T>(
        self,
        schema: OpenApiSchema,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_request_body(
            OpenApiRequestBody::json(schema).try_with_json_named_example(name, example)?,
        ))
    }

    pub fn with_request_body_content_type_named_example_ref(
        self,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        let content_type = content_type.into();
        let request_body = OpenApiRequestBody::content(content_type.clone(), schema)
            .with_content_named_example_ref(content_type, name, component_name);
        self.with_request_body(request_body)
    }

    pub fn with_json_request_body_named_example_ref(
        self,
        schema: OpenApiSchema,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_request_body(
            OpenApiRequestBody::json(schema).with_json_named_example_ref(name, component_name),
        )
    }

    pub fn with_response(mut self, status: u16, response: OpenApiResponse) -> Self {
        self.openapi
            .responses
            .insert(status.to_string(), OpenApiReferenceOr::value(response));
        self
    }

    pub fn with_response_ref(mut self, status: u16, component_name: impl AsRef<str>) -> Self {
        self.openapi.responses.insert(
            status.to_string(),
            OpenApiReferenceOr::reference(OpenApiRef::response(component_name)),
        );
        self
    }

    pub fn with_default_response(mut self, response: OpenApiResponse) -> Self {
        self.openapi
            .responses
            .insert("default".to_string(), OpenApiReferenceOr::value(response));
        self
    }

    pub fn with_default_response_ref(mut self, component_name: impl AsRef<str>) -> Self {
        self.openapi.responses.insert(
            "default".to_string(),
            OpenApiReferenceOr::reference(OpenApiRef::response(component_name)),
        );
        self
    }

    pub fn with_openapi_response_header(
        mut self,
        status: u16,
        name: impl Into<String>,
        header: OpenApiHeader,
    ) -> Self {
        if let Some(response) = self
            .openapi
            .responses
            .entry(status.to_string())
            .or_insert_with(|| OpenApiReferenceOr::value(OpenApiResponse::description("Success")))
            .value_mut()
        {
            response
                .headers
                .insert(name.into(), OpenApiReferenceOr::value(header));
        }
        self
    }

    pub fn with_openapi_response_header_ref(
        mut self,
        status: u16,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        if let Some(response) = self
            .openapi
            .responses
            .entry(status.to_string())
            .or_insert_with(|| OpenApiReferenceOr::value(OpenApiResponse::description("Success")))
            .value_mut()
        {
            response.headers.insert(
                name.into(),
                OpenApiReferenceOr::reference(OpenApiRef::header(component_name)),
            );
        }
        self
    }

    pub fn with_default_openapi_response_header(
        mut self,
        name: impl Into<String>,
        header: OpenApiHeader,
    ) -> Self {
        if let Some(response) = self
            .openapi
            .responses
            .entry("default".to_string())
            .or_insert_with(|| {
                OpenApiReferenceOr::value(OpenApiResponse::description("Default response"))
            })
            .value_mut()
        {
            response
                .headers
                .insert(name.into(), OpenApiReferenceOr::value(header));
        }
        self
    }

    pub fn with_default_openapi_response_header_ref(
        mut self,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        if let Some(response) = self
            .openapi
            .responses
            .entry("default".to_string())
            .or_insert_with(|| {
                OpenApiReferenceOr::value(OpenApiResponse::description("Default response"))
            })
            .value_mut()
        {
            response.headers.insert(
                name.into(),
                OpenApiReferenceOr::reference(OpenApiRef::header(component_name)),
            );
        }
        self
    }

    pub fn with_response_content_type(
        self,
        status: u16,
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_response(
            status,
            OpenApiResponse::content(description, content_type, schema),
        )
    }

    pub fn with_json_response(
        self,
        status: u16,
        description: impl Into<String>,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_response(status, OpenApiResponse::json(description, schema))
    }

    pub fn try_with_response_content_type_example<T>(
        self,
        status: u16,
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_response(
            status,
            OpenApiResponse::try_content_example(description, content_type, schema, example)?,
        ))
    }

    pub fn try_with_json_response_example<T>(
        self,
        status: u16,
        description: impl Into<String>,
        schema: OpenApiSchema,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_response(
            status,
            OpenApiResponse::try_json_example(description, schema, example)?,
        ))
    }

    pub fn try_with_response_content_type_named_example<T>(
        self,
        status: u16,
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        let content_type = content_type.into();
        let response = OpenApiResponse::content(description, content_type.clone(), schema)
            .try_with_content_named_example(content_type, name, example)?;
        Ok(self.with_response(status, response))
    }

    pub fn try_with_json_response_named_example<T>(
        self,
        status: u16,
        description: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        example: T,
    ) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_response(
            status,
            OpenApiResponse::json(description, schema)
                .try_with_json_named_example(name, example)?,
        ))
    }

    pub fn with_response_content_type_named_example_ref(
        self,
        status: u16,
        description: impl Into<String>,
        content_type: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        let content_type = content_type.into();
        let response = OpenApiResponse::content(description, content_type.clone(), schema)
            .with_content_named_example_ref(content_type, name, component_name);
        self.with_response(status, response)
    }

    pub fn with_json_response_named_example_ref(
        self,
        status: u16,
        description: impl Into<String>,
        schema: OpenApiSchema,
        name: impl Into<String>,
        component_name: impl AsRef<str>,
    ) -> Self {
        self.with_response(
            status,
            OpenApiResponse::json(description, schema)
                .with_json_named_example_ref(name, component_name),
        )
    }

    pub fn with_security_requirement(mut self, requirement: OpenApiSecurityRequirement) -> Self {
        self.openapi.security.push(requirement);
        self
    }

    pub fn with_api_security<I, S>(self, name: impl Into<String>, scopes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let mut requirement = OpenApiSecurityRequirement::new();
        requirement.insert(
            name.into(),
            scopes.into_iter().map(Into::into).collect::<Vec<_>>(),
        );
        self.with_security_requirement(requirement)
    }

    pub fn with_security_scheme(
        mut self,
        name: impl Into<String>,
        scheme: OpenApiSecurityScheme,
    ) -> Self {
        self.openapi.security_schemes.insert(name.into(), scheme);
        self
    }

    pub fn with_bearer_auth(self) -> Self {
        self.with_bearer_auth_named("bearerAuth")
    }

    pub fn with_bearer_auth_named(self, name: impl Into<String>) -> Self {
        let name = name.into();
        self.with_security_scheme(name.clone(), OpenApiSecurityScheme::http_bearer())
            .with_api_security(name, Vec::<String>::new())
    }

    pub fn with_api_key_auth(
        self,
        scheme_name: impl Into<String>,
        location: OpenApiApiKeyLocation,
        key_name: impl Into<String>,
    ) -> Self {
        let scheme_name = scheme_name.into();
        self.with_security_scheme(
            scheme_name.clone(),
            OpenApiSecurityScheme::api_key(location, key_name),
        )
        .with_api_security(scheme_name, Vec::<String>::new())
    }

    pub fn with_header_api_key_auth(
        self,
        scheme_name: impl Into<String>,
        header_name: impl Into<String>,
    ) -> Self {
        self.with_api_key_auth(scheme_name, OpenApiApiKeyLocation::Header, header_name)
    }

    pub fn with_query_api_key_auth(
        self,
        scheme_name: impl Into<String>,
        query_name: impl Into<String>,
    ) -> Self {
        self.with_api_key_auth(scheme_name, OpenApiApiKeyLocation::Query, query_name)
    }

    pub fn with_cookie_auth(
        self,
        scheme_name: impl Into<String>,
        cookie_name: impl Into<String>,
    ) -> Self {
        self.with_api_key_auth(scheme_name, OpenApiApiKeyLocation::Cookie, cookie_name)
    }

    pub fn with_oauth2_auth<I, S>(
        self,
        scheme_name: impl Into<String>,
        flows: OpenApiOAuthFlows,
        scopes: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let scheme_name = scheme_name.into();
        self.with_security_scheme(scheme_name.clone(), OpenApiSecurityScheme::oauth2(flows))
            .with_api_security(scheme_name, scopes)
    }

    pub fn with_open_id_connect_auth<I, S>(
        self,
        scheme_name: impl Into<String>,
        url: impl Into<String>,
        scopes: I,
    ) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        let scheme_name = scheme_name.into();
        self.with_security_scheme(
            scheme_name.clone(),
            OpenApiSecurityScheme::open_id_connect(url),
        )
        .with_api_security(scheme_name, scopes)
    }

    pub fn with_schema_component(mut self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        self.openapi.schema_components.insert(name.into(), schema);
        self
    }

    pub fn with_response_component(
        mut self,
        name: impl Into<String>,
        response: OpenApiResponse,
    ) -> Self {
        self.openapi
            .response_components
            .insert(name.into(), response);
        self
    }

    pub fn with_parameter_component(
        mut self,
        name: impl Into<String>,
        parameter: OpenApiParameter,
    ) -> Self {
        self.openapi
            .parameter_components
            .insert(name.into(), parameter);
        self
    }

    pub fn with_example_component(
        mut self,
        name: impl Into<String>,
        example: OpenApiExample,
    ) -> Self {
        self.openapi.example_components.insert(name.into(), example);
        self
    }

    pub fn try_with_example_component<T>(self, name: impl Into<String>, value: T) -> Result<Self>
    where
        T: Serialize,
    {
        Ok(self.with_example_component(name, OpenApiExample::try_value(value)?))
    }

    pub fn with_request_body_component(
        mut self,
        name: impl Into<String>,
        request_body: OpenApiRequestBody,
    ) -> Self {
        self.openapi
            .request_body_components
            .insert(name.into(), request_body);
        self
    }

    pub fn with_header_component(mut self, name: impl Into<String>, header: OpenApiHeader) -> Self {
        self.openapi.header_components.insert(name.into(), header);
        self
    }

    #[cfg(feature = "openapi-schemas")]
    pub fn try_with_json_schema_component<T>(self) -> Result<Self>
    where
        T: schemars::JsonSchema,
    {
        let schema = OpenApiSchema::json_schema::<T>()
            .map_err(|error| BootError::Internal(error.to_string()))?;
        Ok(self.with_schema_component(openapi_schema_name::<T>(), schema))
    }
}

fn upsert_parameter(
    parameters: &mut Vec<OpenApiReferenceOr<OpenApiParameter>>,
    parameter: OpenApiReferenceOr<OpenApiParameter>,
) {
    if let Some((location, name)) = parameter.parameter_identity() {
        if let Some(existing) = parameters.iter_mut().find(|existing| {
            existing
                .parameter_identity()
                .is_some_and(|(existing_location, existing_name)| {
                    existing_location == location && existing_name == name
                })
        }) {
            *existing = parameter;
            return;
        }
    }

    parameters.push(parameter);
}
