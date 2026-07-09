use super::definition::RouteDefinition;
#[cfg(feature = "openapi-schemas")]
use crate::{openapi_schema_name, BootError};
use crate::{
    OpenApiApiKeyLocation, OpenApiParameter, OpenApiRequestBody, OpenApiResponse,
    OpenApiRouteMetadata, OpenApiSchema, OpenApiSecurityRequirement, OpenApiSecurityScheme, Result,
};
use serde::Serialize;

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

    pub fn hide_from_openapi(mut self) -> Self {
        self.openapi.hidden = true;
        self
    }

    pub fn with_parameter(mut self, parameter: OpenApiParameter) -> Self {
        upsert_parameter(&mut self.openapi.parameters, parameter);
        self
    }

    pub fn with_path_parameter(self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        self.with_parameter(OpenApiParameter::path(name, schema))
    }

    pub fn with_query_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::query(name, required, schema))
    }

    pub fn with_header_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::header(name, required, schema))
    }

    pub fn with_cookie_parameter(
        self,
        name: impl Into<String>,
        required: bool,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_parameter(OpenApiParameter::cookie(name, required, schema))
    }

    pub fn with_request_body(mut self, request_body: OpenApiRequestBody) -> Self {
        self.openapi.request_body = Some(request_body);
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

    pub fn with_response(mut self, status: u16, response: OpenApiResponse) -> Self {
        self.openapi.responses.insert(status.to_string(), response);
        self
    }

    pub fn with_default_response(mut self, response: OpenApiResponse) -> Self {
        self.openapi
            .responses
            .insert("default".to_string(), response);
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

    pub fn with_schema_component(mut self, name: impl Into<String>, schema: OpenApiSchema) -> Self {
        self.openapi.schema_components.insert(name.into(), schema);
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

fn upsert_parameter(parameters: &mut Vec<OpenApiParameter>, parameter: OpenApiParameter) {
    if let Some(existing) = parameters
        .iter_mut()
        .find(|existing| existing.location == parameter.location && existing.name == parameter.name)
    {
        *existing = parameter;
        return;
    }

    parameters.push(parameter);
}
