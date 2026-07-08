use super::input::{BodyExtractor, Extractor, MethodArg, RouteMethodInput, SingleValueExtractor};
use crate::file_upload;
use crate::option_inner_type;
use quote::quote;
use syn::{Expr, Ident, Result, Type};

pub(super) struct ExtractedArguments {
    pub(super) setup: proc_macro2::TokenStream,
    pub(super) extractors: Vec<proc_macro2::TokenStream>,
    pub(super) args: Vec<Ident>,
    pub(super) response_passthrough: bool,
}

pub(super) fn extracted_arguments(input: RouteMethodInput) -> Result<ExtractedArguments> {
    let mut whole_body_arg: Option<Ident> = None;
    let mut body_field_arg: Option<Ident> = None;
    let mut multipart_arg: Option<Ident> = None;
    let mut response_arg: Option<Ident> = None;
    let mut extractors = Vec::new();
    let mut args = Vec::new();

    for arg in input.args {
        let extractor = arg.extractor.clone().ok_or_else(|| {
            syn::Error::new_spanned(
                &arg.ident,
                "all route arguments must use extractor attributes when any extractor is used",
            )
        })?;

        if let Extractor::Body(body) = &extractor {
            if multipart_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine #[body] with multipart upload extractors",
                ));
            }
            match body {
                BodyExtractor::Whole => {
                    if let Some(existing) = whole_body_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "route methods can accept at most one whole #[body] argument",
                        ));
                    }
                    if let Some(existing) = &body_field_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "route methods cannot combine whole #[body] arguments with #[body(\"field\")] arguments",
                        ));
                    }
                    whole_body_arg = Some(arg.ident.clone());
                }
                BodyExtractor::Field(_) => {
                    if let Some(existing) = &whole_body_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "route methods cannot combine whole #[body] arguments with #[body(\"field\")] arguments",
                        ));
                    }
                    body_field_arg.get_or_insert_with(|| arg.ident.clone());
                }
            }
        }
        if matches!(
            extractor,
            Extractor::UploadedFile(_) | Extractor::UploadedFiles(_)
        ) {
            if whole_body_arg.is_some() || body_field_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine multipart upload extractors with #[body]",
                ));
            }
            multipart_arg = Some(arg.ident.clone());
        }
        if matches!(extractor, Extractor::Response) {
            if let Some(existing) = response_arg {
                return Err(syn::Error::new_spanned(
                    existing,
                    "route methods can accept at most one #[res] argument",
                ));
            }
            response_arg = Some(arg.ident.clone());
        }

        args.push(arg.ident.clone());
        extractors.push(extractor_tokens(arg, extractor));
    }

    let response_passthrough = response_arg.is_some();
    let setup = if response_passthrough {
        quote! {
            let __a3s_boot_response_passthrough =
                ::a3s_boot::ResponsePassthrough::new();
        }
    } else {
        quote! {}
    };

    if multipart_arg.is_some() {
        extractors.insert(
            0,
            quote! {
                let __a3s_boot_multipart_form = __a3s_boot_request.multipart_form().await?;
            },
        );
    }

    Ok(ExtractedArguments {
        setup,
        extractors,
        args,
        response_passthrough,
    })
}

fn extractor_tokens(arg: MethodArg, extractor: Extractor) -> proc_macro2::TokenStream {
    let MethodArg { ident, ty, .. } = arg;
    match extractor {
        Extractor::Body(BodyExtractor::Whole) => quote! {
            __a3s_boot_request.require_json_content_type()?;
            let #ident: #ty = __a3s_boot_request.json::<#ty>()?;
        },
        Extractor::Body(BodyExtractor::Field(spec)) => body_field_extractor_tokens(ident, ty, spec),
        Extractor::Request => quote! {
            let #ident: #ty = __a3s_boot_request.clone();
        },
        Extractor::Params => quote! {
            let #ident: #ty = __a3s_boot_request.params::<#ty>()?;
        },
        Extractor::Param(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.param_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_param_as::<#value_ty>(#name)),
            )
        }
        Extractor::Query(spec) => {
            if let Some(name) = spec.name {
                single_value_extractor_tokens(
                    ident,
                    ty,
                    spec.pipe,
                    spec.default,
                    |value_ty| quote!(__a3s_boot_request.query_value_as::<#value_ty>(#name)),
                    |value_ty| quote!(__a3s_boot_request.optional_query_value_as::<#value_ty>(#name)),
                )
            } else {
                quote! {
                    let #ident: #ty = __a3s_boot_request.query::<#ty>()?;
                }
            }
        }
        Extractor::Header(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.header_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_header_as::<#value_ty>(#name)),
            )
        }
        Extractor::Headers => quote! {
            let #ident: #ty = __a3s_boot_request.headers.clone();
        },
        Extractor::Cookie(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.cookie_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_cookie_as::<#value_ty>(#name)),
            )
        }
        Extractor::Cookies => quote! {
            let #ident: #ty = __a3s_boot_request.cookies()?;
        },
        Extractor::HostParam(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.host_param_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_host_param_as::<#value_ty>(#name)),
            )
        }
        Extractor::Ip(pipe) => single_value_extractor_tokens(
            ident,
            ty,
            pipe,
            None,
            |value_ty| quote!(__a3s_boot_request.ip_as::<#value_ty>()),
            |value_ty| quote!(__a3s_boot_request.optional_ip_as::<#value_ty>()),
        ),
        Extractor::Response => quote! {
            let #ident: #ty = __a3s_boot_response_passthrough.clone();
        },
        Extractor::Session => {
            if option_inner_type(&ty).is_some() {
                quote! {
                    let #ident: #ty = __a3s_boot_request.optional_session()?;
                }
            } else {
                quote! {
                    let #ident: #ty = __a3s_boot_request.session()?;
                }
            }
        }
        Extractor::UploadedFile(spec) => file_upload::uploaded_file_extractor_tokens(
            ident,
            ty.clone(),
            spec,
            option_inner_type(&ty).is_some(),
        ),
        Extractor::UploadedFiles(spec) => {
            file_upload::uploaded_files_extractor_tokens(ident, ty, spec)
        }
        Extractor::Custom(extractor) => quote! {
            let #ident: #ty = ::a3s_boot::extract_request_value::<#ty, _>(&__a3s_boot_request, #extractor)?;
        },
    }
}

