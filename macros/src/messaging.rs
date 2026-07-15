use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Result, Token};

use crate::controller::attrs::{
    take_controller_metadata_attrs, take_controller_pipeline_attrs, take_route_metadata_attrs,
    take_route_pipeline_attrs, MetadataSpec, PipelineSpec,
};
use crate::controller::{MethodArg, ProtocolExtractor, ProtocolPayloadExtractor, RouteMethodInput};
use crate::decorators::expand_apply_decorators_attrs;
use crate::protocol::json_payload_binding_tokens;
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
    let (impl_attrs, impl_decorator_errors) = expand_apply_decorators_attrs(&item_impl.attrs);
    for error in impl_decorator_errors {
        push_error(&mut errors, error);
    }
    let (clean_impl_attrs, controller_validation, controller_validation_errors) =
        take_controller_validation_attrs(&impl_attrs);
    let (clean_impl_attrs, controller_metadata, controller_metadata_errors) =
        take_controller_metadata_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_pipeline, controller_pipeline_errors) =
        take_controller_pipeline_attrs(&clean_impl_attrs);
    item_impl.attrs = clean_impl_attrs;
    for error in controller_validation_errors {
        push_error(&mut errors, error);
    }
    for error in controller_metadata_errors {
        push_error(&mut errors, error);
    }
    for error in controller_pipeline_errors {
        push_error(&mut errors, error);
    }
    let controller_metadata = controller_metadata.tokens();
    let controller_pipeline = controller_pipeline.tokens();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (method_attrs, decorator_errors) = expand_apply_decorators_attrs(&method.attrs);
        for error in decorator_errors {
            push_error(&mut errors, error);
        }
        let (clean_attrs, specs, pattern_errors) = take_message_pattern_attrs(&method_attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        let (clean_attrs, metadata_specs, metadata_errors) =
            take_route_metadata_attrs(&clean_attrs);
        let (clean_attrs, pipeline_specs, pipeline_errors) =
            take_route_pipeline_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in pattern_errors {
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
        if specs.is_empty() && route_validation.is_present() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "validation attributes must be used on message pattern methods",
                ),
            );
        }
        if specs.is_empty() && !pipeline_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "pipeline attributes must be used on message pattern methods",
                ),
            );
        }
        if specs.is_empty() && !metadata_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "metadata attributes must be used on message pattern methods",
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
                    method.sig.fn_token,
                    "message pattern handlers must be async",
                ),
            );
            continue;
        }

        let validation_options = route_validation.enabled_options(controller_validation);
        for spec in specs {
            match message_pattern_registration(
                method,
                input.clone(),
                spec,
                validation_options,
                &controller_metadata,
                &metadata_specs,
                &controller_pipeline,
                &pipeline_specs,
            ) {
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
                if matches!(kind, MessagePatternAttrKind::Event) {
                    if let Some(raw) = &args.raw {
                        errors.push(syn::Error::new_spanned(
                            raw,
                            "raw is not supported on event pattern attributes",
                        ));
                        continue;
                    }
                }
                patterns.push(MessagePatternSpec { kind, args });
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, patterns, errors)
}

