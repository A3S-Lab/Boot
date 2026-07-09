use syn::{Attribute, LitInt};

use super::is_attribute_named;

pub(in crate::controller) fn take_route_http_code_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<LitInt>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut status = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "http_code") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitInt>() {
            Ok(value) if status.is_none() => status = Some(value),
            Ok(value) => errors.push(syn::Error::new_spanned(
                value,
                "route methods can use at most one #[http_code(...)] attribute",
            )),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, status, errors)
}
