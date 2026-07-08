use crate::{BootError, Result};
use std::collections::{BTreeMap, BTreeSet};

pub(crate) fn validate_host_pattern(pattern: &str) -> Result<()> {
    if pattern.trim().is_empty()
        || pattern.contains(['/', '?', '#'])
        || pattern.chars().any(char::is_whitespace)
    {
        return Err(BootError::InvalidHostPattern(pattern.to_string()));
    }

    let mut params = BTreeSet::new();
    for label in split_host(pattern) {
        validate_host_label(pattern, label, &mut params)?;
    }

    Ok(())
}

pub(crate) fn match_host_params(
    pattern: &str,
    host: &str,
) -> Result<Option<BTreeMap<String, String>>> {
    validate_host_pattern(pattern)?;
    let pattern_labels = split_host(pattern);
    let host = normalize_host_header(host).unwrap_or(host);
    let host_labels = split_host(host);
    let mut params = BTreeMap::new();

    if pattern_labels.len() != host_labels.len() {
        return Ok(None);
    }

    for (pattern, value) in pattern_labels.iter().zip(host_labels.iter()) {
        if let Some(name) = host_param_name(pattern) {
            params.insert(name.to_string(), (*value).to_string());
        } else if !pattern.eq_ignore_ascii_case(value) {
            return Ok(None);
        }
    }

    Ok(Some(params))
}

pub(crate) fn match_host_shape(pattern: &str, host: &str) -> bool {
    let pattern_labels = split_host(pattern);
    let host = normalize_host_header(host).unwrap_or(host);
    let host_labels = split_host(host);

    pattern_labels.len() == host_labels.len()
        && pattern_labels
            .iter()
            .zip(host_labels.iter())
            .all(|(pattern, value)| {
                host_param_name(pattern).is_some() || pattern.eq_ignore_ascii_case(value)
            })
}

pub(crate) fn host_shape_key(pattern: &str) -> String {
    split_host(pattern)
        .into_iter()
        .map(|label| {
            if host_param_name(label).is_some() {
                "{}".to_string()
            } else {
                label.to_ascii_lowercase()
            }
        })
        .collect::<Vec<_>>()
        .join(".")
}

pub(crate) fn host_param_names(pattern: &str) -> Vec<&str> {
    split_host(pattern)
        .into_iter()
        .filter_map(host_param_name)
        .collect()
}

pub(crate) fn host_specificity(pattern: Option<&str>) -> Vec<u8> {
    match pattern {
        Some(pattern) => split_host(pattern)
            .into_iter()
            .map(|label| u8::from(host_param_name(label).is_none()))
            .collect(),
        None => vec![0],
    }
}

pub(crate) fn normalize_host_header(value: &str) -> Option<&str> {
    let value = value.trim();
    if value.is_empty() {
        return None;
    }

    if let Some(rest) = value.strip_prefix('[') {
        let end = rest.find(']')?;
        return Some(&rest[..end]);
    }

    Some(value.split_once(':').map_or(value, |(host, _)| host)).filter(|host| !host.is_empty())
}

fn split_host(host: &str) -> Vec<&str> {
    host.trim_matches('.')
        .split('.')
        .filter(|label| !label.is_empty())
        .collect()
}

fn host_param_name(label: &str) -> Option<&str> {
    label
        .strip_prefix('{')
        .and_then(|value| value.strip_suffix('}'))
        .or_else(|| label.strip_prefix(':'))
        .filter(|name| !name.is_empty() && !name.contains(['{', '}', ':', '.']))
}

fn validate_host_label<'a>(
    pattern: &str,
    label: &'a str,
    params: &mut BTreeSet<&'a str>,
) -> Result<()> {
    if label.is_empty() {
        return Err(BootError::InvalidHostPattern(pattern.to_string()));
    }

    if !label.contains(['{', '}', ':']) {
        return Ok(());
    }

    let Some(name) = host_param_name(label) else {
        return Err(BootError::InvalidHostPattern(pattern.to_string()));
    };

    if !params.insert(name) {
        return Err(BootError::InvalidHostPattern(pattern.to_string()));
    }

    Ok(())
}
