use super::header::parse_cookie_header_values;
use super::request::{parse_optional_value, parse_required_value, parse_value, BootRequest};
use crate::{BootError, Result};
use std::collections::BTreeMap;
use std::fmt;
use std::str::FromStr;

impl BootRequest {
    pub fn cookie_pairs(&self) -> Result<Vec<(String, String)>> {
        parse_cookie_header_values(&self.header_values("cookie"))
    }

    pub fn cookie(&self, name: &str) -> Result<Option<String>> {
        Ok(self
            .cookie_pairs()?
            .into_iter()
            .find_map(|(key, value)| (key == name).then_some(value)))
    }

    pub fn require_cookie(&self, name: &str) -> Result<String> {
        self.cookie(name)?
            .ok_or_else(|| BootError::Unauthorized(format!("missing cookie: {name}")))
    }

    pub fn cookie_as<T>(&self, name: &str) -> Result<T>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_required_value(self.cookie(name)?, "cookie", name)
    }

    pub fn optional_cookie_as<T>(&self, name: &str) -> Result<Option<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        parse_optional_value(self.cookie(name)?, "cookie", name)
    }

    pub fn cookie_values(&self, name: &str) -> Result<Vec<String>> {
        Ok(self
            .cookie_pairs()?
            .into_iter()
            .filter_map(|(key, value)| (key == name).then_some(value))
            .collect())
    }

    pub fn cookie_values_as<T>(&self, name: &str) -> Result<Vec<T>>
    where
        T: FromStr,
        T::Err: fmt::Display,
    {
        self.cookie_values(name)?
            .into_iter()
            .map(|value| parse_value(value, "cookie", name))
            .collect()
    }

    pub fn cookies(&self) -> Result<BTreeMap<String, String>> {
        let mut cookies = BTreeMap::new();
        for (name, value) in self.cookie_pairs()? {
            cookies.entry(name).or_insert(value);
        }
        Ok(cookies)
    }
}
