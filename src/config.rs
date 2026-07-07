use crate::{BootError, Module, ProviderDefinition, ProviderToken, Result, Validate};
use a3s_acl::{Block, Document, Value};
use serde::de::DeserializeOwned;
use serde_json::{Map, Number};
use std::fmt;
use std::path::Path;
use std::sync::Arc;

/// ACL-backed configuration module that exposes a typed config value as a provider.
pub struct ConfigModule<T>
where
    T: Send + Sync + 'static,
{
    name: &'static str,
    token: ProviderToken,
    value: Arc<T>,
    global: bool,
}

impl<T> Clone for ConfigModule<T>
where
    T: Send + Sync + 'static,
{
    fn clone(&self) -> Self {
        Self {
            name: self.name,
            token: self.token.clone(),
            value: Arc::clone(&self.value),
            global: self.global,
        }
    }
}

impl<T> fmt::Debug for ConfigModule<T>
where
    T: Send + Sync + 'static,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfigModule")
            .field("name", &self.name)
            .field("token", &self.token)
            .field("global", &self.global)
            .finish_non_exhaustive()
    }
}

impl<T> ConfigModule<T>
where
    T: Send + Sync + 'static,
{
    pub fn from_value(name: &'static str, value: T) -> Self {
        Self::from_arc(name, Arc::new(value))
    }

    pub fn from_arc(name: &'static str, value: Arc<T>) -> Self {
        Self {
            name,
            token: ProviderToken::of::<T>(),
            value,
            global: false,
        }
    }

    pub fn named(mut self, token: impl Into<String>) -> Self {
        self.token = ProviderToken::named(token);
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }
}

impl<T> ConfigModule<T>
where
    T: DeserializeOwned + Send + Sync + 'static,
{
    pub fn from_acl_str(name: &'static str, input: &str) -> Result<Self> {
        parse_acl_config(input).map(|value| Self::from_value(name, value))
    }

    pub fn from_acl_file(name: &'static str, path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        let input = std::fs::read_to_string(path).map_err(|error| {
            BootError::Internal(format!(
                "failed to read ACL config file {}: {error}",
                path.display()
            ))
        })?;
        Self::from_acl_str(name, &input)
    }

    pub fn from_acl_document(name: &'static str, document: &Document) -> Result<Self> {
        config_from_acl_document(document).map(|value| Self::from_value(name, value))
    }
}

impl<T> ConfigModule<T>
where
    T: DeserializeOwned + Validate + Send + Sync + 'static,
{
    pub fn from_validated_acl_str(name: &'static str, input: &str) -> Result<Self> {
        parse_validated_acl_config(input).map(|value| Self::from_value(name, value))
    }

    pub fn from_validated_acl_file(name: &'static str, path: impl AsRef<Path>) -> Result<Self> {
        let module = Self::from_acl_file(name, path)?;
        module.value.validate()?;
        Ok(module)
    }

    pub fn from_validated_acl_document(name: &'static str, document: &Document) -> Result<Self> {
        let module = Self::from_acl_document(name, document)?;
        module.value.validate()?;
        Ok(module)
    }
}

impl<T> Module for ConfigModule<T>
where
    T: Send + Sync + 'static,
{
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::named_from_arc(
            self.token.as_str(),
            Arc::clone(&self.value),
        )])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![self.token.clone()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}

/// Parse ACL text into a typed configuration value.
pub fn parse_acl_config<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned,
{
    let document = a3s_acl::parse(input)
        .map_err(|error| BootError::BadRequest(format!("invalid ACL config: {error}")))?;
    config_from_acl_document(&document)
}

/// Parse ACL text into a typed configuration value and run its validator.
pub fn parse_validated_acl_config<T>(input: &str) -> Result<T>
where
    T: DeserializeOwned + Validate,
{
    let value = parse_acl_config::<T>(input)?;
    value.validate()?;
    Ok(value)
}

/// Convert an ACL document to a JSON value suitable for serde deserialization.
pub fn acl_document_to_json(document: &Document) -> Result<serde_json::Value> {
    let mut root = Map::new();
    for block in &document.blocks {
        insert_block(&mut root, block)?;
    }
    Ok(serde_json::Value::Object(root))
}

fn config_from_acl_document<T>(document: &Document) -> Result<T>
where
    T: DeserializeOwned,
{
    let value = acl_document_to_json(document)?;
    serde_json::from_value(value)
        .map_err(|error| BootError::BadRequest(format!("invalid ACL config shape: {error}")))
}

fn insert_block(target: &mut Map<String, serde_json::Value>, block: &Block) -> Result<()> {
    if let Some((key, value)) = bare_attribute(block) {
        target.insert(key.to_string(), acl_value_to_json(value)?);
        return Ok(());
    }

    let value = block_body_to_json(block)?;
    if let Some(label) = block.labels.first() {
        let entry = target
            .entry(block.name.clone())
            .or_insert_with(|| serde_json::Value::Object(Map::new()));
        let serde_json::Value::Object(labels) = entry else {
            return Err(BootError::BadRequest(format!(
                "ACL block {} cannot mix labeled and unlabeled entries",
                block.name
            )));
        };
        insert_repeated(labels, label.clone(), value);
    } else {
        insert_repeated(target, block.name.clone(), value);
    }
    Ok(())
}