// Message registration mirrors the independent controller and method decorators.
#[allow(clippy::too_many_arguments)]
fn message_pattern_registration(
    method: &ImplItemFn,
    input: RouteMethodInput,
    spec: MessagePatternSpec,
    validation_options: Option<ValidationAttrOptions>,
    controller_metadata: &[proc_macro2::TokenStream],
    metadata_specs: &[MetadataSpec],
    controller_pipeline: &[proc_macro2::TokenStream],
    pipeline_specs: &[PipelineSpec],
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let pattern = spec.args.pattern;
    let raw = spec.args.raw.is_some();
    let args = message_handler_args(input)?;
    let definition = match spec.kind {
        MessagePatternAttrKind::Message => {
            let handler = message_request_handler(method_ident, raw, args.clone());
            quote! {
                ::a3s_boot::MessagePatternDefinition::request(#pattern, #handler)?
            }
        }
        MessagePatternAttrKind::Event => {
            let handler = message_event_handler(method_ident, args.clone());
            quote! {
                ::a3s_boot::MessagePatternDefinition::event(#pattern, #handler)?
            }
        }
    };

    let definition = message_validation_definition(definition, &args, validation_options)?;
    let definition = message_metadata_definition(definition, controller_metadata, metadata_specs);
    let pipeline_specs = pipeline_specs.iter().map(PipelineSpec::token);
    let definition = quote! {
        (#definition)#(.#controller_pipeline)*#(.#pipeline_specs)*
    };
    Ok(definition)
}

fn message_metadata_definition(
    mut definition: proc_macro2::TokenStream,
    controller_metadata: &[proc_macro2::TokenStream],
    metadata_specs: &[MetadataSpec],
) -> proc_macro2::TokenStream {
    for metadata in controller_metadata {
        definition = quote! {
            (#definition).#metadata?
        };
    }
    for spec in metadata_specs {
        let key = &spec.key;
        let value = &spec.value;
        definition = quote! {
            (#definition).with_metadata(#key, #value)?
        };
    }
    definition
}

fn message_request_handler(
    method_ident: &Ident,
    raw: bool,
    args: MessageHandlerArgs,
) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_message_{}", method_ident);
    let bindings = args.bindings();
    let call_args = args.call_args();
    let call = message_request_call(method_ident, &controller_name, raw, quote!(#(#call_args),*));
    quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#bindings)*
                    #call
                }
            }
        }
    }
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
    args: MessageHandlerArgs,
) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_event_{}", method_ident);
    let bindings = args.bindings();
    let call_args = args.call_args();
    quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_message: ::a3s_boot::TransportMessage| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#bindings)*
                    let _ = #controller_name.#method_ident(#(#call_args),*).await?;
                    Ok(())
                }
            }
        }
    }
}

