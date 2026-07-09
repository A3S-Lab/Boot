use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Expr, LitStr, Result, Token};

use super::is_attribute_named;
use crate::parse_optional_comma;

pub(in crate::controller) fn take_controller_metadata_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerMetadataAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut metadata = ControllerMetadataAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "metadata") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<MetadataSpec>() {
            Ok(spec) => metadata.specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, metadata, errors)
}

pub(in crate::controller) fn take_route_metadata_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<MetadataSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "metadata") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<MetadataSpec>() {
            Ok(spec) => specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

#[derive(Default)]
pub(in crate::controller) struct ControllerMetadataAttrs {
    specs: Vec<MetadataSpec>,
}

impl ControllerMetadataAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs
            .iter()
            .map(|spec| {
                let key = &spec.key;
                let value = &spec.value;
                quote!(with_metadata(#key, #value))
            })
            .collect()
    }
}

#[derive(Clone)]
pub(in crate::controller) struct MetadataSpec {
    pub(in crate::controller) key: LitStr,
    pub(in crate::controller) value: Expr,
}

impl Parse for MetadataSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let key = input.parse::<LitStr>()?;
        input.parse::<Token![,]>()?;
        let value = input.parse::<Expr>()?;
        parse_optional_comma(input)?;
        Ok(Self { key, value })
    }
}
