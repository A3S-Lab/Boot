use quote::quote;
use syn::{Attribute, LitStr};

use super::is_attribute_named;

pub(in crate::controller) fn take_controller_host_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerHostAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut host = ControllerHostAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "host") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(pattern) => {
                if host.pattern.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "controller impl blocks can use at most one #[host] attribute",
                    ));
                } else {
                    host.pattern = Some(pattern);
                }
            }
            Err(_) => errors.push(syn::Error::new_spanned(
                attr,
                "#[host] requires one string literal argument",
            )),
        }
    }

    (clean_attrs, host, errors)
}

pub(in crate::controller) fn take_route_host_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<HostSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "host") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(pattern) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one #[host] attribute",
                    ));
                } else {
                    spec = Some(HostSpec { pattern });
                }
            }
            Err(_) => errors.push(syn::Error::new_spanned(
                attr,
                "#[host] requires one string literal argument",
            )),
        }
    }

    (clean_attrs, spec, errors)
}

#[derive(Default)]
pub(in crate::controller) struct ControllerHostAttrs {
    pattern: Option<LitStr>,
}

impl ControllerHostAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.pattern
            .iter()
            .map(|pattern| quote!(with_host(#pattern)))
            .collect()
    }
}

pub(in crate::controller) struct HostSpec {
    pattern: LitStr,
}

impl HostSpec {
    pub(in crate::controller) fn token(&self) -> proc_macro2::TokenStream {
        let pattern = &self.pattern;
        quote!(with_host(#pattern))
    }
}
