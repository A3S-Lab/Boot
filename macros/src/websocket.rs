use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Meta, Result, Token};

use crate::controller::{MethodArg, RouteMethodInput};
use crate::{push_error, set_once};

pub(crate) struct WebSocketGatewayArgs {
    path: LitStr,
    namespace: Option<LitStr>,
}

impl Parse for WebSocketGatewayArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let path = input.parse::<LitStr>()?;
        let mut namespace = None;

        while input.peek(Token![,]) {
            input.parse::<Token![,]>()?;
            if input.is_empty() {
                break;
            }

            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            match name.to_string().as_str() {
                "namespace" => set_once(&mut namespace, input.parse::<LitStr>()?, name)?,
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "unsupported websocket_gateway option",
                    ));
                }
            }
        }

        Ok(Self { path, namespace })
    }
}

pub(crate) fn expand_websocket_gateway(
    args: WebSocketGatewayArgs,
    mut item_impl: ItemImpl,
) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[websocket_gateway] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let path = args.path;
    let namespace = args.namespace.as_ref().map(
        |namespace| quote!(__a3s_boot_gateway = __a3s_boot_gateway.with_namespace(#namespace)?;),
    );
    let mut subscriptions = Vec::new();
    let mut lifecycle_hooks = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, events, event_errors) = take_subscribe_message_attrs(&method.attrs);
        let (clean_attrs, lifecycle_kinds, lifecycle_errors) =
            take_websocket_lifecycle_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in event_errors {
            push_error(&mut errors, error);
        }
        for error in lifecycle_errors {
            push_error(&mut errors, error);
        }
        if events.is_empty() && lifecycle_kinds.is_empty() {
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
                    "websocket gateway message handlers and lifecycle hooks must be async",
                ),
            );
            continue;
        }

        for event in events {
            match websocket_subscription(method, input.clone(), event) {
                Ok(subscription) => subscriptions.push(subscription),
                Err(error) => push_error(&mut errors, error),
            }
        }
        for kind in lifecycle_kinds {
            match websocket_lifecycle_hook(method, input.clone(), kind) {
                Ok(hook) => lifecycle_hooks.push(hook),
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
            pub fn gateway(
                self: ::std::sync::Arc<Self>,
            ) -> ::a3s_boot::Result<::a3s_boot::WebSocketGatewayDefinition> {
                let mut __a3s_boot_gateway =
                    ::a3s_boot::WebSocketGatewayDefinition::new(#path)?;
                #namespace
                #(
                    __a3s_boot_gateway = #subscriptions;
                )*
                #(
                    __a3s_boot_gateway = #lifecycle_hooks;
                )*
                Ok(__a3s_boot_gateway)
            }
        }
    })
}

fn take_subscribe_message_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<LitStr>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut events = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(ident) = attr.path().segments.last().map(|segment| &segment.ident) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        if ident != "subscribe_message" {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(event) => events.push(event),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, events, errors)
}

#[derive(Clone, Copy)]
enum WebSocketLifecycleHookKind {
    Init,
    Connection,
    Disconnect,
}

impl WebSocketLifecycleHookKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last().map(|segment| &segment.ident)?;
        if ident == "on_gateway_init" {
            Some(Self::Init)
        } else if ident == "on_gateway_connection" {
            Some(Self::Connection)
        } else if ident == "on_gateway_disconnect" {
            Some(Self::Disconnect)
        } else {
            None
        }
    }

    fn attribute_name(self) -> &'static str {
        match self {
            Self::Init => "on_gateway_init",
            Self::Connection => "on_gateway_connection",
            Self::Disconnect => "on_gateway_disconnect",
        }
    }
}

fn take_websocket_lifecycle_attrs(
    attrs: &[Attribute],
) -> (
    Vec<Attribute>,
    Vec<WebSocketLifecycleHookKind>,
    Vec<syn::Error>,
) {
    let mut clean_attrs = Vec::new();
    let mut hooks = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = WebSocketLifecycleHookKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match &attr.meta {
            Meta::Path(_) => hooks.push(kind),
            _ => errors.push(syn::Error::new_spanned(
                attr,
                format!("#[{}] does not accept arguments", kind.attribute_name()),
            )),
        }
    }

    (clean_attrs, hooks, errors)
}

fn websocket_subscription(
    method: &ImplItemFn,
    input: RouteMethodInput,
    event: LitStr,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let controller_name = format_ident!("__a3s_boot_ws_{}", method_ident);
    let handler = match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_message: ::a3s_boot::WebSocketMessage| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let #ident: #ty = __a3s_boot_message;
                        #controller_name.#method_ident(#ident).await
                    }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |_message: ::a3s_boot::WebSocketMessage| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move { #controller_name.#method_ident().await }
                }
            }
        },
    };

    Ok(quote! {
        __a3s_boot_gateway.subscribe(#event, #handler)?
    })
}

fn websocket_lifecycle_hook(
    method: &ImplItemFn,
    input: RouteMethodInput,
    kind: WebSocketLifecycleHookKind,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let gateway_name = format_ident!("__a3s_boot_ws_{}", method_ident);
    let (builder, context_ty) = match kind {
        WebSocketLifecycleHookKind::Init => (
            quote!(with_after_init),
            quote!(::a3s_boot::WebSocketGatewayInitContext),
        ),
        WebSocketLifecycleHookKind::Connection => (
            quote!(with_connection_hook),
            quote!(::a3s_boot::WebSocketGatewayConnection),
        ),
        WebSocketLifecycleHookKind::Disconnect => (
            quote!(with_disconnect_hook),
            quote!(::a3s_boot::WebSocketGatewayConnection),
        ),
    };
    let handler = match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #gateway_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_context: #context_ty| {
                    let #gateway_name = ::std::sync::Arc::clone(&#gateway_name);
                    async move {
                        let #ident: #ty = __a3s_boot_context;
                        #gateway_name.#method_ident(#ident).await
                    }
                }
            }
        },
        None => quote! {
            {
                let #gateway_name = ::std::sync::Arc::clone(&self);
                move |_context: #context_ty| {
                    let #gateway_name = ::std::sync::Arc::clone(&#gateway_name);
                    async move { #gateway_name.#method_ident().await }
                }
            }
        },
    };

    Ok(quote! {
        __a3s_boot_gateway.#builder(#handler)
    })
}
