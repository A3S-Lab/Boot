use crate::{BootError, BootRequest, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Arc;

/// DTO validation hook used by validating route helpers and controller macros.
pub trait Validate {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

/// Field metadata used by Nest-style whitelist validation options.
pub trait ValidationSchema {
    fn allowed_fields() -> &'static [&'static str];
}

/// Options for route-level DTO validation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ValidationOptions {
    pub whitelist: bool,
    pub forbid_non_whitelisted: bool,
}

impl ValidationOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn whitelist(mut self, enabled: bool) -> Self {
        self.whitelist = enabled;
        self
    }

    pub fn forbid_non_whitelisted(mut self, enabled: bool) -> Self {
        self.forbid_non_whitelisted = enabled;
        self
    }

    pub fn merge(self, other: Self) -> Self {
        Self {
            whitelist: self.whitelist || other.whitelist,
            forbid_non_whitelisted: self.forbid_non_whitelisted || other.forbid_non_whitelisted,
        }
    }

    pub(crate) fn checks_unknown_fields(self) -> bool {
        self.whitelist || self.forbid_non_whitelisted
    }
}

pub(crate) type RequestValidator =
    Arc<dyn Fn(BootRequest, ValidationOptions) -> Result<BootRequest> + Send + Sync>;

pub(crate) fn body_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request, _| {
        request.require_json_content_type()?;
        validate_value(request.json::<T>()?).map(|_| request)
    })
}

pub(crate) fn body_validator_with_options<T>(options: ValidationOptions) -> RequestValidator
where
    T: DeserializeOwned + Validate + ValidationSchema + 'static,
{
    Arc::new(move |mut request, inherited_options| {
        let options = inherited_options.merge(options);
        request.require_json_content_type()?;
        let value = request.json::<Value>()?;
        let value = validate_json_value_with_options::<T>(value, options, "body property")?;
        if options.whitelist {
            let body =
                serde_json::to_vec(&value).map_err(|err| BootError::Internal(err.to_string()))?;
            rewrite_request_body(&mut request, body);
        }
        Ok(request)
    })
}

pub(crate) fn params_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request, _| validate_value(request.params::<T>()?).map(|_| request))
}

pub(crate) fn params_validator_with_options<T>(options: ValidationOptions) -> RequestValidator
where
    T: DeserializeOwned + Validate + ValidationSchema + 'static,
{
    Arc::new(move |mut request, inherited_options| {
        let options = inherited_options.merge(options);
        apply_map_field_options::<T>(&mut request.params, options, "path parameter")?;
        validate_value(request.params::<T>()?)?;
        Ok(request)
    })
}

pub(crate) fn query_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request, _| validate_value(request.query::<T>()?).map(|_| request))
}

pub(crate) fn query_validator_with_options<T>(options: ValidationOptions) -> RequestValidator
where
    T: DeserializeOwned + Validate + ValidationSchema + 'static,
{
    Arc::new(move |mut request, inherited_options| {
        let options = inherited_options.merge(options);
        let pairs = request.query_pairs()?;
        apply_pair_field_options::<T>(&pairs, &mut request.query, options, "query parameter")?;
        if options.whitelist {
            request.query_string = None;
        }
        validate_value(request.query::<T>()?)?;
        Ok(request)
    })
}

pub(crate) fn validate_value<T>(value: T) -> Result<T>
where
    T: Validate,
{
    value
        .validate()
        .map_err(|error| validation_bad_request(error, std::any::type_name::<T>()))?;
    Ok(value)
}

fn validation_bad_request(error: BootError, type_name: &'static str) -> BootError {
    match error {
        BootError::BadRequest(message) => {
            BootError::BadRequest(format!("validation failed for {type_name}: {message}"))
        }
        error => error,
    }
}

pub(crate) fn validate_json_value_with_options<T>(
    value: Value,
    options: ValidationOptions,
    label: &'static str,
) -> Result<Value>
where
    T: DeserializeOwned + Validate + ValidationSchema,
{
    let value = apply_json_field_options::<T>(value, options, label)?;
    validate_value(
        serde_json::from_value::<T>(value.clone())
            .map_err(|err| BootError::BadRequest(err.to_string()))?,
    )?;
    Ok(value)
}

fn apply_json_field_options<T>(
    mut value: Value,
    options: ValidationOptions,
    label: &'static str,
) -> Result<Value>
where
    T: ValidationSchema,
{
    if !options.checks_unknown_fields() {
        return Ok(value);
    }

    let Some(object) = value.as_object_mut() else {
        return Ok(value);
    };
    let allowed = allowed_fields::<T>();
    let unknown = object
        .keys()
        .filter(|field| !allowed.contains(field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    handle_unknown_fields(label, &unknown, options)?;
    if options.whitelist {
        object.retain(|field, _| allowed.contains(field.as_str()));
    }
    Ok(value)
}

fn apply_map_field_options<T>(
    values: &mut BTreeMap<String, String>,
    options: ValidationOptions,
    label: &'static str,
) -> Result<()>
where
    T: ValidationSchema,
{
    if !options.checks_unknown_fields() {
        return Ok(());
    }

    let allowed = allowed_fields::<T>();
    let unknown = values
        .keys()
        .filter(|field| !allowed.contains(field.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    handle_unknown_fields(label, &unknown, options)?;
    if options.whitelist {
        values.retain(|field, _| allowed.contains(field.as_str()));
    }
    Ok(())
}

fn apply_pair_field_options<T>(
    pairs: &[(String, String)],
    values: &mut BTreeMap<String, String>,
    options: ValidationOptions,
    label: &'static str,
) -> Result<()>
where
    T: ValidationSchema,
{
    if !options.checks_unknown_fields() {
        return Ok(());
    }

    let allowed = allowed_fields::<T>();
    let unknown = pairs
        .iter()
        .map(|(field, _)| field)
        .filter(|field| !allowed.contains(field.as_str()))
        .cloned()
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>();
    handle_unknown_fields(label, &unknown, options)?;
    if options.whitelist {
        values.retain(|field, _| allowed.contains(field.as_str()));
    }
    Ok(())
}

fn allowed_fields<T>() -> BTreeSet<&'static str>
where
    T: ValidationSchema,
{
    T::allowed_fields().iter().copied().collect()
}

fn handle_unknown_fields(
    label: &'static str,
    unknown: &[String],
    options: ValidationOptions,
) -> Result<()> {
    if unknown.is_empty() || !options.forbid_non_whitelisted {
        return Ok(());
    }

    let fields = unknown.join(", ");
    Err(BootError::BadRequest(format!(
        "non-whitelisted {}: {fields}",
        plural_label(label)
    )))
}

fn plural_label(label: &'static str) -> &'static str {
    match label {
        "body property" => "body properties",
        "message property" => "message properties",
        "path parameter" => "path parameters",
        "query parameter" => "query parameters",
        value => value,
    }
}

fn rewrite_request_body(request: &mut BootRequest, body: Vec<u8>) {
    let body_len = body.len();
    request.body = body;
    if request.header("content-length").is_some() {
        request
            .headers
            .insert("content-length".to_string(), body_len.to_string());
        request
            .appended_headers
            .retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
    }
}
