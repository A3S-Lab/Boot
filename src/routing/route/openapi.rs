use super::definition::RouteDefinition;
#[cfg(feature = "openapi-schemas")]
use crate::{openapi_schema_name, BootError, Result};
use crate::{
    OpenApiParameter, OpenApiRequestBody, OpenApiResponse, OpenApiRouteMetadata, OpenApiSchema,
    OpenApiSecurityRequirement,
};

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

    pub fn with_request_body(mut self, request_body: OpenApiRequestBody) -> Self {
        self.openapi.request_body = Some(request_body);
        self
    }

    pub fn with_json_request_body(self, schema: OpenApiSchema) -> Self {
        self.with_request_body(OpenApiRequestBody::json(schema))
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

    pub fn with_json_response(
        self,
        status: u16,
        description: impl Into<String>,
        schema: OpenApiSchema,
    ) -> Self {
        self.with_response(status, OpenApiResponse::json(description, schema))
    }

    pub fn with_security_requirement(mut self, requirement: OpenApiSecurityRequirement) -> Self {
        self.openapi.security.push(requirement);
        self
    }

    pub fn with_bearer_auth(self) -> Self {
        let mut requirement = OpenApiSecurityRequirement::new();
        requirement.insert("bearerAuth".to_string(), Vec::new());
        self.with_security_requirement(requirement)
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
