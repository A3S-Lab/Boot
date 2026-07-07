use crate::{BootError, BootResponse, BoxFuture, ExecutionContext, Interceptor, Result};
use serde_json::{Map, Value};
use std::collections::BTreeSet;

/// JSON response shaping options used by [`SerializationInterceptor`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SerializationOptions {
    include_fields: Option<BTreeSet<String>>,
    exclude_fields: BTreeSet<String>,
    skip_null_fields: bool,
}

impl SerializationOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn include_field(mut self, field: impl Into<String>) -> Self {
        self.include_fields
            .get_or_insert_with(BTreeSet::new)
            .insert(field.into());
        self
    }

    pub fn include_fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for field in fields {
            self = self.include_field(field);
        }
        self
    }

    pub fn exclude_field(mut self, field: impl Into<String>) -> Self {
        self.exclude_fields.insert(field.into());
        self
    }

    pub fn exclude_fields<I, S>(mut self, fields: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: Into<String>,
    {
        for field in fields {
            self = self.exclude_field(field);
        }
        self
    }

    pub fn skip_null_fields(mut self) -> Self {
        self.skip_null_fields = true;
        self
    }

    pub fn included_fields(&self) -> Option<impl Iterator<Item = &str>> {
        self.include_fields
            .as_ref()
            .map(|fields| fields.iter().map(String::as_str))
    }

    pub fn excluded_fields(&self) -> impl Iterator<Item = &str> {
        self.exclude_fields.iter().map(String::as_str)
    }

    pub fn skips_null_fields(&self) -> bool {
        self.skip_null_fields
    }

    pub fn is_empty(&self) -> bool {
        self.include_fields.is_none() && self.exclude_fields.is_empty() && !self.skip_null_fields
    }

    fn merged_with(&self, route_options: &Self) -> Self {
        let mut merged = self.clone();
        if route_options.include_fields.is_some() {
            merged.include_fields = route_options.include_fields.clone();
        }
        merged
            .exclude_fields
            .extend(route_options.exclude_fields.iter().cloned());
        merged.skip_null_fields |= route_options.skip_null_fields;
        merged
    }
}

/// Interceptor that shapes JSON response objects using route serialization metadata.
#[derive(Debug, Clone, Default)]
pub struct SerializationInterceptor {
    options: SerializationOptions,
}

impl SerializationInterceptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: SerializationOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &SerializationOptions {
        &self.options
    }
}

impl Interceptor for SerializationInterceptor {
    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let options = self.options.clone();
        Box::pin(async move { serialize_response(response, &options, &context.serialization) })
    }
}

fn serialize_response(
    mut response: BootResponse,
    interceptor_options: &SerializationOptions,
    route_options: &SerializationOptions,
) -> Result<BootResponse> {
    let options = interceptor_options.merged_with(route_options);
    if options.is_empty()
        || response.is_streaming()
        || !response.has_body()
        || !response.is_json_content_type()
    {
        return Ok(response);
    }

    let had_content_length = !response.header_values("content-length").is_empty();
    let mut value = serde_json::from_slice::<Value>(&response.body)
        .map_err(|error| BootError::Internal(format!("invalid JSON response body: {error}")))?;
    transform_payload(&mut value, &options);
    response.body = serde_json::to_vec(&value)
        .map_err(|error| BootError::Internal(format!("failed to serialize response: {error}")))?;

    if had_content_length {
        response.headers.remove("content-length");
        response
            .appended_headers
            .retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
        let content_length = response.body.len() as u64;
        response = response.with_content_length(content_length);
    }

    Ok(response)
}

fn transform_payload(value: &mut Value, options: &SerializationOptions) {
    match value {
        Value::Object(object) => transform_object(object, options),
        Value::Array(values) => {
            for value in values {
                if let Value::Object(object) = value {
                    transform_object(object, options);
                }
            }
        }
        _ => {}
    }
}

fn transform_object(object: &mut Map<String, Value>, options: &SerializationOptions) {
    if let Some(include_fields) = &options.include_fields {
        object.retain(|field, _| include_fields.contains(field));
    }

    for field in &options.exclude_fields {
        object.remove(field);
    }

    if options.skip_null_fields {
        object.retain(|_, value| !value.is_null());
    }
}
