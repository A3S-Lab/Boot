use crate::percent::decode_percent_encoded;
use crate::Result;
use std::collections::BTreeMap;

pub(super) fn split_path_query(
    value: String,
) -> (String, Option<String>, BTreeMap<String, String>) {
    if let Some((path, query)) = value.split_once('?') {
        (
            path.to_string(),
            Some(query.to_string()),
            parse_query(query),
        )
    } else {
        (value, None, BTreeMap::new())
    }
}

pub(super) fn parse_query(query: &str) -> BTreeMap<String, String> {
    parse_query_pairs(query)
        .map(|pairs| pairs.into_iter().collect())
        .unwrap_or_default()
}

pub(super) fn parse_query_pairs(query: &str) -> Result<Vec<(String, String)>> {
    if query.is_empty() {
        return Ok(Vec::new());
    }

    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| {
            let (name, value) = pair.split_once('=').unwrap_or((pair, ""));
            Ok((decode_query_part(name)?, decode_query_part(value)?))
        })
        .collect()
}

fn decode_query_part(value: &str) -> Result<String> {
    let value = value.replace('+', " ");
    decode_percent_encoded(&value)
}
