use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Meta, Result, Token};

use crate::controller::attrs::{
    take_controller_metadata_attrs, take_controller_pipeline_attrs, take_route_metadata_attrs,
    take_route_pipeline_attrs, MetadataSpec, PipelineSpec,
};
use crate::controller::{MethodArg, ProtocolExtractor, ProtocolPayloadExtractor, RouteMethodInput};
use crate::decorators::expand_apply_decorators_attrs;
use crate::is_type_ident;
use crate::protocol::json_payload_binding_tokens;
use crate::validation::{
    take_controller_validation_attrs, take_route_validation_attrs,
    AttrOptions as ValidationAttrOptions,
};
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
    let (impl_attrs, impl_decorator_errors) = expand_apply_decorators_attrs(&item_impl.attrs);
    for error in impl_decorator_errors {
        push_error(&mut errors, error);
    }
    let (clean_impl_attrs, gateway_validation, gateway_validation_errors) =
        take_controller_validation_attrs(&impl_attrs);
    let (clean_impl_attrs, gateway_pipeline, gateway_pipeline_errors) =
        take_controller_pipeline_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, gateway_metadata, gateway_metadata_errors) =
        take_controller_metadata_attrs(&clean_impl_attrs);
    item_impl.attrs = clean_impl_attrs;
    for error in gateway_validation_errors {
        push_error(&mut errors, error);
    }
    for error in gateway_pipeline_errors {
        push_error(&mut errors, error);
    }
    for error in gateway_metadata_errors {
        push_error(&mut errors, error);
    }
    let gateway_pipeline = gateway_pipeline.tokens();
    let gateway_metadata = gateway_metadata.tokens();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (method_attrs, decorator_errors) = expand_apply_decorators_attrs(&method.attrs);
        for error in decorator_errors {
            push_error(&mut errors, error);
        }
        let (clean_attrs, events, event_errors) = take_subscribe_message_attrs(&method_attrs);
        let (clean_attrs, lifecycle_kinds, lifecycle_errors) =
            take_websocket_lifecycle_attrs(&clean_attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        let (clean_attrs, metadata_specs, metadata_errors) =
            take_route_metadata_attrs(&clean_attrs);
        let (clean_attrs, pipeline_specs, pipeline_errors) =
            take_route_pipeline_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in event_errors {
            push_error(&mut errors, error);
        }
        for error in lifecycle_errors {
            push_error(&mut errors, error);
        }
        for error in validation_errors {
            push_error(&mut errors, error);
        }
        for error in metadata_errors {
            push_error(&mut errors, error);
        }
        for error in pipeline_errors {
            push_error(&mut errors, error);
        }
        if events.is_empty() && !metadata_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "metadata attributes must be used on websocket message handlers",
                ),
            );
        }
        if events.is_empty() && route_validation.is_present() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "validation attributes must be used on websocket message handlers",
                ),
            );
        }
        if events.is_empty() && !pipeline_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "pipeline attributes must be used on websocket message handlers",
                ),
            );
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
                    method.sig.fn_token,
                    "websocket gateway message handlers and lifecycle hooks must be async",
                ),
            );
            continue;
        }

        for event in events {
            let validation_options = route_validation.enabled_options(gateway_validation);
            match websocket_subscription(
                method,
                input.clone(),
                event,
                validation_options,
                &metadata_specs,
                &pipeline_specs,
            ) {
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
                    __a3s_boot_gateway = __a3s_boot_gateway.#gateway_metadata?;
                )*
                #(
                    __a3s_boot_gateway = __a3s_boot_gateway.#gateway_pipeline;
                )*
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
    validation_options: Option<ValidationAttrOptions>,
    metadata_specs: &[MetadataSpec],
    pipeline_specs: &[PipelineSpec],
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let controller_name = format_ident!("__a3s_boot_ws_{}", method_ident);
    let args = websocket_subscription_args(input)?;
    let handler = websocket_subscription_handler(method_ident, &controller_name, &args);
    let pipeline_specs = pipeline_specs.iter().map(PipelineSpec::token);
    let subscription = websocket_subscription_metadata_definition(
        quote! {
            ::a3s_boot::WebSocketSubscriptionDefinition::new_with_connection(#handler)
        },
        metadata_specs,
    );
    let subscription = websocket_validation_subscription(subscription, &args, validation_options)?;
    Ok(quote! {
        __a3s_boot_gateway.subscribe_definition(
            #event,
            (#subscription)
                #(.#pipeline_specs)*
        )?
    })
}

#[derive(Clone)]
struct WebSocketSubscriptionArgs {
    args: Vec<WebSocketSubscriptionArg>,
}

impl WebSocketSubscriptionArgs {
    fn whole_payload_arg(&self) -> Option<&MethodArg> {
        self.args.iter().find_map(|arg| match &arg.kind {
            WebSocketSubscriptionArgKind::Payload(ProtocolPayloadExtractor::Whole) => {
                Some(&arg.arg)
            }
            WebSocketSubscriptionArgKind::Payload(ProtocolPayloadExtractor::Field(_))
            | WebSocketSubscriptionArgKind::Connection
            | WebSocketSubscriptionArgKind::Server
            | WebSocketSubscriptionArgKind::Message => None,
        })
    }

    fn message_arg(&self) -> Option<&MethodArg> {
        self.args.iter().find_map(|arg| match &arg.kind {
            WebSocketSubscriptionArgKind::Message => Some(&arg.arg),
            WebSocketSubscriptionArgKind::Connection
            | WebSocketSubscriptionArgKind::Server
            | WebSocketSubscriptionArgKind::Payload(_) => None,
        })
    }
}

#[derive(Clone)]
struct WebSocketSubscriptionArg {
    arg: MethodArg,
    kind: WebSocketSubscriptionArgKind,
}

#[derive(Clone)]
enum WebSocketSubscriptionArgKind {
    Connection,
    Server,
    Message,
    Payload(ProtocolPayloadExtractor),
}

fn websocket_subscription_args(input: RouteMethodInput) -> Result<WebSocketSubscriptionArgs> {
    if input.has_extractors() {
        return Err(syn::Error::new_spanned(
            input
                .args
                .iter()
                .find(|arg| arg.extractor.is_some())
                .map(|arg| arg.ident.clone())
                .unwrap_or_else(|| format_ident!("argument")),
            "websocket subscription methods do not support route extractor attributes",
        ));
    }

    let has_protocol_extractors = input.has_protocol_extractors();
    let mut has_connection = false;
    let mut has_server = false;
    let mut has_message_arg = false;
    let mut whole_payload_arg = None;
    let mut field_payload_arg = None;
    let mut args = Vec::new();

    for arg in input.args {
        let kind = if is_type_ident(&arg.ty, "WebSocketGatewayConnection") {
            if arg.protocol_extractor.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "WebSocketGatewayConnection arguments do not use protocol payload extractor attributes",
                ));
            }
            if has_connection {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "websocket subscription methods can accept at most one WebSocketGatewayConnection argument",
                ));
            }
            has_connection = true;
            WebSocketSubscriptionArgKind::Connection
        } else if is_type_ident(&arg.ty, "WebSocketGatewayServer") {
            if arg.protocol_extractor.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "WebSocketGatewayServer arguments do not use protocol payload extractor attributes",
                ));
            }
            if has_server {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "websocket subscription methods can accept at most one WebSocketGatewayServer argument",
                ));
            }
            has_server = true;
            WebSocketSubscriptionArgKind::Server
        } else if !has_protocol_extractors {
            if is_type_ident(&arg.ty, "WebSocketMessage") {
                if has_message_arg || whole_payload_arg.is_some() {
                    return Err(syn::Error::new_spanned(
                        arg.ident,
                        "websocket subscription methods can accept at most one message body argument",
                    ));
                }
                has_message_arg = true;
                WebSocketSubscriptionArgKind::Message
            } else {
                if has_message_arg || whole_payload_arg.is_some() {
                    return Err(syn::Error::new_spanned(
                        arg.ident,
                        "websocket subscription methods can accept at most one message body argument",
                    ));
                }
                whole_payload_arg = Some(arg.ident.clone());
                WebSocketSubscriptionArgKind::Payload(ProtocolPayloadExtractor::Whole)
            }
        } else {
            let Some(extractor) = arg.protocol_extractor.clone() else {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "websocket subscription methods must use #[message_body] on every payload argument when any protocol payload extractor is used",
                ));
            };

            let payload = match extractor {
                ProtocolExtractor::MessageBody(payload) => payload,
                ProtocolExtractor::Payload(_) => {
                    return Err(syn::Error::new_spanned(
                        arg.ident,
                        "websocket subscription methods support #[message_body], not #[payload]",
                    ));
                }
            };

            if is_type_ident(&arg.ty, "WebSocketMessage") {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "whole #[message_body] arguments must be DTOs; use an undecorated WebSocketMessage argument for raw access",
                ));
            }

            match &payload {
                ProtocolPayloadExtractor::Whole => {
                    if let Some(existing) = whole_payload_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "websocket subscription methods can accept at most one whole #[message_body] argument",
                        ));
                    }
                    if let Some(existing) = &field_payload_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "websocket subscription methods cannot combine whole #[message_body] arguments with #[message_body(\"field\")] arguments",
                        ));
                    }
                    whole_payload_arg = Some(arg.ident.clone());
                }
                ProtocolPayloadExtractor::Field(_) => {
                    if let Some(existing) = &whole_payload_arg {
                        return Err(syn::Error::new_spanned(
                            existing,
                            "websocket subscription methods cannot combine whole #[message_body] arguments with #[message_body(\"field\")] arguments",
                        ));
                    }
                    field_payload_arg.get_or_insert_with(|| arg.ident.clone());
                }
            }

            WebSocketSubscriptionArgKind::Payload(payload)
        };
        args.push(WebSocketSubscriptionArg { arg, kind });
    }

    Ok(WebSocketSubscriptionArgs { args })
}