fn message_validation_definition(
    definition: proc_macro2::TokenStream,
    args: &MessageHandlerArgs,
    validation_options: Option<ValidationAttrOptions>,
) -> Result<proc_macro2::TokenStream> {
    let Some(options) = validation_options else {
        return Ok(definition);
    };

    let Some(arg) = args.whole_payload_arg() else {
        return Err(syn::Error::new(
            proc_macro2::Span::call_site(),
            "message validation requires one whole typed payload argument",
        ));
    };

    if is_type_ident(&arg.ty, "TransportMessage") {
        return Err(syn::Error::new_spanned(
            arg.ident.clone(),
            "message validation requires a DTO payload argument, not TransportMessage",
        ));
    }

    let ty = &arg.ty;
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

#[derive(Clone)]
struct MessageHandlerArgs {
    args: Vec<MessageHandlerArg>,
}

impl MessageHandlerArgs {
    fn bindings(&self) -> Vec<proc_macro2::TokenStream> {
        self.args
            .iter()
            .map(|arg| {
                let MethodArg { ident, ty, .. } = &arg.arg;
                match &arg.kind {
                    MessageHandlerArgKind::Message => quote! {
                        let #ident: #ty = __a3s_boot_message.clone();
                    },
                    MessageHandlerArgKind::Payload(extractor) => json_payload_binding_tokens(
                        ident.clone(),
                        ty.clone(),
                        extractor.clone(),
                        |value_ty| quote!(__a3s_boot_message.data_as::<#value_ty>()),
                        |value_ty, name| {
                            quote!(__a3s_boot_message.data_field_as::<#value_ty>(#name))
                        },
                        |value_ty, name| {
                            quote!(__a3s_boot_message.optional_data_field_as::<#value_ty>(#name))
                        },
                        |name| quote!(__a3s_boot_message.data_field_string(#name)),
                        |name| quote!(__a3s_boot_message.optional_data_field_string(#name)),
                    ),
                }
            })
            .collect()
    }

    fn call_args(&self) -> Vec<Ident> {
        self.args.iter().map(|arg| arg.arg.ident.clone()).collect()
    }

    fn whole_payload_arg(&self) -> Option<&MethodArg> {
        self.args.iter().find_map(|arg| match &arg.kind {
            MessageHandlerArgKind::Payload(ProtocolPayloadExtractor::Whole) => Some(&arg.arg),
            MessageHandlerArgKind::Payload(ProtocolPayloadExtractor::Field(_))
            | MessageHandlerArgKind::Message => None,
        })
    }
}

#[derive(Clone)]
struct MessageHandlerArg {
    arg: MethodArg,
    kind: MessageHandlerArgKind,
}

#[derive(Clone)]
enum MessageHandlerArgKind {
    Message,
    Payload(ProtocolPayloadExtractor),
}

fn message_handler_args(input: RouteMethodInput) -> Result<MessageHandlerArgs> {
    if input.has_extractors() {
        return Err(syn::Error::new_spanned(
            input
                .args
                .iter()
                .find(|arg| arg.extractor.is_some())
                .map(|arg| arg.ident.clone())
                .unwrap_or_else(|| format_ident!("argument")),
            "message pattern methods do not support route extractor attributes",
        ));
    }

    if !input.has_protocol_extractors() {
        if input.args.len() > 1 {
            return Err(syn::Error::new_spanned(
                input.args[1].ident.clone(),
                "message pattern methods without #[payload] can accept at most one argument after &self",
            ));
        }

        return Ok(MessageHandlerArgs {
            args: input
                .args
                .into_iter()
                .map(|arg| {
                    let kind = if is_type_ident(&arg.ty, "TransportMessage") {
                        MessageHandlerArgKind::Message
                    } else {
                        MessageHandlerArgKind::Payload(ProtocolPayloadExtractor::Whole)
                    };
                    MessageHandlerArg { arg, kind }
                })
                .collect(),
        });
    }

    let mut whole_payload_arg = None;
    let mut field_payload_arg = None;
    let mut args = Vec::new();

    for arg in input.args {
        let Some(extractor) = arg.protocol_extractor.clone() else {
            return Err(syn::Error::new_spanned(
                arg.ident,
                "message pattern methods must use #[payload] on every argument when any protocol payload extractor is used",
            ));
        };

        let payload = match extractor {
            ProtocolExtractor::Payload(payload) => payload,
            ProtocolExtractor::MessageBody(_) => {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "message pattern methods support #[payload], not #[message_body]",
                ));
            }
        };

        match &payload {
            ProtocolPayloadExtractor::Whole => {
                if is_type_ident(&arg.ty, "TransportMessage") {
                    return Err(syn::Error::new_spanned(
                        arg.ident,
                        "whole #[payload] arguments must be DTOs; use an undecorated TransportMessage argument for raw access",
                    ));
                }
                if let Some(existing) = whole_payload_arg {
                    return Err(syn::Error::new_spanned(
                        existing,
                        "message pattern methods can accept at most one whole #[payload] argument",
                    ));
                }
                if let Some(existing) = &field_payload_arg {
                    return Err(syn::Error::new_spanned(
                        existing,
                        "message pattern methods cannot combine whole #[payload] arguments with #[payload(\"field\")] arguments",
                    ));
                }
                whole_payload_arg = Some(arg.ident.clone());
            }
            ProtocolPayloadExtractor::Field(_) => {
                if let Some(existing) = &whole_payload_arg {
                    return Err(syn::Error::new_spanned(
                        existing,
                        "message pattern methods cannot combine whole #[payload] arguments with #[payload(\"field\")] arguments",
                    ));
                }
                field_payload_arg.get_or_insert_with(|| arg.ident.clone());
            }
        }

        args.push(MessageHandlerArg {
            arg,
            kind: MessageHandlerArgKind::Payload(payload),
        });
    }

    Ok(MessageHandlerArgs { args })
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
