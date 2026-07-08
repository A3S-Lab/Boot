use super::input::{Extractor, RouteMethodInput};
use super::routing::RouteFlavor;
use crate::validation::AttrOptions as ValidationAttrOptions;
use quote::quote;
use syn::Result;

pub(super) fn validation_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    validation_options: Option<ValidationAttrOptions>,
    validation_skipped: bool,
) -> Result<proc_macro2::TokenStream> {
    if validation_skipped {
        return Ok(quote! {
            (#route_definition).without_validation()
        });
    }

    let Some(options) = validation_options else {
        return Ok(route_definition);
    };

    for token in extractor_validation_tokens(input, flavor, options) {
        route_definition = quote! {
            (#route_definition).#token
        };
    }

    if options.is_empty() {
        Ok(quote! {
            (#route_definition).with_validation()
        })
    } else {
        let options = options.token();
        Ok(quote! {
            (#route_definition).with_validation_options(#options)
        })
    }
}

fn extractor_validation_tokens(
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    options: ValidationAttrOptions,
) -> Vec<proc_macro2::TokenStream> {
    let mut tokens = Vec::new();
    let use_options = !options.is_empty();
    let options_token = options.token();

    if matches!(flavor, RouteFlavor::JsonBody) && !input.has_extractors() {
        if let Some(arg) = input.args.first() {
            let ty = &arg.ty;
            if use_options {
                tokens.push(quote! {
                    with_body_validation_options::<#ty>(#options_token)
                });
            } else {
                tokens.push(quote! {
                    with_body_validation::<#ty>()
                });
            }
        }
    }

    for arg in &input.args {
        let Some(extractor) = &arg.extractor else {
            continue;
        };
        let ty = &arg.ty;

        match extractor {
            Extractor::Body => {
                if use_options {
                    tokens.push(quote! {
                        with_body_validation_options::<#ty>(#options_token)
                    });
                } else {
                    tokens.push(quote! {
                        with_body_validation::<#ty>()
                    });
                }
            }
            Extractor::Params => {
                if use_options {
                    tokens.push(quote! {
                        with_params_validation_options::<#ty>(#options_token)
                    });
                } else {
                    tokens.push(quote! {
                        with_params_validation::<#ty>()
                    });
                }
            }
            Extractor::Query(query) => {
                if query.name.is_none() {
                    if use_options {
                        tokens.push(quote! {
                            with_query_validation_options::<#ty>(#options_token)
                        });
                    } else {
                        tokens.push(quote! {
                            with_query_validation::<#ty>()
                        });
                    }
                }
            }
            Extractor::Request
            | Extractor::Param(_)
            | Extractor::Header(_)
            | Extractor::Headers
            | Extractor::HostParam(_)
            | Extractor::Ip(_)
            | Extractor::Response
            | Extractor::Session
            | Extractor::UploadedFile(_)
            | Extractor::UploadedFiles(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
}
