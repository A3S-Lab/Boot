use crate::{BootError, BootRequest, Result};
use serde::de::DeserializeOwned;
use std::sync::Arc;

/// DTO validation hook used by validating route helpers and controller macros.
pub trait Validate {
    fn validate(&self) -> Result<()> {
        Ok(())
    }
}

pub(crate) type RequestValidator = Arc<dyn Fn(&BootRequest) -> Result<()> + Send + Sync>;

pub(crate) fn body_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request| {
        request.require_json_content_type()?;
        validate_value(request.json::<T>()?).map(|_| ())
    })
}

pub(crate) fn params_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request| validate_value(request.params::<T>()?).map(|_| ()))
}

pub(crate) fn query_validator<T>() -> RequestValidator
where
    T: DeserializeOwned + Validate + 'static,
{
    Arc::new(|request| validate_value(request.query::<T>()?).map(|_| ()))
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
