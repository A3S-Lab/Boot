use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Result, Token};

use crate::controller::{MethodArg, RouteMethodInput};
use crate::validation::{
    take_controller_validation_attrs, take_route_validation_attrs,
    AttrOptions as ValidationAttrOptions,
};
use crate::{is_type_ident, push_error};

pub(crate) fn expand_message_controller(
    mut item_impl: ItemImpl,
) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[message_controller] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut patterns = Vec::new();
    let mut errors: Option<syn::Error> = None;
    let (clean_impl_attrs, controller_validation, controller_validation_errors) =
        take_controller_validation_attrs(&item_impl.attrs);
    item_impl.attrs = clean_impl_attrs;
    for error in controller_validation_errors {
        push_error(&mut errors, error);
    }

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, specs, pattern_errors) = take_message_pattern_attrs(&method.attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in pattern_errors {
            push_error(&mut errors, error);
        }
        for error in validation_errors {
            push_error(&mut errors, error);
        }
        if specs.is_empty() && route_validation.is_present() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "validation attributes must be used on message pattern methods",
                ),
            );
        }
        if specs.is_empty() {
            continue;
        }

        let input = match RouteMethodInput::from_method(method) {
            Ok(input) => input,
            Err(error) => {
                push_error(&mut errors, error);
                continue;
            }
        };

        if method.sig.asyncness.is_none() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.fn_token,
                    "message pattern handlers must be async",
                ),
            );
            continue;
        }

        let validation_options = route_validation.enabled_options(controller_validation);
        for spec in specs {
            match message_pattern_registration(method, input.clone(), spec, validation_options) {
                Ok(pattern) => patterns.push(pattern),
                Err(error) => push_error(&mut errors, error),
            }
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    Ok(quote! {
        #item_impl

        impl #self_ty {
            pub fn message_patterns(
                self: ::std::sync::Arc<Self>,
            ) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::MessagePatternDefinition>> {
                let mut __a3s_boot_patterns = ::std::vec::Vec::new();
                #(
                    __a3s_boot_patterns.push(#patterns);
                )*
                Ok(__a3s_boot_patterns)
            }
        }
    })
}

