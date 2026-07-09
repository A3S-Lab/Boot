use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Ident, LitBool, LitStr, Result, Token};

use super::is_attribute_named;

pub(in crate::controller) fn take_controller_serialization_attrs(
    attrs: &[Attribute],
) -> (
    Vec<Attribute>,
    ControllerSerializationAttrs,
    Vec<syn::Error>,
) {
    let mut clean_attrs = Vec::new();
    let mut serialization = ControllerSerializationAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "serialize") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<SerializationSpec>() {
            Ok(spec) => {
                if serialization.spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "controller impl blocks can use at most one #[serialize] attribute",
                    ));
                } else {
                    serialization.spec = Some(spec);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, serialization, errors)
}

pub(in crate::controller) fn take_route_serialization_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<SerializationSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "serialize") {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<SerializationSpec>() {
            Ok(parsed) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one #[serialize] attribute",
                    ));
                } else {
                    spec = Some(parsed);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, spec, errors)
}

#[derive(Default)]
pub(in crate::controller) struct ControllerSerializationAttrs {
    spec: Option<SerializationSpec>,
}

impl ControllerSerializationAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(SerializationSpec::token).collect()
    }
}

#[derive(Clone, Default)]
pub(in crate::controller) struct SerializationSpec {
    include_fields: Vec<LitStr>,
    exclude_fields: Vec<LitStr>,
    skip_null_fields: bool,
}

impl SerializationSpec {
    pub(in crate::controller) fn token(&self) -> proc_macro2::TokenStream {
        let mut options = quote!(::a3s_boot::SerializationOptions::new());

        if !self.include_fields.is_empty() {
            let fields = &self.include_fields;
            options = quote! {
                (#options).include_fields([#(#fields),*])
            };
        }

        if !self.exclude_fields.is_empty() {
            let fields = &self.exclude_fields;
            options = quote! {
                (#options).exclude_fields([#(#fields),*])
            };
        }

        if self.skip_null_fields {
            options = quote! {
                (#options).skip_null_fields()
            };
        }

        quote!(with_serialization(#options))
    }
}

impl Parse for SerializationSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut spec = Self::default();

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            let key = name.to_string();
            match key.as_str() {
                "include" => {
                    input.parse::<Token![=]>()?;
                    spec.include_fields.extend(parse_lit_str_array(input)?);
                }
                "exclude" => {
                    input.parse::<Token![=]>()?;
                    spec.exclude_fields.extend(parse_lit_str_array(input)?);
                }
                "skip_null" => {
                    if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        spec.skip_null_fields = input.parse::<LitBool>()?.value;
                    } else {
                        spec.skip_null_fields = true;
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `include`, `exclude`, or `skip_null`",
                    ));
                }
            }

            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(spec)
    }
}

fn parse_lit_str_array(input: ParseStream<'_>) -> Result<Vec<LitStr>> {
    let content;
    syn::bracketed!(content in input);
    Ok(Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}
