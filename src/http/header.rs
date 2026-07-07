use crate::{BootError, Result};
use std::collections::BTreeMap;

pub(crate) fn normalize_header_name(name: impl Into<String>) -> String {
    name.into().to_ascii_lowercase()
}

pub(crate) fn normalize_headers(headers: BTreeMap<String, String>) -> BTreeMap<String, String> {
    headers
        .into_iter()
        .map(|(name, value)| (normalize_header_name(name), value))
        .collect()
}

pub(crate) fn validate_header_name(name: &str) -> std::result::Result<(), &'static str> {
    if name.is_empty() {
        return Err("header name cannot be empty");
    }

    if name.bytes().all(is_header_name_byte) {
        Ok(())
    } else {
        Err("header name contains invalid characters")
    }
}

pub(crate) fn validate_header_value(value: &str) -> std::result::Result<(), &'static str> {
    if value.bytes().all(is_header_value_byte) {
        Ok(())
    } else {
        Err("header value contains invalid characters")
    }
}

pub(crate) fn get_header<'a>(headers: &'a BTreeMap<String, String>, name: &str) -> Option<&'a str> {
    let normalized = normalize_header_name(name);
    headers
        .get(&normalized)
        .or_else(|| {
            headers
                .iter()
                .find(|(key, _)| key.eq_ignore_ascii_case(name))
                .map(|(_, value)| value)
        })
        .map(String::as_str)
}

pub(crate) fn matches_media_type(value: &str, expected: &str) -> bool {
    media_type(value).eq_ignore_ascii_case(media_type(expected))
}

pub(crate) fn is_json_media_type(value: &str) -> bool {
    let Some((_, subtype)) = media_type(value).split_once('/') else {
        return false;
    };

    subtype.eq_ignore_ascii_case("json") || subtype.to_ascii_lowercase().ends_with("+json")
}

pub(crate) fn parse_content_length(value: &str) -> Option<u64> {
    let value = value.trim();
    if value.is_empty() || !value.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }

    value.parse().ok()
}

pub(crate) fn strict_content_length_values<'a>(
    values: impl IntoIterator<Item = &'a str>,
    invalid_error: impl Fn(&str) -> BootError,
    conflicting_error: impl Fn(u64, u64) -> BootError,
) -> Result<Option<u64>> {
    let mut expected_content_length = None;
    for content_length in values {
        let parsed_content_length =
            parse_content_length(content_length).ok_or_else(|| invalid_error(content_length))?;
        if let Some(expected_content_length) = expected_content_length {
            if parsed_content_length != expected_content_length {
                return Err(conflicting_error(
                    expected_content_length,
                    parsed_content_length,
                ));
            }
        } else {
            expected_content_length = Some(parsed_content_length);
        }
    }

    Ok(expected_content_length)
}

pub(crate) fn accepts_json_response(values: &[&str]) -> bool {
    if values.is_empty() {
        return true;
    }

    let mut best_match: Option<(u8, f32)> = None;
    for value in values {
        for range in value.split(',') {
            let range = AcceptedMediaRange::parse(range);
            if let Some(specificity) = range.json_specificity() {
                let q = range.q;
                best_match = match best_match {
                    Some((best_specificity, best_q)) if best_specificity > specificity => {
                        Some((best_specificity, best_q))
                    }
                    Some((best_specificity, best_q)) if best_specificity == specificity => {
                        Some((best_specificity, best_q.max(q)))
                    }
                    _ => Some((specificity, q)),
                };
            }
        }
    }

    best_match.is_some_and(|(_, q)| q > 0.0)
}

pub(crate) fn accepts_event_stream_response(values: &[&str]) -> bool {
    if values.is_empty() {
        return true;
    }

    let mut best_match: Option<(u8, f32)> = None;
    for value in values {
        for range in value.split(',') {
            let range = AcceptedMediaRange::parse(range);
            if let Some(specificity) = range.event_stream_specificity() {
                let q = range.q;
                best_match = match best_match {
                    Some((best_specificity, best_q)) if best_specificity > specificity => {
                        Some((best_specificity, best_q))
                    }
                    Some((best_specificity, best_q)) if best_specificity == specificity => {
                        Some((best_specificity, best_q.max(q)))
                    }
                    _ => Some((specificity, q)),
                };
            }
        }
    }

    best_match.is_some_and(|(_, q)| q > 0.0)
}

pub(crate) fn parse_cookie_header_values(values: &[&str]) -> Result<Vec<(String, String)>> {
    let mut cookies = Vec::new();
    for value in values {
        for pair in value.split(';') {
            let pair = pair.trim();
            if pair.is_empty() {
                continue;
            }

            let Some((name, value)) = pair.split_once('=') else {
                return Err(BootError::BadRequest(format!(
                    "invalid cookie pair: {pair}"
                )));
            };
            let name = name.trim();
            if name.is_empty() {
                return Err(BootError::BadRequest(
                    "cookie name cannot be empty".to_string(),
                ));
            }
            cookies.push((
                name.to_string(),
                unquote_cookie_value(value.trim()).to_string(),
            ));
        }
    }
    Ok(cookies)
}

fn media_type(value: &str) -> &str {
    value.split(';').next().unwrap_or(value).trim()
}

fn is_header_name_byte(byte: u8) -> bool {
    matches!(
        byte,
        b'!' | b'#'
            | b'$'
            | b'%'
            | b'&'
            | b'\''
            | b'*'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
            | b'0'..=b'9'
            | b'a'..=b'z'
            | b'A'..=b'Z'
    )
}

fn is_header_value_byte(byte: u8) -> bool {
    matches!(byte, b'\t' | 0x20..=0x7e)
}

fn unquote_cookie_value(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .unwrap_or(value)
}

struct AcceptedMediaRange<'a> {
    value: &'a str,
    q: f32,
}

impl<'a> AcceptedMediaRange<'a> {
    fn parse(value: &'a str) -> Self {
        let mut parts = value.split(';');
        let value = parts.next().unwrap_or(value).trim();
        let q = parts
            .filter_map(|part| part.trim().split_once('='))
            .find_map(|(name, value)| name.eq_ignore_ascii_case("q").then_some(value.trim()))
            .and_then(|value| value.parse::<f32>().ok())
            .filter(|q| q.is_finite())
            .map(|q| q.clamp(0.0, 1.0))
            .unwrap_or(1.0);

        Self { value, q }
    }

    fn json_specificity(&self) -> Option<u8> {
        let (media_type, subtype) = self.value.split_once('/')?;
        if media_type == "*" && subtype == "*" {
            return Some(0);
        }

        if !media_type.eq_ignore_ascii_case("application") {
            return None;
        }

        if subtype == "*" {
            return Some(1);
        }

        if subtype.eq_ignore_ascii_case("*+json") {
            return Some(2);
        }

        (subtype.eq_ignore_ascii_case("json") || subtype.to_ascii_lowercase().ends_with("+json"))
            .then_some(3)
    }

    fn event_stream_specificity(&self) -> Option<u8> {
        let (media_type, subtype) = self.value.split_once('/')?;
        if media_type == "*" && subtype == "*" {
            return Some(0);
        }

        if !media_type.eq_ignore_ascii_case("text") {
            return None;
        }

        if subtype == "*" {
            return Some(1);
        }

        subtype.eq_ignore_ascii_case("event-stream").then_some(2)
    }
}