fn take_message_pattern_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<MessagePatternSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut patterns = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = MessagePatternAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match attr.parse_args::<MessagePatternArgs>() {
            Ok(args) => {
                if matches!(kind, MessagePatternAttrKind::Event) && args.raw.is_some() {
                    errors.push(syn::Error::new_spanned(
                        args.raw.unwrap(),
                        "raw is not supported on event pattern attributes",
                    ));
                } else {
                    patterns.push(MessagePatternSpec { kind, args });
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, patterns, errors)
}

fn message_pattern_registration(
    method: &ImplItemFn,
    input: RouteMethodInput,
    spec: MessagePatternSpec,
    validation_options: Option<ValidationAttrOptions>,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let pattern = spec.args.pattern;
    let raw = spec.args.raw.is_some();
    let definition = match spec.kind {
        MessagePatternAttrKind::Message => {
            let handler = message_request_handler(method_ident, input.clone(), raw)?;
            quote! {
                ::a3s_boot::MessagePatternDefinition::request(#pattern, #handler)?
            }
        }
        MessagePatternAttrKind::Event => {
            let handler = message_event_handler(method_ident, input.clone())?;
            quote! {
                ::a3s_boot::MessagePatternDefinition::event(#pattern, #handler)?
            }
        }
    };

    let definition = message_validation_definition(definition, input, validation_options)?;
    Ok(definition)
}

fn message_request_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
    raw: bool,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_message_{}", method_ident);
    Ok(match input.into_legacy_arg()? {
        Some(arg) if is_type_ident(&arg.ty, "TransportMessage") => {
            let MethodArg { ident, ty, .. } = arg;
            let call = message_request_call(method_ident, &controller_name, raw, quote!(#ident));
            quote! {
                {
                    let #controller_name = ::std::sync::Arc::clone(&self);
                    move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                        let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                        async move {
                            let #ident: #ty = __a3s_boot_message;
                            #call
                        }
                    }
                }
            }
        }
        Some(MethodArg { ident, ty, .. }) => {
            let call = message_request_call(method_ident, &controller_name, raw, quote!(#ident));
            quote! {
                {
                    let #controller_name = ::std::sync::Arc::clone(&self);
                    move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                        let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                        async move {
                            let #ident: #ty = __a3s_boot_message.data_as::<#ty>()?;
                            #call
                        }
                    }
                }
            }
        }
        None => {
            let call = message_request_call(method_ident, &controller_name, raw, quote!());
            quote! {
                {
                    let #controller_name = ::std::sync::Arc::clone(&self);
                    move |_message: ::a3s_boot::TransportMessage| {
                        let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                        async move { #call }
                    }
                }
            }
        }
    })
}

fn message_request_call(
    method_ident: &Ident,
    controller_name: &Ident,
    raw: bool,
    args: proc_macro2::TokenStream,
) -> proc_macro2::TokenStream {
    if raw {
        quote! {
            #controller_name.#method_ident(#args).await
        }
    } else {
        quote! {
            {
                let __a3s_boot_reply = #controller_name.#method_ident(#args).await?;
                ::a3s_boot::TransportReply::json(&__a3s_boot_reply)
            }
        }
    }
}

fn message_event_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_event_{}", method_ident);
    Ok(match input.into_legacy_arg()? {
        Some(arg) if is_type_ident(&arg.ty, "TransportMessage") => {
            let MethodArg { ident, ty, .. } = arg;
            quote! {
                {
                    let #controller_name = ::std::sync::Arc::clone(&self);
                    move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                        let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                        async move {
                            let #ident: #ty = __a3s_boot_message;
                            let _ = #controller_name.#method_ident(#ident).await?;
                            Ok(())
                        }
                    }
                }
            }
        }
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let #ident: #ty = __a3s_boot_message.data_as::<#ty>()?;
                        let _ = #controller_name.#method_ident(#ident).await?;
                        Ok(())
                    }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |_message: ::a3s_boot::TransportMessage| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let _ = #controller_name.#method_ident().await?;
                        Ok(())
                    }
                }
            }
        },
    })
}

fn message_validation_definition(
    definition: proc_macro2::TokenStream,
    input: RouteMethodInput,
    validation_options: Option<ValidationAttrOptions>,
) -> Result<proc_macro2::TokenStream> {
    let Some(options) = validation_options else {
        return Ok(definition);
    };

    let Some(arg) = input.into_legacy_arg()? else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "message validation requires one typed payload argument",
        ));
    };

    if is_type_ident(&arg.ty, "TransportMessage") {
        return Err(syn::Error::new_spanned(
            arg.ident,
            "message validation requires a DTO payload argument, not TransportMessage",
        ));
    }

    let ty = arg.ty;
    if options.is_empty() {
        Ok(quote! {
            (#definition).with_payload_validation::<#ty>()
        })
    } else {
        let options = options.token();
        Ok(quote! {
            (#definition).with_payload_validation_options::<#ty>(#options)
        })
    }
}

#[derive(Clone, Copy)]
enum MessagePatternAttrKind {
    Message,
    Event,
}

impl MessagePatternAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "message_pattern" => Some(Self::Message),
            "event_pattern" => Some(Self::Event),
            _ => None,
        }
    }
}

struct MessagePatternSpec {
    kind: MessagePatternAttrKind,
    args: MessagePatternArgs,
}

struct MessagePatternArgs {
    pattern: LitStr,
    raw: Option<Ident>,
}

impl Parse for MessagePatternArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let pattern = input.parse::<LitStr>()?;
        let mut raw = None;

        while !input.is_empty() {
            input.parse::<Token![,]>()?;
            let name = input.parse::<Ident>()?;

            if name == "raw" {
                if raw.is_some() {
                    return Err(syn::Error::new_spanned(name, "duplicate `raw` option"));
                }
                raw = Some(name);
            } else {
                return Err(syn::Error::new_spanned(name, "expected `raw`"));
            }
        }

        Ok(Self { pattern, raw })
    }
}
