use quote::{format_ident, quote};
use syn::{Attribute, FnArg, Ident, ImplItem, ImplItemFn, ItemImpl, LitStr, Pat, Result, Type};

use crate::{is_type_ident, push_error};

pub(crate) fn expand_event_listener(mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[event_listener] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut listeners = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, patterns, event_errors) = take_on_event_attrs(&method.attrs);
        method.attrs = clean_attrs;
        for error in event_errors {
            push_error(&mut errors, error);
        }
        if patterns.is_empty() {
            continue;
        }

        let input = match EventMethodInput::from_method(method) {
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
                    "event listener methods must be async",
                ),
            );
            continue;
        }

        for pattern in patterns {
            match event_listener_registration(method, &input, pattern) {
                Ok(listener) => listeners.push(listener),
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
            pub fn event_listeners(
                self: ::std::sync::Arc<Self>,
            ) -> ::std::vec::Vec<::a3s_boot::EventListenerDefinition> {
                let mut __a3s_boot_listeners = ::std::vec::Vec::new();
                #(
                    __a3s_boot_listeners.push(#listeners);
                )*
                __a3s_boot_listeners
            }
        }
    })
}

fn take_on_event_attrs(attrs: &[Attribute]) -> (Vec<Attribute>, Vec<LitStr>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut patterns = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(ident) = attr.path().segments.last().map(|segment| &segment.ident) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        if ident != "on_event" {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(pattern) => patterns.push(pattern),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, patterns, errors)
}

fn event_listener_registration(
    method: &ImplItemFn,
    input: &EventMethodInput,
    pattern: LitStr,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let listener_name = format_ident!("__a3s_boot_event_listener_{}", method_ident);
    let (extractors, args) = input.argument_tokens();

    Ok(quote! {
        ::a3s_boot::EventListenerDefinition::new(#pattern, {
            let #listener_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_event: ::a3s_boot::EventEnvelope,
                  __a3s_boot_context: ::a3s_boot::EventContext| {
                let #listener_name = ::std::sync::Arc::clone(&#listener_name);
                async move {
                    #(#extractors)*
                    #listener_name.#method_ident(#(#args),*).await
                }
            }
        })
    })
}

struct EventMethodInput {
    args: Vec<EventMethodArg>,
}

impl EventMethodInput {
    fn from_method(method: &ImplItemFn) -> Result<Self> {
        let mut inputs = method.sig.inputs.iter();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "event listener methods must take &self as their first argument",
            ));
        };

        if receiver.reference.is_none() || receiver.mutability.is_some() {
            return Err(syn::Error::new_spanned(
                receiver,
                "event listener methods must use an immutable &self receiver",
            ));
        }

        let mut args = Vec::new();
        let mut event_arg_seen = false;
        let mut context_seen = false;

        for (index, input) in inputs.enumerate() {
            let FnArg::Typed(input) = input else {
                return Err(syn::Error::new_spanned(
                    input,
                    "unexpected receiver argument",
                ));
            };

            if index > 1 {
                return Err(syn::Error::new_spanned(
                    input,
                    "event listener methods can accept at most one event argument and one EventContext argument after &self",
                ));
            }

            let Pat::Ident(ident) = input.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &input.pat,
                    "event listener arguments must be simple identifiers",
                ));
            };

            let ty = input.ty.clone();
            let kind = if is_type_ident(&ty, "EventEnvelope") {
                EventMethodArgKind::Envelope
            } else if is_type_ident(&ty, "EventContext") {
                EventMethodArgKind::Context
            } else {
                EventMethodArgKind::Payload
            };

            match kind {
                EventMethodArgKind::Envelope | EventMethodArgKind::Payload => {
                    if event_arg_seen {
                        return Err(syn::Error::new_spanned(
                            input,
                            "event listener methods can accept at most one event payload or EventEnvelope argument",
                        ));
                    }
                    event_arg_seen = true;
                }
                EventMethodArgKind::Context => {
                    if context_seen {
                        return Err(syn::Error::new_spanned(
                            input,
                            "event listener methods can accept at most one EventContext argument",
                        ));
                    }
                    context_seen = true;
                }
            }

            args.push(EventMethodArg {
                ident: ident.ident.clone(),
                ty,
                kind,
            });
        }

        Ok(Self { args })
    }

    fn argument_tokens(&self) -> (Vec<proc_macro2::TokenStream>, Vec<Ident>) {
        let mut extractors = Vec::new();
        let mut args = Vec::new();

        for arg in &self.args {
            let ident = &arg.ident;
            let ty = &arg.ty;
            args.push(ident.clone());
            extractors.push(match arg.kind {
                EventMethodArgKind::Envelope => quote! {
                    let #ident: #ty = __a3s_boot_event.clone();
                },
                EventMethodArgKind::Context => quote! {
                    let #ident: #ty = __a3s_boot_context.clone();
                },
                EventMethodArgKind::Payload => quote! {
                    let #ident: #ty = __a3s_boot_event.data_as::<#ty>()?;
                },
            });
        }

        (extractors, args)
    }
}

struct EventMethodArg {
    ident: Ident,
    ty: Box<Type>,
    kind: EventMethodArgKind,
}

#[derive(Clone, Copy)]
enum EventMethodArgKind {
    Envelope,
    Context,
    Payload,
}
