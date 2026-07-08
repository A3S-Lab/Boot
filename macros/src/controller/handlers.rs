use super::input::{Extractor, MethodArg, RouteMethodInput, SingleValueExtractor};
use crate::file_upload;
use crate::option_inner_type;
use quote::{format_ident, quote};
use syn::{Expr, Ident, LitStr, Result, Type};

pub(super) fn raw_or_json_request_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    Ok(match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |#ident: #ty| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move { #controller_name.#method_ident(#ident).await }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move { #controller_name.#method_ident().await }
                }
            }
        },
    })
}

pub(super) fn json_body_handler(
    method_ident: &Ident,
    input: MethodArg,
) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let MethodArg { ident, ty, .. } = input;
    quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |#ident: #ty| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move { #controller_name.#method_ident(#ident).await }
            }
        }
    }
}

pub(super) fn extracted_raw_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#extractors)*
                    #controller_name.#method_ident(#(#args),*).await
                }
            }
        }
    })
}

pub(super) fn extracted_json_response_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
    status: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    __a3s_boot_request.require_accepts_json()?;
                    #(#extractors)*
                    let __a3s_boot_body = #controller_name.#method_ident(#(#args),*).await?;
                    ::a3s_boot::BootResponse::json_with_status(#status, &__a3s_boot_body)
                }
            }
        }
    })
}

pub(super) fn rendered_view_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
    view: &LitStr,
    status: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);

    if input.has_extractors() {
        let (extractors, args) = extracted_arguments(input)?;
        return Ok(quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        #(#extractors)*
                        let __a3s_boot_context =
                            #controller_name.#method_ident(#(#args),*).await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        });
    }

    Ok(match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        let #ident: #ty = __a3s_boot_request.clone();
                        let __a3s_boot_context = #controller_name.#method_ident(#ident).await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        let __a3s_boot_context = #controller_name.#method_ident().await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        },
    })
}

pub(super) fn extracted_sse_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#extractors)*
                    #controller_name.#method_ident(#(#args),*).await
                }
            }
        }
    })
}

fn extracted_arguments(
    input: RouteMethodInput,
) -> Result<(Vec<proc_macro2::TokenStream>, Vec<Ident>)> {
    let mut body_arg: Option<Ident> = None;
    let mut multipart_arg: Option<Ident> = None;
    let mut extractors = Vec::new();
    let mut args = Vec::new();

    for arg in input.args {
        let extractor = arg.extractor.clone().ok_or_else(|| {
            syn::Error::new_spanned(
                &arg.ident,
                "all route arguments must use extractor attributes when any extractor is used",
            )
        })?;

        if matches!(extractor, Extractor::Body) {
            if multipart_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine #[body] with multipart upload extractors",
                ));
            }
            if let Some(existing) = body_arg {
                return Err(syn::Error::new_spanned(
                    existing,
                    "route methods can accept at most one #[body] argument",
                ));
            }
            body_arg = Some(arg.ident.clone());
        }
        if matches!(
            extractor,
            Extractor::UploadedFile(_) | Extractor::UploadedFiles(_)
        ) {
            if body_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine multipart upload extractors with #[body]",
                ));
            }
            multipart_arg = Some(arg.ident.clone());
        }

        args.push(arg.ident.clone());
        extractors.push(extractor_tokens(arg, extractor));
    }

    if multipart_arg.is_some() {
        extractors.insert(
            0,
            quote! {
                let __a3s_boot_multipart_form = __a3s_boot_request.multipart_form().await?;
            },
        );
    }

    Ok((extractors, args))
}

fn extractor_tokens(arg: MethodArg, extractor: Extractor) -> proc_macro2::TokenStream {
    let MethodArg { ident, ty, .. } = arg;
    match extractor {
        Extractor::Body => quote! {
            __a3s_boot_request.require_json_content_type()?;
            let #ident: #ty = __a3s_boot_request.json::<#ty>()?;
        },
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
