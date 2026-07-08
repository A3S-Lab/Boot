use super::handler_arguments::{
    extracted_arguments, response_passthrough_apply, ExtractedArguments,
};
use super::input::{MethodArg, RouteMethodInput};
use quote::{format_ident, quote};
use syn::{Ident, LitStr, Result};

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
    let ExtractedArguments {
        setup,
        extractors,
        args,
        response_passthrough,
    } = extracted_arguments(input)?;
    let apply_response = response_passthrough_apply(response_passthrough);

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #setup
                    #(#extractors)*
                    let __a3s_boot_response =
                        #controller_name.#method_ident(#(#args),*).await?;
                    #apply_response
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
    let ExtractedArguments {
        setup,
        extractors,
        args,
        response_passthrough,
    } = extracted_arguments(input)?;
    let apply_response = response_passthrough_apply(response_passthrough);

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #setup
                    __a3s_boot_request.require_accepts_json()?;
                    #(#extractors)*
                    let __a3s_boot_body = #controller_name.#method_ident(#(#args),*).await?;
                    let __a3s_boot_response =
                        ::a3s_boot::BootResponse::json_with_status(#status, &__a3s_boot_body)?;
                    #apply_response
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
        let ExtractedArguments {
            setup,
            extractors,
            args,
            response_passthrough,
        } = extracted_arguments(input)?;
        let apply_response = response_passthrough_apply(response_passthrough);
        return Ok(quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        #setup
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        #(#extractors)*
                        let __a3s_boot_context =
                            #controller_name.#method_ident(#(#args),*).await?;
                        let __a3s_boot_response = __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await?;
                        #apply_response
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
    let ExtractedArguments {
        setup,
        extractors,
        args,
        response_passthrough,
    } = extracted_arguments(input)?;
    if response_passthrough {
        return Err(syn::Error::new_spanned(
            method_ident,
            "#[res] is not supported on SSE route methods",
        ));
    }

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #setup
                    #(#extractors)*
                    #controller_name.#method_ident(#(#args),*).await
                }
            }
        }
    })
}
