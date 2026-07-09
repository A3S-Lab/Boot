use crate::HttpMethod;
use std::collections::BTreeSet;

pub(crate) fn collect_http_methods<I>(methods: I) -> BTreeSet<HttpMethod>
where
    I: IntoIterator<Item = HttpMethod>,
{
    let mut collected = BTreeSet::new();
    for method in methods {
        insert_http_method(&mut collected, method);
    }
    collected
}

pub(crate) fn insert_http_method(methods: &mut BTreeSet<HttpMethod>, method: HttpMethod) {
    if method.is_wildcard() {
        methods.extend(HttpMethod::standard_methods().iter().copied());
    } else {
        methods.insert(method);
    }
}
