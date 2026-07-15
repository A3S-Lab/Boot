use crate::controller::ProtocolPayloadExtractor;
use crate::option_inner_type;
use quote::quote;
use syn::{Ident, LitStr, Type};

// Each callback represents a distinct payload source and target shape.
#[allow(clippy::too_many_arguments)]
pub(crate) fn json_payload_binding_tokens<
    Whole,
    Required,
    Optional,
    RequiredString,
    OptionalString,
>(
    ident: Ident,
    ty: Box<Type>,
    extractor: ProtocolPayloadExtractor,
    whole: Whole,
    required: Required,
    optional: Optional,
    required_string: RequiredString,
    optional_string: OptionalString,
) -> proc_macro2::TokenStream
where
    Whole: FnOnce(&Type) -> proc_macro2::TokenStream,
    Required: FnOnce(&Type, &LitStr) -> proc_macro2::TokenStream,
    Optional: FnOnce(&Type, &LitStr) -> proc_macro2::TokenStream,
    RequiredString: FnOnce(&LitStr) -> proc_macro2::TokenStream,
    OptionalString: FnOnce(&LitStr) -> proc_macro2::TokenStream,
{
    match extractor {
        ProtocolPayloadExtractor::Whole => {
            let value = whole(&ty);
            quote! {
                let #ident: #ty = #value?;
            }
        }
        ProtocolPayloadExtractor::Field(spec) => {
            let name = spec.name;
            let pipe = spec.pipe;
            let default = spec.default;
            if let Some(pipe) = pipe {
                if let Some(inner) = option_inner_type(&ty) {
                    let value = optional_string(&name);
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
                    let value = optional_string(&name);
                    quote! {
                        let __a3s_boot_value = match #value? {
                            Some(__a3s_boot_value) => __a3s_boot_value,
                            None => ::std::string::ToString::to_string(&(#default)),
                        };
                        let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                            __a3s_boot_value,
                            #pipe,
                        )?;
                    }
                } else {
                    let value = required_string(&name);
                    quote! {
                        let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                            #value?,
                            #pipe,
                        )?;
                    }
                }
            } else if let Some(inner) = option_inner_type(&ty) {
                let value = optional(&inner, &name);
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
                let value = optional(&ty, &name);
                quote! {
                    let #ident: #ty = match #value? {
                        Some(__a3s_boot_value) => __a3s_boot_value,
                        None => #default,
                    };
                }
            } else {
                let value = required(&ty, &name);
                quote! {
                    let #ident: #ty = #value?;
                }
            }
        }
    }
}