fn body_field_extractor_tokens(
    ident: Ident,
    ty: Box<Type>,
    spec: SingleValueExtractor,
) -> proc_macro2::TokenStream {
    let SingleValueExtractor {
        name,
        pipe,
        default,
    } = spec;
    let uses_pipe = pipe.is_some();
    single_value_extractor_tokens(
        ident,
        ty,
        pipe,
        default,
        |value_ty| {
            if uses_pipe {
                quote!(__a3s_boot_request.body_field_string(#name))
            } else {
                quote!(__a3s_boot_request.body_field_as::<#value_ty>(#name))
            }
        },
        |value_ty| {
            if uses_pipe {
                quote!(__a3s_boot_request.optional_body_field_string(#name))
            } else {
                quote!(__a3s_boot_request.optional_body_field_as::<#value_ty>(#name))
            }
        },
    )
}

fn single_value_extractor_tokens<Required, Optional>(
    ident: Ident,
    ty: Box<Type>,
    pipe: Option<Expr>,
    default: Option<Expr>,
    required: Required,
    optional: Optional,
) -> proc_macro2::TokenStream
where
    Required: FnOnce(&Type) -> proc_macro2::TokenStream,
    Optional: FnOnce(&Type) -> proc_macro2::TokenStream,
{
    if let Some(pipe) = pipe {
        if let Some(inner) = option_inner_type(&ty) {
            let value = optional(&parse_string_type());
            if let Some(default) = default {
                quote! {
                    let #ident: #ty = match #value? {
                        Some(__a3s_boot_value) => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                __a3s_boot_value,
                                #pipe,
                            )?)
                        }
                        None => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                ::std::string::ToString::to_string(&(#default)),
                                #pipe,
                            )?)
                        }
                    };
                }
            } else {
                quote! {
                    let #ident: #ty = match #value? {
                        Some(__a3s_boot_value) => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                __a3s_boot_value,
                                #pipe,
                            )?)
                        }
                        None => None,
                    };
                }
            }
        } else if let Some(default) = default {
            let value = optional(&parse_string_type());
            quote! {
                let __a3s_boot_value = match #value? {
                    Some(__a3s_boot_value) => {
                        __a3s_boot_value
                    }
                    None => ::std::string::ToString::to_string(&(#default)),
                };
                let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                    __a3s_boot_value,
                    #pipe,
                )?;
            }
        } else {
            let value = required(&parse_string_type());
            quote! {
                let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                    #value?,
                    #pipe,
                )?;
            }
        }
    } else if let Some(inner) = option_inner_type(&ty) {
        let value = optional(&inner);
        if let Some(default) = default {
            quote! {
                let #ident: #ty = match #value? {
                    Some(__a3s_boot_value) => Some(__a3s_boot_value),
                    None => Some(#default),
                };
            }
        } else {
            quote! {
                let #ident: #ty = #value?;
            }
        }
    } else if let Some(default) = default {
        let value = optional(&ty);
        quote! {
            let #ident: #ty = match #value? {
                Some(__a3s_boot_value) => __a3s_boot_value,
                None => #default,
            };
        }
    } else {
        let value = required(&ty);
        quote! {
            let #ident: #ty = #value?;
        }
    }
}

fn parse_string_type() -> Type {
    syn::parse_quote!(String)
}

pub(super) fn response_passthrough_apply(enabled: bool) -> proc_macro2::TokenStream {
    if enabled {
        quote! {
            __a3s_boot_response_passthrough.apply(__a3s_boot_response)
        }
    } else {
        quote! {
            Ok(__a3s_boot_response)
        }
    }
}
