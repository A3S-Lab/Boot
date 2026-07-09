use quote::quote;
use syn::{Attribute, LitStr};

use crate::openapi::{
    ApiExtensionArgs, ApiExtraModelArgs, AttrKind as OpenApiAttrKind, RouteSpec as RouteOpenApiSpec,
};

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
            OpenApiAttrKind::ApiExtraModel => match attr.parse_args::<ApiExtraModelArgs>() {
                Ok(extra_model) => openapi.extra_models.push(extra_model),
                Err(error) => errors.push(error),
            },
            OpenApiAttrKind::ApiExtension => match attr.parse_args::<ApiExtensionArgs>() {
                Ok(extension) => openapi.extensions.push(extension),
                Err(error) => errors.push(error),
            },
            OpenApiAttrKind::HideFromOpenApi => {
                if let Err(error) = crate::expect_no_extractor_args(attr, "hide_from_openapi") {
                    errors.push(error);
                } else {
                    openapi.hidden = true;
                }
            }
            _ => errors.push(syn::Error::new_spanned(
                attr,
                "only #[tag(\"name\")], #[api_extra_model(...)], #[api_extension(...)], and #[hide_from_openapi] are supported on #[controller] impl blocks",
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
    extra_models: Vec<ApiExtraModelArgs>,
    extensions: Vec<ApiExtensionArgs>,
    hidden: bool,
}

impl ControllerOpenApiAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        let mut tokens = self
            .tags
            .iter()
            .map(|tag| quote!(with_tag(#tag)))
            .chain(self.extra_models.iter().map(ApiExtraModelArgs::tokens))
            .chain(self.extensions.iter().map(ApiExtensionArgs::tokens))
            .collect::<Vec<_>>();
        if self.hidden {
            tokens.push(quote!(hide_from_openapi()));
        }
        tokens
    }
}
