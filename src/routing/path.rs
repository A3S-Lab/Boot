use crate::{BootError, Result};
use std::collections::BTreeMap;

pub(crate) fn validate_route_path(path: &str) -> Result<()> {
    if path.starts_with('/') {
        Ok(())
    } else {
        Err(BootError::InvalidRoutePath(path.to_string()))
    }
}

pub(super) fn normalize_prefix(prefix: &str) -> Result<String> {
    if prefix.is_empty() || prefix == "/" {
        return Ok(String::new());
    }
    validate_route_path(prefix)?;
    Ok(prefix.trim_end_matches('/').to_string())
}

pub(super) fn join_paths(prefix: &str, path: &str) -> Result<String> {
    validate_route_path(path)?;
    let prefix = normalize_prefix(prefix)?;
    let path = path.trim_start_matches('/');

    if prefix.is_empty() {
        return Ok(if path.is_empty() {
            "/".to_string()
        } else {
            format!("/{path}")
        });
    }

    Ok(if path.is_empty() {
        prefix
    } else {
        format!("{prefix}/{path}")
    })
}

pub(super) fn extract_path_params(pattern: &str, path: &str) -> BTreeMap<String, String> {
    let pattern_segments = split_path(pattern);
    let path_segments = split_path(path);
    let mut params = BTreeMap::new();

    if pattern_segments.len() != path_segments.len() {
        return params;
    }

    for (pattern, value) in pattern_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = route_param_name(pattern) {
            params.insert(name.to_string(), (*value).to_string());
        } else if pattern != value {
            return BTreeMap::new();
        }
    }

    params
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect()
}

fn route_param_name(segment: &str) -> Option<&str> {
    segment
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .filter(|name| !name.is_empty())
}