fn bare_attribute(block: &Block) -> Option<(&str, &Value)> {
    if block.labels.is_empty() && block.blocks.is_empty() && block.attributes.len() == 1 {
        return block
            .attributes
            .get(&block.name)
            .map(|value| (block.name.as_str(), value));
    }
    None
}

fn block_body_to_json(block: &Block) -> Result<serde_json::Value> {
    let mut object = Map::new();

    if block.labels.len() > 1 {
        object.insert(
            "labels".to_string(),
            serde_json::Value::Array(
                block
                    .labels
                    .iter()
                    .skip(1)
                    .cloned()
                    .map(serde_json::Value::String)
                    .collect(),
            ),
        );
    }

    for (key, value) in &block.attributes {
        object.insert(key.clone(), acl_value_to_json(value)?);
    }

    for nested in &block.blocks {
        insert_block(&mut object, nested)?;
    }

    Ok(serde_json::Value::Object(object))
}

fn insert_repeated(
    target: &mut Map<String, serde_json::Value>,
    key: String,
    value: serde_json::Value,
) {
    match target.remove(&key) {
        None => {
            target.insert(key, value);
        }
        Some(serde_json::Value::Array(mut values)) => {
            values.push(value);
            target.insert(key, serde_json::Value::Array(values));
        }
        Some(existing) => {
            target.insert(key, serde_json::Value::Array(vec![existing, value]));
        }
    }
}

fn acl_value_to_json(value: &Value) -> Result<serde_json::Value> {
    match value {
        Value::String(value) => Ok(serde_json::Value::String(value.clone())),
        Value::Number(value) => number_to_json(*value),
        Value::Bool(value) => Ok(serde_json::Value::Bool(*value)),
        Value::List(values) => values
            .iter()
            .map(acl_value_to_json)
            .collect::<Result<Vec<_>>>()
            .map(serde_json::Value::Array),
        Value::Object(values) => {
            let mut object = Map::new();
            for (key, value) in values {
                object.insert(key.clone(), acl_value_to_json(value)?);
            }
            Ok(serde_json::Value::Object(object))
        }
        Value::Null => Ok(serde_json::Value::Null),
        Value::Call(name, args) => evaluate_call(name, args),
    }
}

fn number_to_json(value: f64) -> Result<serde_json::Value> {
    if value.fract() == 0.0 && value >= i64::MIN as f64 && value <= i64::MAX as f64 {
        return Ok(serde_json::Value::Number(Number::from(value as i64)));
    }

    Number::from_f64(value)
        .map(serde_json::Value::Number)
        .ok_or_else(|| BootError::BadRequest(format!("invalid ACL number: {value}")))
}

fn evaluate_call(name: &str, args: &[Value]) -> Result<serde_json::Value> {
    match name {
        "env" => evaluate_env(args),
        "concat" => evaluate_concat(args),
        _ => Err(BootError::BadRequest(format!(
            "unsupported ACL config function: {name}"
        ))),
    }
}

fn evaluate_env(args: &[Value]) -> Result<serde_json::Value> {
    let name = string_arg("env", args, 0)?;
    match std::env::var(name) {
        Ok(value) => Ok(serde_json::Value::String(value)),
        Err(_) if args.len() == 2 => acl_value_to_json(&args[1]),
        Err(_) => Err(BootError::BadRequest(format!(
            "missing environment variable for ACL config: {name}"
        ))),
    }
}

fn evaluate_concat(args: &[Value]) -> Result<serde_json::Value> {
    let mut output = String::new();
    for arg in args {
        match acl_value_to_json(arg)? {
            serde_json::Value::String(value) => output.push_str(&value),
            serde_json::Value::Number(value) => output.push_str(&value.to_string()),
            serde_json::Value::Bool(value) => output.push_str(if value { "true" } else { "false" }),
            serde_json::Value::Null => {}
            other => {
                return Err(BootError::BadRequest(format!(
                    "concat ACL config function only accepts scalar values, got {other}"
                )));
            }
        }
    }
    Ok(serde_json::Value::String(output))
}

fn string_arg<'a>(function: &str, args: &'a [Value], index: usize) -> Result<&'a str> {
    let Some(value) = args.get(index) else {
        return Err(BootError::BadRequest(format!(
            "{function} ACL config function is missing argument {}",
            index + 1
        )));
    };

    value.as_str().ok_or_else(|| {
        BootError::BadRequest(format!(
            "{function} ACL config function argument {} must be a string",
            index + 1
        ))
    })
}
