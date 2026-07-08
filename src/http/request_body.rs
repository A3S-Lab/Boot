use super::request::BootRequest;
use crate::{BootError, Result};
use serde::de::DeserializeOwned;
use serde_json::Value;

impl BootRequest {
    pub fn body_field(&self, name: &str) -> Result<Option<Value>> {
        let body = self.json::<Value>()?;
        let Value::Object(fields) = body else {
            return Err(BootError::BadRequest(
                "expected JSON object body".to_string(),
            ));
        };

        Ok(fields.get(name).filter(|value| !value.is_null()).cloned())
    }

    pub fn body_field_as<T>(&self, name: &str) -> Result<T>
    where
        T: DeserializeOwned,
    {
        let Some(value) = self.body_field(name)? else {
            return Err(BootError::BadRequest(format!("missing body field: {name}")));
        };
        deserialize_body_field(name, value)
    }

    pub fn optional_body_field_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: DeserializeOwned,
    {
        self.body_field(name)?
            .map(|value| deserialize_body_field(name, value))
            .transpose()
    }

    pub fn body_field_string(&self, name: &str) -> Result<String> {
        let Some(value) = self.body_field(name)? else {
            return Err(BootError::BadRequest(format!("missing body field: {name}")));
        };
        body_field_value_to_string(value)
    }

    pub fn optional_body_field_string(&self, name: &str) -> Result<Option<String>> {
        self.body_field(name)?
            .map(body_field_value_to_string)
            .transpose()
    }
}

fn deserialize_body_field<T>(name: &str, value: Value) -> Result<T>
where
    T: DeserializeOwned,
{
    serde_json::from_value(value)
        .map_err(|error| BootError::BadRequest(format!("invalid body field {name}: {error}")))
}

fn body_field_value_to_string(value: Value) -> Result<String> {
    match value {
        Value::String(value) => Ok(value),
        Value::Bool(value) => Ok(value.to_string()),
        Value::Number(value) => Ok(value.to_string()),
        Value::Array(_) | Value::Object(_) => {
            serde_json::to_string(&value).map_err(|error| BootError::BadRequest(error.to_string()))
        }
        Value::Null => Ok("null".to_string()),
    }
}
