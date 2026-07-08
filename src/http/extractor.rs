use super::request::BootRequest;
use crate::{BootError, Result};
use std::fmt;

/// Custom request value extractor used by Nest-style controller argument binding.
pub trait RequestExtractor<T>: Send + Sync + 'static {
    fn extract(&self, request: &BootRequest) -> Result<T>;
}

impl<T, F> RequestExtractor<T> for F
where
    F: Fn(&BootRequest) -> Result<T> + Send + Sync + 'static,
{
    fn extract(&self, request: &BootRequest) -> Result<T> {
        self(request)
    }
}

pub fn extract_request_value<T, E>(request: &BootRequest, extractor: E) -> Result<T>
where
    E: RequestExtractor<T>,
{
    extractor.extract(request)
}

/// Transforms a single request value extracted from a path, query, header, or host parameter.
pub trait RequestValuePipe<I, O>: Send + Sync + 'static {
    fn transform(&self, value: I) -> Result<O>;
}

impl<I, O, F> RequestValuePipe<I, O> for F
where
    F: Fn(I) -> Result<O> + Send + Sync + 'static,
{
    fn transform(&self, value: I) -> Result<O> {
        self(value)
    }
}

pub fn transform_request_value<I, O, P>(value: I, pipe: P) -> Result<O>
where
    P: RequestValuePipe<I, O>,
{
    pipe.transform(value)
}

/// Built-in Nest-style pipe that parses integer request values.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseIntPipe;

impl<T> RequestValuePipe<String, T> for ParseIntPipe
where
    T: ParseIntTarget,
{
    fn transform(&self, value: String) -> Result<T> {
        T::parse_int(value.trim()).map_err(|error| {
            BootError::BadRequest(format!(
                "validation failed: numeric string is expected for {}: {error}",
                T::target_name()
            ))
        })
    }
}

/// Built-in Nest-style pipe that parses boolean request values.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseBoolPipe;

impl RequestValuePipe<String, bool> for ParseBoolPipe {
    fn transform(&self, value: String) -> Result<bool> {
        match value.trim().to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(true),
            "false" | "0" => Ok(false),
            _ => Err(BootError::BadRequest(
                "validation failed: boolean string is expected".to_string(),
            )),
        }
    }
}

/// Built-in Nest-style pipe that parses floating point request values.
#[derive(Debug, Clone, Copy, Default)]
pub struct ParseFloatPipe;

impl<T> RequestValuePipe<String, T> for ParseFloatPipe
where
    T: ParseFloatTarget,
{
    fn transform(&self, value: String) -> Result<T> {
        T::parse_float(value.trim()).map_err(|error| {
            BootError::BadRequest(format!(
                "validation failed: numeric string is expected for {}: {error}",
                T::target_name()
            ))
        })
    }
}

/// Built-in Nest-style pipe that replaces missing optional values with a default.
#[derive(Debug, Clone)]
pub struct DefaultValuePipe<T> {
    value: T,
}

impl<T> DefaultValuePipe<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn value(&self) -> &T {
        &self.value
    }

    pub fn into_value(self) -> T {
        self.value
    }
}

impl<T> RequestValuePipe<Option<T>, T> for DefaultValuePipe<T>
where
    T: Clone + Send + Sync + 'static,
{
    fn transform(&self, value: Option<T>) -> Result<T> {
        Ok(value.unwrap_or_else(|| self.value.clone()))
    }
}

pub trait ParseIntTarget: Sized + Send + Sync + 'static {
    fn parse_int(value: &str) -> std::result::Result<Self, String>;
    fn target_name() -> &'static str;
}

pub trait ParseFloatTarget: Sized + Send + Sync + 'static {
    fn parse_float(value: &str) -> std::result::Result<Self, String>;
    fn target_name() -> &'static str;
}

macro_rules! impl_parse_int_target {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ParseIntTarget for $ty {
                fn parse_int(value: &str) -> std::result::Result<Self, String> {
                    value.parse::<$ty>().map_err(display_error)
                }

                fn target_name() -> &'static str {
                    stringify!($ty)
                }
            }
        )*
    };
}

macro_rules! impl_parse_float_target {
    ($($ty:ty),* $(,)?) => {
        $(
            impl ParseFloatTarget for $ty {
                fn parse_float(value: &str) -> std::result::Result<Self, String> {
                    value.parse::<$ty>().map_err(display_error)
                }

                fn target_name() -> &'static str {
                    stringify!($ty)
                }
            }
        )*
    };
}

impl_parse_int_target!(i8, i16, i32, i64, i128, isize, u8, u16, u32, u64, u128, usize,);
impl_parse_float_target!(f32, f64);

fn display_error(error: impl fmt::Display) -> String {
    error.to_string()
}
