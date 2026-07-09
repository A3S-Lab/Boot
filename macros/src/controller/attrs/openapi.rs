use quote::quote;
use syn::{Attribute, LitStr};

use crate::openapi::{AttrKind as OpenApiAttrKind, RouteSpec as RouteOpenApiSpec};

pub(in crate::controller) fn take_controller_openapi_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerOpenApiAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut openapi = ControllerOpenApiAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = OpenApiAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            OpenApiAttrKind::Tag => match attr.parse_args::<LitStr>() {
                Ok(tag) => openapi.tags.push(tag),
                Err(error) => errors.push(error),
            },
            _ => errors.push(syn::Error::new_spanned(
                attr,
                "only #[tag(\"name\")] is supported on #[controller] impl blocks",
            )),
        }
    }

    (clean_attrs, openapi, errors)
}

pub(in crate::controller) fn take_route_openapi_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<RouteOpenApiSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = OpenApiAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind.parse_route_spec(attr) {
            Ok(spec) => specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

#[derive(Default)]
pub(in crate::controller) struct ControllerOpenApiAttrs {
    tags: Vec<LitStr>,
}

impl ControllerOpenApiAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.tags.iter().map(|tag| quote!(with_tag(#tag))).collect()
    }
}
