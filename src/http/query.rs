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
    serde_urlencoded::from_str::<BTreeMap<String, String>>(query).unwrap_or_default()
}