fn websocket_subscription_handler(
    method_ident: &Ident,
    controller_name: &Ident,
    args: &WebSocketSubscriptionArgs,
) -> proc_macro2::TokenStream {
    let bindings = args.args.iter().map(|arg| {
        let MethodArg { ident, ty, .. } = &arg.arg;
        match &arg.kind {
            WebSocketSubscriptionArgKind::Connection => quote! {
                let #ident: #ty = __a3s_boot_connection.clone();
            },
            WebSocketSubscriptionArgKind::Server => quote! {
                let #ident: #ty = __a3s_boot_connection.server();
            },
            WebSocketSubscriptionArgKind::Message => quote! {
                let #ident: #ty = __a3s_boot_message.clone();
            },
            WebSocketSubscriptionArgKind::Payload(extractor) => json_payload_binding_tokens(
                ident.clone(),
                ty.clone(),
                extractor.clone(),
                |value_ty| quote!(__a3s_boot_message.data_as::<#value_ty>()),
                |value_ty, name| quote!(__a3s_boot_message.data_field_as::<#value_ty>(#name)),
                |value_ty, name| {
                    quote!(__a3s_boot_message.optional_data_field_as::<#value_ty>(#name))
                },
                |name| quote!(__a3s_boot_message.data_field_string(#name)),
                |name| quote!(__a3s_boot_message.optional_data_field_string(#name)),
            ),
        }
    });
    let call_args = args.args.iter().map(|arg| &arg.arg.ident);

    quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |
                __a3s_boot_connection: ::a3s_boot::WebSocketGatewayConnection,
                __a3s_boot_message: ::a3s_boot::WebSocketMessage,
            | {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#bindings)*
                    #controller_name.#method_ident(#(#call_args),*).await
                }
            }
        }
    }
}

fn websocket_validation_subscription(
    subscription: proc_macro2::TokenStream,
    args: &WebSocketSubscriptionArgs,
    validation_options: Option<ValidationAttrOptions>,
) -> Result<proc_macro2::TokenStream> {
    let Some(options) = validation_options else {
        return Ok(subscription);
    };

    if let Some(arg) = args.message_arg() {
        return Err(syn::Error::new_spanned(
            arg.ident.clone(),
            "websocket validation requires a DTO message body argument, not WebSocketMessage",
        ));
    }

    let Some(arg) = args.whole_payload_arg() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "websocket validation requires one whole typed message body argument",
        ));
    };

    let ty = &arg.ty;
    if options.is_empty() {
        Ok(quote! {
            (#subscription).with_payload_validation::<#ty>()
        })
    } else {
        let options = options.token();
        Ok(quote! {
            (#subscription).with_payload_validation_options::<#ty>(#options)
        })
    }
}

fn websocket_subscription_metadata_definition(
    mut subscription: proc_macro2::TokenStream,
    metadata_specs: &[MetadataSpec],
) -> proc_macro2::TokenStream {
    for spec in metadata_specs {
        let key = &spec.key;
        let value = &spec.value;
        subscription = quote! {
            (#subscription).with_metadata(#key, #value)?
        };
    }
    subscription
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
    let args = websocket_lifecycle_args(input, kind)?;
    let bindings = args.bindings();
    let call_args = args.call_args();
    let handler = quote! {
        {
            let #gateway_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_context: #context_ty| {
                let #gateway_name = ::std::sync::Arc::clone(&#gateway_name);
                async move {
                    #(#bindings)*
                    #gateway_name.#method_ident(#(#call_args),*).await
                }
            }
        }
    };

    Ok(quote! {
        __a3s_boot_gateway.#builder(#handler)
    })
}

#[derive(Clone)]
struct WebSocketLifecycleArgs {
    args: Vec<WebSocketLifecycleArg>,
}

impl WebSocketLifecycleArgs {
    fn bindings(&self) -> Vec<proc_macro2::TokenStream> {
        self.args
            .iter()
            .map(|arg| {
                let MethodArg { ident, ty, .. } = &arg.arg;
                match arg.kind {
                    WebSocketLifecycleArgKind::Context => quote! {
                        let #ident: #ty = __a3s_boot_context.clone();
                    },
                    WebSocketLifecycleArgKind::Server => quote! {
                        let #ident: #ty = __a3s_boot_context.server();
                    },
                }
            })
            .collect()
    }

    fn call_args(&self) -> Vec<Ident> {
        self.args.iter().map(|arg| arg.arg.ident.clone()).collect()
    }
}

#[derive(Clone)]
struct WebSocketLifecycleArg {
    arg: MethodArg,
    kind: WebSocketLifecycleArgKind,
}

#[derive(Clone, Copy)]
enum WebSocketLifecycleArgKind {
    Context,
    Server,
}

fn websocket_lifecycle_args(
    input: RouteMethodInput,
    kind: WebSocketLifecycleHookKind,
) -> Result<WebSocketLifecycleArgs> {
    if input.has_extractors() || input.has_protocol_extractors() {
        return Err(syn::Error::new_spanned(
            input
                .args
                .iter()
                .find(|arg| arg.extractor.is_some() || arg.protocol_extractor.is_some())
                .map(|arg| arg.ident.clone())
                .unwrap_or_else(|| format_ident!("argument")),
            "websocket lifecycle hook methods do not support extractor attributes",
        ));
    }

    let context_type = match kind {
        WebSocketLifecycleHookKind::Init => "WebSocketGatewayInitContext",
        WebSocketLifecycleHookKind::Connection | WebSocketLifecycleHookKind::Disconnect => {
            "WebSocketGatewayConnection"
        }
    };
    let mut has_context = false;
    let mut has_server = false;
    let mut args = Vec::new();

    for arg in input.args {
        let kind = if is_type_ident(&arg.ty, "WebSocketGatewayServer") {
            if has_server {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "websocket lifecycle hook methods can accept at most one WebSocketGatewayServer argument",
                ));
            }
            has_server = true;
            WebSocketLifecycleArgKind::Server
        } else if is_type_ident(&arg.ty, context_type) {
            if has_context {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    format!(
                        "websocket lifecycle hook methods can accept at most one {context_type} argument"
                    ),
                ));
            }
            has_context = true;
            WebSocketLifecycleArgKind::Context
        } else {
            return Err(syn::Error::new_spanned(
                arg.ident,
                format!(
                    "websocket lifecycle hook methods can only accept {context_type} and WebSocketGatewayServer arguments"
                ),
            ));
        };

        args.push(WebSocketLifecycleArg { arg, kind });
    }

    Ok(WebSocketLifecycleArgs { args })
}
