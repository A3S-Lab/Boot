use crate::percent::decode_percent_encoded;
use crate::{BootError, Result};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn validate_route_path(path: &str) -> Result<()> {
    if !path.starts_with('/') || path.contains(['?', '#']) {
        return Err(BootError::InvalidRoutePath(path.to_string()));
    }

    let mut params = BTreeSet::new();
    for segment in split_path(path) {
        validate_route_segment(path, segment, &mut params)?;
    }

    Ok(())
}

pub(super) fn normalize_prefix(prefix: &str) -> Result<String> {
    if prefix.is_empty() || prefix == "/" {
        return Ok(String::new());
    }
    validate_route_path(prefix)?;
    Ok(prefix.trim_end_matches('/').to_string())
}

pub(crate) fn join_paths(prefix: &str, path: &str) -> Result<String> {
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

    let joined = if path.is_empty() {
        prefix
    } else {
        format!("{prefix}/{path}")
    };
    validate_route_path(&joined)?;
    Ok(joined)
}

pub(crate) fn match_path_params(
    pattern: &str,
    path: &str,
) -> Result<Option<BTreeMap<String, String>>> {
    let pattern_segments = split_path(pattern);
    let path_segments = split_path(path);
    let mut params = BTreeMap::new();

    if pattern_segments.len() != path_segments.len() {
        return Ok(None);
    }

    for (pattern, value) in pattern_segments.iter().zip(path_segments.iter()) {
        if let Some(name) = route_param_name(pattern) {
            params.insert(name.to_string(), decode_path_param(value)?);
        } else if pattern != value {
            return Ok(None);
        }
    }

    Ok(Some(params))
}

pub(crate) fn match_path_shape(pattern: &str, path: &str) -> bool {
    let pattern_segments = split_path(pattern);
    let path_segments = split_path(path);

    if pattern_segments.len() != path_segments.len() {
        return false;
    }

    pattern_segments
        .iter()
        .zip(path_segments.iter())
        .all(|(pattern, value)| route_param_name(pattern).is_some() || pattern == value)
}

pub(crate) fn route_shape_key(path: &str) -> String {
    let segments = split_path(path)
        .into_iter()
        .map(|segment| {
            if route_param_name(segment).is_some() {
                "{}"
            } else {
                segment
            }
        })
        .collect::<Vec<_>>();

    if segments.is_empty() {
        "/".to_string()
    } else {
        format!("/{}", segments.join("/"))
    }
}

pub(super) fn route_param_names(path: &str) -> Vec<&str> {
    split_path(path)
        .into_iter()
        .filter_map(route_param_name)
        .collect()
}

pub(super) fn route_specificity(path: &str) -> Vec<u8> {
    split_path(path)
        .into_iter()
        .map(|segment| u8::from(route_param_name(segment).is_none()))
        .collect()
}

fn split_path(path: &str) -> Vec<&str> {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        Vec::new()
    } else {
        path.split('/').collect()
    }
}

fn route_param_name(segment: &str) -> Option<&str> {
    segment
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .filter(|name| !name.is_empty() && !name.contains(['{', '}']))
}

fn validate_route_segment<'a>(
    path: &str,
    segment: &'a str,
    params: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if !segment.contains(['{', '}']) {
        return Ok(());
    }

    let Some(name) = route_param_name(segment) else {
        return Err(BootError::InvalidRoutePath(path.to_string()));
    };

    if !params.insert(name) {
        return Err(BootError::InvalidRoutePath(path.to_string()));
    }

    Ok(())
}

fn decode_path_param(value: &str) -> Result<String> {
    decode_percent_encoded(value)
}
