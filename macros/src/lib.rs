use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    parse_macro_input, Attribute, Expr, Fields, FnArg, GenericArgument, Ident, ImplItem,
    ImplItemFn, Item, ItemImpl, LitBool, LitInt, LitStr, Meta, Pat, PatType, PathArguments, Result,
    Token, Type,
};

#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[injectable] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item = parse_macro_input!(item as Item);
    match item {
        Item::Struct(item_struct) => expand_injectable(item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[injectable] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ModuleArgs);
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => expand_module(args, item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[module] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn catch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as CatchArgs);
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => expand_catch(args, item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[catch] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    let prefix = parse_macro_input!(attr as LitStr);
    let item_impl = parse_macro_input!(item as ItemImpl);

    expand_controller(prefix, item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn websocket_gateway(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as WebSocketGatewayArgs);
    let item_impl = parse_macro_input!(item as ItemImpl);

    expand_websocket_gateway(args, item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn message_controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[message_controller] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_message_controller(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn event_listener(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[event_listener] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_event_listener(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn schedule(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[schedule] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_schedule(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn subscribe_message(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("subscribe_message", item)
}

#[proc_macro_attribute]
pub fn on_gateway_init(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_init", item)
}

#[proc_macro_attribute]
pub fn on_gateway_connection(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_connection", item)
}

#[proc_macro_attribute]
pub fn on_gateway_disconnect(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_disconnect", item)
}

#[proc_macro_attribute]
pub fn cron(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("cron", item)
}

#[proc_macro_attribute]
pub fn interval(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("interval", item)
}

#[proc_macro_attribute]
pub fn timeout(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("timeout", item)
}

#[proc_macro_attribute]
pub fn message_pattern(_attr: TokenStream, item: TokenStream) -> TokenStream {
    message_attribute_outside_controller("message_pattern", item)
}

#[proc_macro_attribute]
pub fn event_pattern(_attr: TokenStream, item: TokenStream) -> TokenStream {
    message_attribute_outside_controller("event_pattern", item)
}

#[proc_macro_attribute]
pub fn on_event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    event_attribute_outside_listener("on_event", item)
}

#[proc_macro_attribute]
pub fn all(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("all", item)
}

#[proc_macro_attribute]
pub fn get(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("get", item)
}

#[proc_macro_attribute]
pub fn sse(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("sse", item)
}

#[proc_macro_attribute]
pub fn post(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("post", item)
}

#[proc_macro_attribute]
pub fn put(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("put", item)
}

#[proc_macro_attribute]
pub fn patch(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("patch", item)
}

#[proc_macro_attribute]
pub fn delete(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("delete", item)
}

#[proc_macro_attribute]
pub fn options(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("options", item)
}

#[proc_macro_attribute]
pub fn head(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("head", item)
}

#[proc_macro_attribute]
pub fn get_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("get_json", item)
}

#[proc_macro_attribute]
pub fn post_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("post_json", item)
}

#[proc_macro_attribute]
pub fn put_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("put_json", item)
}

#[proc_macro_attribute]
pub fn patch_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("patch_json", item)
}

#[proc_macro_attribute]
pub fn delete_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("delete_json", item)
}

#[proc_macro_attribute]
pub fn body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("body", item)
}

#[proc_macro_attribute]
pub fn request(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("request", item)
}

#[proc_macro_attribute]
pub fn param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("param", item)
}

#[proc_macro_attribute]
pub fn params(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("params", item)
}

#[proc_macro_attribute]
pub fn query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("query", item)
}

#[proc_macro_attribute]
pub fn header(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("header", item)
}

#[proc_macro_attribute]
pub fn headers(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("headers", item)
}

#[proc_macro_attribute]
pub fn host_param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("host_param", item)
}

#[proc_macro_attribute]
pub fn ip(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("ip", item)
}

#[proc_macro_attribute]
pub fn extract(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("extract", item)
}

#[proc_macro_attribute]
pub fn host(_attr: TokenStream, item: TokenStream) -> TokenStream {
    host_attribute_outside_controller("host", item)
}

#[proc_macro_attribute]
pub fn version(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("version", item)
}

#[proc_macro_attribute]
pub fn versions(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("versions", item)
}

#[proc_macro_attribute]
pub fn version_neutral(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("version_neutral", item)
}

#[proc_macro_attribute]
pub fn serialize(_attr: TokenStream, item: TokenStream) -> TokenStream {
    serialization_attribute_outside_controller("serialize", item)
}

#[proc_macro_attribute]
pub fn tag(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("tag", item)
}

#[proc_macro_attribute]
pub fn operation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("operation", item)
}

#[proc_macro_attribute]
pub fn response(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("response", item)
}

#[proc_macro_attribute]
pub fn request_body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("request_body", item)
}

#[proc_macro_attribute]
pub fn bearer_auth(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("bearer_auth", item)
}

#[proc_macro_attribute]
pub fn hide_from_openapi(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("hide_from_openapi", item)
}

#[proc_macro_attribute]
pub fn redirect(_attr: TokenStream, item: TokenStream) -> TokenStream {
    response_attribute_outside_controller("redirect", item)
}

#[proc_macro_attribute]
pub fn render(_attr: TokenStream, item: TokenStream) -> TokenStream {
    render_attribute_outside_controller("render", item)
}

#[proc_macro_attribute]
pub fn http_code(_attr: TokenStream, item: TokenStream) -> TokenStream {
    http_code_attribute_outside_controller("http_code", item)
}

#[proc_macro_attribute]
pub fn metadata(_attr: TokenStream, item: TokenStream) -> TokenStream {
    metadata_attribute_outside_controller("metadata", item)
}

#[proc_macro_attribute]
pub fn validate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    validation_attribute_outside_controller("validate", item)
}

#[proc_macro_attribute]
pub fn skip_validation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    validation_attribute_outside_controller("skip_validation", item)
}

#[proc_macro_attribute]
pub fn use_guard(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_guard", item)
}

#[proc_macro_attribute]
pub fn use_interceptor(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_interceptor", item)
}

#[proc_macro_attribute]
pub fn use_filter(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_filter", item)
}

#[proc_macro_attribute]
pub fn use_pipe(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_pipe", item)
}

fn expand_injectable(mut item_struct: syn::ItemStruct) -> Result<proc_macro2::TokenStream> {
    let constructor = injectable_constructor(&mut item_struct)?;
    let ident = &item_struct.ident;
    let mut from_module_ref_generics = item_struct.generics.clone();
    from_module_ref_generics
        .make_where_clause()
        .predicates
        .push(syn::parse_quote!(Self: ::std::marker::Send + ::std::marker::Sync + 'static));
    let (from_impl_generics, _, from_where_clause) = from_module_ref_generics.split_for_impl();
    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    Ok(quote! {
        #item_struct

        impl #from_impl_generics ::a3s_boot::FromModuleRef for #ident #ty_generics #from_where_clause {
            fn from_module_ref(module_ref: &::a3s_boot::ModuleRef) -> ::a3s_boot::Result<Self> {
                #constructor
            }
        }

        impl #impl_generics #ident #ty_generics #where_clause {
            pub fn provider() -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::injectable::<Self>()
            }

            pub fn named_provider(token: impl Into<String>) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::named_injectable::<Self>(token)
            }

            pub fn request_scoped_provider() -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::request_scoped_injectable::<Self>()
            }

            pub fn named_request_scoped_provider(token: impl Into<String>) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::named_request_scoped_injectable::<Self>(token)
            }

            pub fn transient_provider() -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::transient_injectable::<Self>()
            }

            pub fn named_transient_provider(token: impl Into<String>) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::a3s_boot::FromModuleRef,
            {
                ::a3s_boot::ProviderDefinition::named_transient_injectable::<Self>(token)
            }

            pub fn into_provider(self) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::std::marker::Send + ::std::marker::Sync + 'static,
            {
                ::a3s_boot::ProviderDefinition::singleton(self)
            }

            pub fn into_named_provider(self, token: impl Into<String>) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::std::marker::Send + ::std::marker::Sync + 'static,
            {
                ::a3s_boot::ProviderDefinition::named_singleton(token, self)
            }

            pub fn from_arc_provider(value: ::std::sync::Arc<Self>) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::std::marker::Send + ::std::marker::Sync + 'static,
            {
                ::a3s_boot::ProviderDefinition::from_arc(value)
            }

            pub fn from_named_arc_provider(
                token: impl Into<String>,
                value: ::std::sync::Arc<Self>,
            ) -> ::a3s_boot::ProviderDefinition
            where
                Self: ::std::marker::Send + ::std::marker::Sync + 'static,
            {
                ::a3s_boot::ProviderDefinition::named_from_arc(token, value)
            }
        }
    })
}

fn expand_module(
    args: ModuleArgs,
    item_struct: syn::ItemStruct,
) -> Result<proc_macro2::TokenStream> {
    let ident = &item_struct.ident;
    let module_name = args.name.unwrap_or_else(|| {
        LitStr::new(
            &item_struct.ident.to_string(),
            proc_macro2::Span::call_site(),
        )
    });
    let imports = args.imports;
    let forward_imports = args.forward_imports;
    let provider_tokens = args.providers.iter().map(provider_registration_token);
    let exports = args.exports.iter().map(export_registration_token);
    let controllers = args.controllers;
    let routes = args.routes;
    let gateways = args.gateways;
    let message_controllers = args.message_controllers;
    let route_prefix = args.route_prefix;
    let route_prefix_body = match route_prefix {
        Some(route_prefix) => quote!(Some(#route_prefix)),
        None => quote!(None),
    };
    let controllers_body = if controllers.is_empty() {
        quote!({
            let _ = module_ref;
            Ok(::std::vec::Vec::new())
        })
    } else {
        quote! {
            Ok(::std::vec![
                #(module_ref.get::<#controllers>()?.controller()?,)*
            ])
        }
    };
    let gateways_body = if gateways.is_empty() {
        quote!({
            let _ = module_ref;
            Ok(::std::vec::Vec::new())
        })
    } else {
        quote! {
            Ok(::std::vec![
                #(module_ref.get::<#gateways>()?.gateway()?,)*
            ])
        }
    };
    let message_patterns_body = if message_controllers.is_empty() {
        quote!({
            let _ = module_ref;
            Ok(::std::vec::Vec::new())
        })
    } else {
        quote! {
            let mut __a3s_boot_patterns = ::std::vec::Vec::new();
            #(
                __a3s_boot_patterns.extend(
                    module_ref.get::<#message_controllers>()?.message_patterns()?
                );
            )*
            Ok(__a3s_boot_patterns)
        }
    };
    let global = args.global;
    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    Ok(quote! {
        #item_struct

        impl #impl_generics ::a3s_boot::Module for #ident #ty_generics #where_clause {
            fn name(&self) -> &'static str {
                #module_name
            }

            fn imports(&self) -> ::std::vec::Vec<::std::sync::Arc<dyn ::a3s_boot::Module>> {
                ::std::vec![#(::std::sync::Arc::new(#imports),)*]
            }

            fn forward_imports(&self) -> ::std::vec::Vec<::std::sync::Arc<dyn ::a3s_boot::Module>> {
                ::std::vec![#(::std::sync::Arc::new(#forward_imports),)*]
            }

            fn providers(&self) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::ProviderDefinition>> {
                Ok(::std::vec![#(#provider_tokens,)*])
            }

            fn exports(&self) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::ProviderToken>> {
                Ok(::std::vec![#(#exports,)*])
            }

            fn is_global(&self) -> bool {
                #global
            }

            fn route_prefix(&self) -> ::std::option::Option<&str> {
                #route_prefix_body
            }

            fn controllers(
                &self,
                module_ref: &::a3s_boot::ModuleRef,
            ) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::ControllerDefinition>> {
                #controllers_body
            }

            fn routes(&self) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::RouteDefinition>> {
                Ok(::std::vec![#(#routes,)*])
            }

            fn gateways(
                &self,
                module_ref: &::a3s_boot::ModuleRef,
            ) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::WebSocketGatewayDefinition>> {
                #gateways_body
            }

            fn message_patterns(
                &self,
                module_ref: &::a3s_boot::ModuleRef,
            ) -> ::a3s_boot::Result<::std::vec::Vec<::a3s_boot::MessagePatternDefinition>> {
                #message_patterns_body
            }
        }
    })
}

fn expand_catch(args: CatchArgs, item_struct: syn::ItemStruct) -> Result<proc_macro2::TokenStream> {
    let ident = &item_struct.ident;
    let catch_kinds = catch_kinds_token(&args.kinds);
    let catch_kinds_for_default = catch_kinds_token(&args.kinds);
    let caught_kinds = args.kinds.iter().map(catch_kind_token);
    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    Ok(quote! {
        #item_struct

        impl #impl_generics #ident #ty_generics #where_clause {
            pub fn catch_filter() -> ::a3s_boot::CatchFilter<Self>
            where
                Self: ::std::default::Default + ::a3s_boot::ExceptionFilter,
            {
                ::a3s_boot::catch_errors(#catch_kinds_for_default, Self::default())
            }

            pub fn catch_filter_with(filter: Self) -> ::a3s_boot::CatchFilter<Self>
            where
                Self: ::a3s_boot::ExceptionFilter,
            {
                ::a3s_boot::catch_errors(#catch_kinds, filter)
            }

            pub fn caught_kinds() -> ::std::vec::Vec<::a3s_boot::BootErrorKind> {
                ::std::vec![#(#caught_kinds),*]
            }
        }
    })
}

fn catch_kinds_token(kinds: &[Expr]) -> proc_macro2::TokenStream {
    if kinds.is_empty() {
        return quote!(::std::iter::empty::<::a3s_boot::BootErrorKind>());
    }

    let kinds = kinds.iter().map(catch_kind_token);
    quote!([#(#kinds),*])
}

fn catch_kind_token(expr: &Expr) -> proc_macro2::TokenStream {
    if let Expr::Path(path) = expr {
        if path.qself.is_none() && path.path.segments.len() == 1 {
            let ident = &path.path.segments[0].ident;
            if is_boot_error_kind_ident(ident) {
                return quote!(::a3s_boot::BootErrorKind::#ident);
            }
        }
    }

    quote!(#expr)
}

fn is_boot_error_kind_ident(ident: &Ident) -> bool {
    matches!(
        ident.to_string().as_str(),
        "EmptyModuleName"
            | "InvalidRoutePath"
            | "InvalidHostPattern"
            | "DuplicateRoute"
            | "NotFound"
            | "MethodNotAllowed"
            | "DuplicateProvider"
            | "MissingProvider"
            | "ProviderTypeMismatch"
            | "Forbidden"
            | "Unauthorized"
            | "BadRequest"
            | "PayloadTooLarge"
            | "UnsupportedMediaType"
            | "NotAcceptable"
            | "TooManyRequests"
            | "Adapter"
            | "Internal"
            | "Io"
    )
}

fn provider_registration_token(expr: &Expr) -> proc_macro2::TokenStream {
    match expr {
        Expr::Path(path) if path.qself.is_none() => {
            let path = &path.path;
            quote!(#path::provider())
        }
        expr => quote!(#expr),
    }
}

fn export_registration_token(spec: &ModuleExportSpec) -> proc_macro2::TokenStream {
    match spec {
        ModuleExportSpec::Type(ty) => quote!(::a3s_boot::ProviderToken::of::<#ty>()),
        ModuleExportSpec::Named(token) => quote!(::a3s_boot::ProviderToken::named(#token)),
    }
}

fn injectable_constructor(item_struct: &mut syn::ItemStruct) -> Result<proc_macro2::TokenStream> {
    match &mut item_struct.fields {
        Fields::Unit => Ok(quote! { Ok(Self) }),
        Fields::Named(fields) => {
            let mut values = Vec::new();
            for field in fields.named.iter_mut() {
                let ident = field.ident.clone().ok_or_else(|| {
                    syn::Error::new_spanned(&*field, "#[injectable] requires named fields")
                })?;
                let token = take_field_inject_attr(&mut field.attrs)?;
                let value = match injectable_field_dependency(&field.ty) {
                    Some(InjectableFieldDependency::Required(inner)) => match token {
                        Some(token) => quote! { module_ref.get_named::<#inner>(#token)? },
                        None => quote! { module_ref.get::<#inner>()? },
                    },
                    Some(InjectableFieldDependency::Optional(inner)) => match token {
                        Some(token) => quote! { module_ref.get_optional_named::<#inner>(#token)? },
                        None => quote! { module_ref.get_optional::<#inner>()? },
                    },
                    Some(InjectableFieldDependency::ProviderRef(inner)) => match token {
                        Some(token) => quote! { module_ref.named_provider_ref::<#inner>(#token) },
                        None => quote! { module_ref.provider_ref::<#inner>() },
                    },
                    Some(InjectableFieldDependency::OptionalProviderRef(inner)) => match token {
                        Some(token) => {
                            quote! { module_ref.optional_named_provider_ref::<#inner>(#token)? }
                        }
                        None => quote! { module_ref.optional_provider_ref::<#inner>()? },
                    },
                    None => {
                        return Err(syn::Error::new_spanned(
                            &field.ty,
                            "#[injectable] fields must be Arc<T>, Option<Arc<T>>, ProviderRef<T>, or Option<ProviderRef<T>>",
                        ));
                    }
                };
                values.push(quote! { #ident: #value });
            }

            Ok(quote! {
                Ok(Self {
                    #(#values,)*
                })
            })
        }
        Fields::Unnamed(fields) => Err(syn::Error::new_spanned(
            fields,
            "#[injectable] auto-wiring supports unit structs and structs with named fields",
        )),
    }
}

enum InjectableFieldDependency<'a> {
    Required(&'a Type),
    Optional(&'a Type),
    ProviderRef(&'a Type),
    OptionalProviderRef(&'a Type),
}

fn injectable_field_dependency(field_type: &Type) -> Option<InjectableFieldDependency<'_>> {
    if let Some(inner) = single_type_argument(field_type, "Arc") {
        return Some(InjectableFieldDependency::Required(inner));
    }
    if let Some(inner) = single_type_argument(field_type, "ProviderRef") {
        return Some(InjectableFieldDependency::ProviderRef(inner));
    }

    let inner = single_type_argument(field_type, "Option")?;
    if let Some(inner) = single_type_argument(inner, "Arc") {
        return Some(InjectableFieldDependency::Optional(inner));
    }
    if let Some(inner) = single_type_argument(inner, "ProviderRef") {
        return Some(InjectableFieldDependency::OptionalProviderRef(inner));
    }
    None
}

fn single_type_argument<'a>(field_type: &'a Type, outer: &str) -> Option<&'a Type> {
    let Type::Path(type_path) = field_type else {
        return None;
    };
    let segment = type_path.path.segments.last()?;
    if segment.ident != outer {
        return None;
    }
    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }
    let Some(GenericArgument::Type(inner)) = arguments.args.first() else {
        return None;
    };
    Some(inner)
}

fn take_field_inject_attr(attrs: &mut Vec<Attribute>) -> Result<Option<LitStr>> {
    let mut kept = Vec::with_capacity(attrs.len());
    let mut token = None;

    for attr in attrs.drain(..) {
        if is_field_inject_attr(&attr) {
            if token.is_some() {
                return Err(syn::Error::new_spanned(
                    attr,
                    "duplicate #[inject(...)] field attribute",
                ));
            }
            token = Some(attr.parse_args::<LitStr>().map_err(|_| {
                syn::Error::new_spanned(&attr, "#[inject(...)] expects a string token")
            })?);
        } else {
            kept.push(attr);
        }
    }

    *attrs = kept;
    Ok(token)
}

fn is_field_inject_attr(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .map(|segment| segment.ident == "inject")
        .unwrap_or(false)
}

fn expand_controller(prefix: LitStr, mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[controller] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut routes = Vec::new();
    let mut errors: Option<syn::Error> = None;
    let (clean_impl_attrs, controller_validation, controller_validation_errors) =
        take_controller_validation_attrs(&item_impl.attrs);
    let (clean_impl_attrs, controller_openapi, controller_openapi_errors) =
        take_controller_openapi_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_metadata, controller_metadata_errors) =
        take_controller_metadata_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_pipeline, controller_pipeline_errors) =
        take_controller_pipeline_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_host, controller_host_errors) =
        take_controller_host_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_version, controller_version_errors) =
        take_controller_version_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_serialization, controller_serialization_errors) =
        take_controller_serialization_attrs(&clean_impl_attrs);
    item_impl.attrs = clean_impl_attrs;
    for error in controller_validation_errors {
        push_error(&mut errors, error);
    }
    for error in controller_openapi_errors {
        push_error(&mut errors, error);
    }
    for error in controller_metadata_errors {
        push_error(&mut errors, error);
    }
    for error in controller_pipeline_errors {
        push_error(&mut errors, error);
    }
    for error in controller_host_errors {
        push_error(&mut errors, error);
    }
    for error in controller_version_errors {
        push_error(&mut errors, error);
    }
    for error in controller_serialization_errors {
        push_error(&mut errors, error);
    }
    let controller_openapi = controller_openapi.tokens();
    let controller_metadata = controller_metadata.tokens();
    let controller_pipeline = controller_pipeline.tokens();
    let controller_host = controller_host.tokens();
    let controller_version = controller_version.tokens();
    let controller_serialization = controller_serialization.tokens();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, method_routes, route_errors) = take_route_attrs(&method.attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        let (clean_attrs, openapi_specs, openapi_errors) = take_route_openapi_attrs(&clean_attrs);
        let (clean_attrs, metadata_specs, metadata_errors) =
            take_route_metadata_attrs(&clean_attrs);
        let (clean_attrs, http_code, http_code_errors) = take_route_http_code_attrs(&clean_attrs);
        let (clean_attrs, response_specs, response_errors) =
            take_route_response_attrs(&clean_attrs);
        let (clean_attrs, render_spec, render_errors) = take_route_render_attrs(&clean_attrs);
        let (clean_attrs, pipeline_specs, pipeline_errors) =
            take_route_pipeline_attrs(&clean_attrs);
        let (clean_attrs, host_specs, host_errors) = take_route_host_attrs(&clean_attrs);
        let (clean_attrs, version_specs, version_errors) = take_route_version_attrs(&clean_attrs);
        let (clean_attrs, serialization_specs, serialization_errors) =
            take_route_serialization_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in route_errors {
            push_error(&mut errors, error);
        }
        for error in validation_errors {
            push_error(&mut errors, error);
        }
        for error in openapi_errors {
            push_error(&mut errors, error);
        }
        for error in metadata_errors {
            push_error(&mut errors, error);
        }
        for error in http_code_errors {
            push_error(&mut errors, error);
        }
        for error in response_errors {
            push_error(&mut errors, error);
        }
        for error in render_errors {
            push_error(&mut errors, error);
        }
        for error in pipeline_errors {
            push_error(&mut errors, error);
        }
        for error in host_errors {
            push_error(&mut errors, error);
        }
        for error in version_errors {
            push_error(&mut errors, error);
        }
        for error in serialization_errors {
            push_error(&mut errors, error);
        }
        if method_routes.is_empty() && !openapi_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "OpenAPI route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !response_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "response route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && render_spec.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "render route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !pipeline_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "pipeline route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && host_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "host route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && version_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "version route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && serialization_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "serialization route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && http_code.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "http_code route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !metadata_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "metadata route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && route_validation.is_present() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "validation route attributes must be used on route methods",
                ),
            );
        }

        let input = if method_routes.is_empty() {
            None
        } else {
            match RouteMethodInput::from_method(method) {
                Ok(input) => Some(input),
                Err(error) => {
                    push_error(&mut errors, error);
                    None
                }
            }
        };

        for route in method_routes {
            let Some(input) = input.clone() else {
                continue;
            };
            let validation_enabled = route_validation.enabled(controller_validation.enabled);
            let validation_skipped = route_validation.skip;
            match route_registration(
                route,
                method,
                input,
                validation_enabled,
                validation_skipped,
                &metadata_specs,
                http_code.as_ref(),
                &response_specs,
                render_spec.as_ref(),
                &pipeline_specs,
                host_specs.as_ref(),
                version_specs.as_ref(),
                serialization_specs.as_ref(),
                &openapi_specs,
            ) {
                Ok(registration) => routes.push(registration),
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
            pub fn controller(
                self: ::std::sync::Arc<Self>,
            ) -> ::a3s_boot::Result<::a3s_boot::ControllerDefinition> {
                let mut __a3s_boot_controller =
                    ::a3s_boot::ControllerDefinition::new(#prefix)?;
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_openapi;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_metadata?;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_pipeline;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_host?;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_version;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_serialization;
                )*
                #(
                    __a3s_boot_controller = #routes;
                )*
                Ok(__a3s_boot_controller)
            }
        }
    })
}

struct WebSocketGatewayArgs {
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

fn expand_websocket_gateway(
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

fn expand_message_controller(mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
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

        let validation_enabled = route_validation.enabled(controller_validation.enabled);
        for spec in specs {
            match message_pattern_registration(method, input.clone(), spec, validation_enabled) {
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

fn expand_event_listener(mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
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

fn expand_schedule(mut item_impl: ItemImpl) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[schedule] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut jobs = Vec::new();
    let mut errors: Option<syn::Error> = None;

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, specs, schedule_errors) = take_schedule_job_attrs(&method.attrs);
        method.attrs = clean_attrs;
        for error in schedule_errors {
            push_error(&mut errors, error);
        }
        if specs.is_empty() {
            continue;
        }

        let input = match ScheduleMethodInput::from_method(method) {
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
                    "scheduled job methods must be async",
                ),
            );
            continue;
        }

        for spec in specs {
            match schedule_job_registration(method, &input, spec) {
                Ok(job) => jobs.push(job),
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
            pub fn scheduled_jobs(
                self: ::std::sync::Arc<Self>,
            ) -> ::std::vec::Vec<::a3s_boot::ScheduledJob> {
                let mut __a3s_boot_jobs = ::std::vec::Vec::new();
                #(
                    __a3s_boot_jobs.push(#jobs);
                )*
                __a3s_boot_jobs
            }
        }
    })
}

fn take_schedule_job_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<ScheduleJobSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = ScheduleJobKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind.parse_args(attr) {
            Ok(args) => specs.push(ScheduleJobSpec { kind, args }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

fn schedule_job_registration(
    method: &ImplItemFn,
    input: &ScheduleMethodInput,
    spec: ScheduleJobSpec,
) -> Result<proc_macro2::TokenStream> {
    let method_ident = &method.sig.ident;
    let name = spec.args.name_token(method_ident);
    let handler = scheduled_task_handler(method_ident, input);

    Ok(match spec.kind {
        ScheduleJobKind::Cron => {
            let ScheduleJobArgs::Cron(args) = spec.args else {
                unreachable!("cron schedule spec must use cron args")
            };
            let expression = args.expression;
            quote! {
                ::a3s_boot::ScheduledJob::cron(#name, #expression, #handler)
            }
        }
        ScheduleJobKind::Interval => {
            let ScheduleJobArgs::Interval(args) = spec.args else {
                unreachable!("interval schedule spec must use interval args")
            };
            let millis = args.duration_millis()?;
            quote! {
                ::a3s_boot::ScheduledJob::interval(
                    #name,
                    ::std::time::Duration::from_millis(#millis),
                    #handler
                )
            }
        }
        ScheduleJobKind::Timeout => {
            let ScheduleJobArgs::Timeout(args) = spec.args else {
                unreachable!("timeout schedule spec must use timeout args")
            };
            let millis = args.duration_millis()?;
            quote! {
                ::a3s_boot::ScheduledJob::timeout(
                    #name,
                    ::std::time::Duration::from_millis(#millis),
                    #handler
                )
            }
        }
    })
}

fn scheduled_task_handler(
    method_ident: &Ident,
    input: &ScheduleMethodInput,
) -> proc_macro2::TokenStream {
    let scheduled_name = format_ident!("__a3s_boot_scheduled_{}", method_ident);
    let (closure_arg, method_args) = if input.accepts_context {
        (quote!(__a3s_boot_context), quote!(__a3s_boot_context))
    } else {
        (quote!(_context), quote!())
    };

    quote! {
        {
            let #scheduled_name = ::std::sync::Arc::clone(&self);
            move |#closure_arg: ::a3s_boot::ScheduleContext| {
                let #scheduled_name = ::std::sync::Arc::clone(&#scheduled_name);
                async move { #scheduled_name.#method_ident(#method_args).await }
            }
        }
    }
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
    validation_enabled: bool,
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

    let definition = message_validation_definition(definition, input, validation_enabled)?;
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
    validation_enabled: bool,
) -> Result<proc_macro2::TokenStream> {
    if !validation_enabled {
        return Ok(definition);
    }

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
    Ok(quote! {
        (#definition).with_payload_validation::<#ty>()
    })
}

fn take_route_attrs(attrs: &[Attribute]) -> (Vec<Attribute>, Vec<RouteSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut routes = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = RouteKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match attr.parse_args::<RouteArgs>() {
            Ok(args) => routes.push(RouteSpec { kind, args }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, routes, errors)
}

fn route_registration(
    route: RouteSpec,
    method: &ImplItemFn,
    input: RouteMethodInput,
    validation_enabled: bool,
    validation_skipped: bool,
    metadata_specs: &[MetadataSpec],
    http_code: Option<&LitInt>,
    response_specs: &[RouteResponseSpec],
    render_spec: Option<&RenderSpec>,
    pipeline_specs: &[PipelineSpec],
    host_spec: Option<&HostSpec>,
    version_spec: Option<&VersionSpec>,
    serialization_spec: Option<&SerializationSpec>,
    openapi_specs: &[RouteOpenApiSpec],
) -> Result<proc_macro2::TokenStream> {
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &method.sig.fn_token,
            "controller route methods must be async",
        ));
    }

    let method_ident = &method.sig.ident;
    let explicit_status = route.args.explicit_status(http_code)?;
    let status = status_value(explicit_status)?;
    let path = route.args.path.clone();
    let metadata_input = input.clone();

    let raw = route.args.raw.is_some();
    if raw && route.kind.is_explicit_json() {
        return Err(syn::Error::new_spanned(
            route.args.raw.unwrap(),
            "raw is not supported on *_json route attributes",
        ));
    }
    if let Some(render_spec) = render_spec {
        if raw {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on raw route attributes",
            ));
        }
        if route.kind == RouteKind::Sse {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on SSE route attributes",
            ));
        }
        if route.kind.is_explicit_json() {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on *_json route attributes",
            ));
        }
    }

    let flavor = route.kind.flavor(raw);
    let mut json_success_status = None;
    let route_definition = if let Some(render_spec) = render_spec {
        let builder = route.kind.raw_builder_ident();
        let view = &render_spec.view;
        let handler = rendered_view_handler(method_ident, input.clone(), view, status.clone())?;
        quote! {
            ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                .with_response(#status, ::a3s_boot::OpenApiResponse::description("Success"))
                .with_metadata("render:view", #view)?
        }
    } else {
        match flavor {
            RouteFlavor::Sse => {
                if let Some(status) = explicit_status {
                    return Err(syn::Error::new_spanned(
                        status,
                        "status is not supported on SSE route attributes",
                    ));
                }
                if route.args.raw.is_some() {
                    return Err(syn::Error::new_spanned(
                        route.args.raw.unwrap(),
                        "raw is not supported on SSE route attributes",
                    ));
                }
                let handler = if input.has_extractors() {
                    extracted_sse_handler(method_ident, input)?
                } else {
                    raw_or_json_request_handler(method_ident, input)?
                };
                quote! {
                    ::a3s_boot::RouteDefinition::sse(#path, #handler)?
                }
            }
            RouteFlavor::Raw => {
                if let Some(status) = explicit_status {
                    return Err(syn::Error::new_spanned(
                        status,
                        "status is only supported on JSON route attributes",
                    ));
                }
                let builder = route.kind.raw_builder_ident();
                let handler = if input.has_extractors() {
                    extracted_raw_handler(method_ident, input)?
                } else {
                    raw_or_json_request_handler(method_ident, input)?
                };
                quote! {
                    ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                }
            }
            RouteFlavor::JsonRequest => {
                if input.has_extractors() {
                    let builder = route.kind.raw_builder_ident();
                    let handler =
                        extracted_json_response_handler(method_ident, input, status.clone())?;
                    json_success_status = Some(status.clone());
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                    }
                } else {
                    let builder = route.kind.json_builder_ident().ok_or_else(|| {
                        syn::Error::new_spanned(
                            &method.sig.ident,
                            "this HTTP method does not support JSON route inference",
                        )
                    })?;
                    let handler = raw_or_json_request_handler(method_ident, input)?;
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #status, #handler)?
                    }
                }
            }
            RouteFlavor::JsonBody => {
                if input.has_extractors() {
                    let builder = route.kind.raw_builder_ident();
                    let handler =
                        extracted_json_response_handler(method_ident, input, status.clone())?;
                    json_success_status = Some(status.clone());
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                    }
                } else {
                    let Some(input) = input.into_legacy_arg()? else {
                        return Err(syn::Error::new_spanned(
                            &method.sig.ident,
                            "JSON body routes must accept one DTO argument after &self",
                        ));
                    };
                    let builder = route.kind.json_builder_ident().ok_or_else(|| {
                        syn::Error::new_spanned(
                            &method.sig.ident,
                            "this HTTP method does not support JSON route inference",
                        )
                    })?;
                    let handler = json_body_handler(method_ident, input);
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #status, #handler)?
                    }
                }
            }
        }
    };

    let route_definition = validation_route_definition(
        route_definition,
        &metadata_input,
        flavor,
        validation_enabled,
        validation_skipped,
    )?;

    let route_definition = metadata_route_definition(route_definition, metadata_specs);

    let route_definition = pipeline_route_definition(route_definition, pipeline_specs);

    let route_definition = host_route_definition(route_definition, host_spec);

    let route_definition = version_route_definition(route_definition, version_spec);

    let route_definition = serialization_route_definition(route_definition, serialization_spec);

    let route_definition = response_route_definition(route_definition, response_specs)?;

    let route_definition = openapi_route_definition(
        route_definition,
        &metadata_input,
        flavor,
        json_success_status,
        openapi_specs,
    )?;

    Ok(quote! {
        __a3s_boot_controller.route(#route_definition)?
    })
}

fn metadata_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    metadata_specs: &[MetadataSpec],
) -> proc_macro2::TokenStream {
    for spec in metadata_specs {
        let key = &spec.key;
        let value = &spec.value;
        route_definition = quote! {
            (#route_definition).with_metadata(#key, #value)?
        };
    }
    route_definition
}

fn response_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    response_specs: &[RouteResponseSpec],
) -> Result<proc_macro2::TokenStream> {
    for spec in response_specs {
        let token = spec.token()?;
        route_definition = quote! {
            (#route_definition).#token
        };
    }
    Ok(route_definition)
}

fn pipeline_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    pipeline_specs: &[PipelineSpec],
) -> proc_macro2::TokenStream {
    for spec in pipeline_specs {
        let token = spec.token();
        route_definition = quote! {
            (#route_definition).#token
        };
    }
    route_definition
}

fn host_route_definition(
    route_definition: proc_macro2::TokenStream,
    host_spec: Option<&HostSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = host_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token?
    }
}

fn version_route_definition(
    route_definition: proc_macro2::TokenStream,
    version_spec: Option<&VersionSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = version_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token
    }
}

fn serialization_route_definition(
    route_definition: proc_macro2::TokenStream,
    serialization_spec: Option<&SerializationSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = serialization_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token
    }
}

fn validation_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    validation_enabled: bool,
    validation_skipped: bool,
) -> Result<proc_macro2::TokenStream> {
    if validation_skipped {
        return Ok(quote! {
            (#route_definition).without_validation()
        });
    }

    if !validation_enabled {
        return Ok(route_definition);
    }

    for token in extractor_validation_tokens(input, flavor) {
        route_definition = quote! {
            (#route_definition).#token
        };
    }

    Ok(quote! {
        (#route_definition).with_validation()
    })
}

fn extractor_validation_tokens(
    input: &RouteMethodInput,
    flavor: RouteFlavor,
) -> Vec<proc_macro2::TokenStream> {
    let mut tokens = Vec::new();

    if matches!(flavor, RouteFlavor::JsonBody) && !input.has_extractors() {
        if let Some(arg) = input.args.first() {
            let ty = &arg.ty;
            tokens.push(quote! {
                with_body_validation::<#ty>()
            });
        }
    }

    for arg in &input.args {
        let Some(extractor) = &arg.extractor else {
            continue;
        };
        let ty = &arg.ty;

        match extractor {
            Extractor::Body => tokens.push(quote! {
                with_body_validation::<#ty>()
            }),
            Extractor::Params => tokens.push(quote! {
                with_params_validation::<#ty>()
            }),
            Extractor::Query(query) => {
                if query.name.is_none() {
                    tokens.push(quote! {
                        with_query_validation::<#ty>()
                    });
                }
            }
            Extractor::Request
            | Extractor::Param(_)
            | Extractor::Header(_)
            | Extractor::Headers
            | Extractor::HostParam(_)
            | Extractor::Ip(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
}

fn raw_or_json_request_handler(
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

fn json_body_handler(method_ident: &Ident, input: MethodArg) -> proc_macro2::TokenStream {
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

fn extracted_raw_handler(
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

fn extracted_json_response_handler(
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

fn rendered_view_handler(
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

fn extracted_sse_handler(
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
            if let Some(existing) = body_arg {
                return Err(syn::Error::new_spanned(
                    existing,
                    "route methods can accept at most one #[body] argument",
                ));
            }
            body_arg = Some(arg.ident.clone());
        }

        args.push(arg.ident.clone());
        extractors.push(extractor_tokens(arg, extractor));
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

fn single_value_extractor_schema_type(extractor_ty: &Type, pipe: Option<&Expr>) -> Type {
    if pipe.is_some() {
        return syn::parse_quote!(String);
    }

    extractor_ty.clone()
}

fn single_value_extractor_required(ty: &Type) -> bool {
    option_inner_type(ty).is_none()
}

fn single_value_extractor_schema(ty: &Type, pipe: Option<&Expr>) -> proc_macro2::TokenStream {
    let schema_ty = single_value_extractor_schema_type(ty, pipe);
    openapi_schema_tokens(&schema_ty)
}

fn single_value_extractor_required_schema(
    ty: &Type,
    pipe: Option<&Expr>,
    default: Option<&Expr>,
) -> (bool, proc_macro2::TokenStream) {
    (
        default.is_none() && single_value_extractor_required(ty),
        single_value_extractor_schema(ty, pipe),
    )
}

fn single_value_extractor_openapi_tokens(
    name: &LitStr,
    ty: &Type,
    pipe: Option<&Expr>,
    default: Option<&Expr>,
    kind: SingleValueOpenApiKind,
) -> proc_macro2::TokenStream {
    match kind {
        SingleValueOpenApiKind::Path => {
            let schema = single_value_extractor_schema(ty, pipe);
            quote! {
                with_path_parameter(#name, #schema)
            }
        }
        SingleValueOpenApiKind::Query => {
            let (required, schema) = single_value_extractor_required_schema(ty, pipe, default);
            quote! {
                with_query_parameter(#name, #required, #schema)
            }
        }
        SingleValueOpenApiKind::Header => {
            let (required, schema) = single_value_extractor_required_schema(ty, pipe, default);
            quote! {
                with_header_parameter(#name, #required, #schema)
            }
        }
    }
}

enum SingleValueOpenApiKind {
    Path,
    Query,
    Header,
}

fn openapi_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    json_success_status: Option<proc_macro2::TokenStream>,
    specs: &[RouteOpenApiSpec],
) -> Result<proc_macro2::TokenStream> {
    if let Some(status) = json_success_status {
        route_definition = quote! {
            (#route_definition).with_response(
                #status,
                ::a3s_boot::OpenApiResponse::description("Success")
            )
        };
    }

    for token in extractor_openapi_tokens(input, flavor) {
        route_definition = quote! {
            (#route_definition).#token
        };
    }

    for spec in specs {
        for token in spec.tokens()? {
            route_definition = quote! {
                (#route_definition).#token
            };
        }
    }

    Ok(route_definition)
}

fn extractor_openapi_tokens(
    input: &RouteMethodInput,
    flavor: RouteFlavor,
) -> Vec<proc_macro2::TokenStream> {
    let mut tokens = Vec::new();

    if matches!(flavor, RouteFlavor::JsonBody) && !input.has_extractors() {
        if let Some(arg) = input.args.first() {
            let schema = openapi_schema_tokens(&arg.ty);
            tokens.push(quote! {
                with_json_request_body(#schema)
            });
        }
    }

    for arg in &input.args {
        let Some(extractor) = &arg.extractor else {
            continue;
        };

        match extractor {
            Extractor::Body => {
                let schema = openapi_schema_tokens(&arg.ty);
                tokens.push(quote! {
                    with_json_request_body(#schema)
                });
            }
            Extractor::Param(spec) => tokens.push(single_value_extractor_openapi_tokens(
                &spec.name,
                &arg.ty,
                spec.pipe.as_ref(),
                spec.default.as_ref(),
                SingleValueOpenApiKind::Path,
            )),
            Extractor::Query(spec) => {
                if let Some(name) = &spec.name {
                    tokens.push(single_value_extractor_openapi_tokens(
                        name,
                        &arg.ty,
                        spec.pipe.as_ref(),
                        spec.default.as_ref(),
                        SingleValueOpenApiKind::Query,
                    ));
                }
            }
            Extractor::Header(spec) => tokens.push(single_value_extractor_openapi_tokens(
                &spec.name,
                &arg.ty,
                spec.pipe.as_ref(),
                spec.default.as_ref(),
                SingleValueOpenApiKind::Header,
            )),
            Extractor::Request
            | Extractor::Params
            | Extractor::Headers
            | Extractor::HostParam(_)
            | Extractor::Ip(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
}

fn openapi_schema_tokens(ty: &Type) -> proc_macro2::TokenStream {
    if let Some(inner) = option_inner_type(ty) {
        return openapi_schema_tokens(&inner);
    }

    let Type::Path(type_path) = ty else {
        return quote!(::a3s_boot::OpenApiSchema::object());
    };
    let Some(segment) = type_path.path.segments.last() else {
        return quote!(::a3s_boot::OpenApiSchema::object());
    };
    let ident = &segment.ident;
    let ident_string = ident.to_string();

    if ident == "Vec" {
        if let PathArguments::AngleBracketed(arguments) = &segment.arguments {
            if let Some(GenericArgument::Type(inner)) = arguments.args.first() {
                let inner_schema = openapi_schema_tokens(inner);
                return quote!(::a3s_boot::OpenApiSchema::array(#inner_schema));
            }
        }
    }

    match ident_string.as_str() {
        "String" | "str" => quote!(::a3s_boot::OpenApiSchema::string()),
        "bool" => quote!(::a3s_boot::OpenApiSchema::boolean()),
        "i8" | "i16" | "i32" | "i64" | "i128" | "isize" | "u8" | "u16" | "u32" | "u64" | "u128"
        | "usize" => quote!(::a3s_boot::OpenApiSchema::integer()),
        "f32" | "f64" => quote!(::a3s_boot::OpenApiSchema::number()),
        _ => quote!(::a3s_boot::OpenApiSchema::reference(#ident_string)),
    }
}

fn take_controller_validation_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerValidationAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut validation = ControllerValidationAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = ValidationAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            ValidationAttrKind::Validate => {
                if let Err(error) = expect_no_extractor_args(attr, "validate") {
                    errors.push(error);
                } else if validation.enabled {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "duplicate #[validate] attribute",
                    ));
                } else {
                    validation.enabled = true;
                }
            }
            ValidationAttrKind::SkipValidation => errors.push(syn::Error::new_spanned(
                attr,
                "#[skip_validation] is only supported on route methods",
            )),
        }
    }

    (clean_attrs, validation, errors)
}

fn take_route_validation_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, RouteValidationAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut validation = RouteValidationAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = ValidationAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        if let Err(error) = expect_no_extractor_args(attr, kind.name()) {
            errors.push(error);
            continue;
        }

        match kind {
            ValidationAttrKind::Validate => validation.validate = true,
            ValidationAttrKind::SkipValidation => validation.skip = true,
        }
    }

    if validation.validate && validation.skip {
        errors.push(syn::Error::new(
            proc_macro2::Span::call_site(),
            "route methods cannot use both #[validate] and #[skip_validation]",
        ));
    }

    (clean_attrs, validation, errors)
}

fn take_controller_openapi_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerOpenApiAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut openapi = ControllerOpenApiAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = OpenApiAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            OpenApiAttrKind::Tag => match attr.parse_args::<LitStr>() {
                Ok(tag) => openapi.tags.push(tag),
                Err(error) => errors.push(error),
            },
            _ => errors.push(syn::Error::new_spanned(
                attr,
                "only #[tag(\"name\")] is supported on #[controller] impl blocks",
            )),
        }
    }

    (clean_attrs, openapi, errors)
}

fn take_route_openapi_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<RouteOpenApiSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = OpenApiAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind.parse_route_spec(attr) {
            Ok(spec) => specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

fn take_controller_metadata_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerMetadataAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut metadata = ControllerMetadataAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_metadata_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<MetadataSpec>() {
            Ok(spec) => metadata.specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, metadata, errors)
}

fn take_route_metadata_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<MetadataSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_metadata_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<MetadataSpec>() {
            Ok(spec) => specs.push(spec),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

fn take_route_http_code_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<LitInt>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut status = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_http_code_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitInt>() {
            Ok(value) if status.is_none() => status = Some(value),
            Ok(value) => errors.push(syn::Error::new_spanned(
                value,
                "route methods can use at most one #[http_code(...)] attribute",
            )),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, status, errors)
}

fn take_route_response_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<RouteResponseSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();
    let mut redirect_seen = false;

    for attr in attrs {
        let Some(kind) = ResponseAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            ResponseAttrKind::Header => match attr.parse_args::<ResponseHeaderArgs>() {
                Ok(args) => specs.push(RouteResponseSpec::Header(args)),
                Err(error) => errors.push(error),
            },
            ResponseAttrKind::Redirect => match attr.parse_args::<RedirectArgs>() {
                Ok(args) if !redirect_seen => {
                    redirect_seen = true;
                    specs.push(RouteResponseSpec::Redirect(args));
                }
                Ok(args) => errors.push(syn::Error::new_spanned(
                    args.location,
                    "route methods can use at most one #[redirect(...)] attribute",
                )),
                Err(error) => errors.push(error),
            },
        }
    }

    (clean_attrs, specs, errors)
}

fn take_route_render_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<RenderSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_render_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(view) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one #[render(...)] attribute",
                    ));
                } else {
                    spec = Some(RenderSpec { view });
                }
            }
            Err(_) => errors.push(syn::Error::new_spanned(
                attr,
                "#[render] requires one string literal argument",
            )),
        }
    }

    (clean_attrs, spec, errors)
}

fn take_controller_pipeline_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerPipelineAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut pipeline = ControllerPipelineAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = PipelineAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match attr.parse_args::<Expr>() {
            Ok(expr) => pipeline.specs.push(PipelineSpec { kind, expr }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, pipeline, errors)
}

fn take_route_pipeline_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<PipelineSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = PipelineAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match attr.parse_args::<Expr>() {
            Ok(expr) => specs.push(PipelineSpec { kind, expr }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, specs, errors)
}

fn take_controller_host_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerHostAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut host = ControllerHostAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_host_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(pattern) => {
                if host.pattern.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "controller impl blocks can use at most one #[host] attribute",
                    ));
                } else {
                    host.pattern = Some(pattern);
                }
            }
            Err(_) => errors.push(syn::Error::new_spanned(
                attr,
                "#[host] requires one string literal argument",
            )),
        }
    }

    (clean_attrs, host, errors)
}

fn take_route_host_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<HostSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_host_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<LitStr>() {
            Ok(pattern) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one #[host] attribute",
                    ));
                } else {
                    spec = Some(HostSpec { pattern });
                }
            }
            Err(_) => errors.push(syn::Error::new_spanned(
                attr,
                "#[host] requires one string literal argument",
            )),
        }
    }

    (clean_attrs, spec, errors)
}

fn is_host_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "host")
}

fn take_controller_version_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerVersionAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut version = ControllerVersionAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = VersionAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match VersionSpec::from_attribute(kind, attr) {
            Ok(spec) => {
                if version.spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "controller impl blocks can use at most one version attribute",
                    ));
                } else {
                    version.spec = Some(spec);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, version, errors)
}

fn take_route_version_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<VersionSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = VersionAttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match VersionSpec::from_attribute(kind, attr) {
            Ok(parsed) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one version attribute",
                    ));
                } else {
                    spec = Some(parsed);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, spec, errors)
}

fn take_controller_serialization_attrs(
    attrs: &[Attribute],
) -> (
    Vec<Attribute>,
    ControllerSerializationAttrs,
    Vec<syn::Error>,
) {
    let mut clean_attrs = Vec::new();
    let mut serialization = ControllerSerializationAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_serialization_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<SerializationSpec>() {
            Ok(spec) => {
                if serialization.spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "controller impl blocks can use at most one #[serialize] attribute",
                    ));
                } else {
                    serialization.spec = Some(spec);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, serialization, errors)
}

fn take_route_serialization_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<SerializationSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_serialization_attribute(attr) {
            clean_attrs.push(attr.clone());
            continue;
        }

        match attr.parse_args::<SerializationSpec>() {
            Ok(parsed) => {
                if spec.is_some() {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "route methods can use at most one #[serialize] attribute",
                    ));
                } else {
                    spec = Some(parsed);
                }
            }
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, spec, errors)
}

fn is_serialization_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "serialize")
}

fn is_metadata_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "metadata")
}

fn is_http_code_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "http_code")
}

fn is_render_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "render")
}

#[derive(Default)]
struct ModuleArgs {
    name: Option<LitStr>,
    imports: Vec<Expr>,
    forward_imports: Vec<Expr>,
    providers: Vec<Expr>,
    controllers: Vec<Type>,
    routes: Vec<Expr>,
    gateways: Vec<Type>,
    message_controllers: Vec<Type>,
    exports: Vec<ModuleExportSpec>,
    global: bool,
    route_prefix: Option<LitStr>,
}

impl Parse for ModuleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();
        let mut global_seen = false;

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            let key = name.to_string();

            if key == "global" {
                if global_seen {
                    return Err(syn::Error::new_spanned(name, "duplicate `global` option"));
                }
                global_seen = true;
                if input.peek(Token![=]) {
                    input.parse::<Token![=]>()?;
                    args.global = input.parse::<LitBool>()?.value;
                } else {
                    args.global = true;
                }
                parse_optional_comma(input)?;
                continue;
            }

            input.parse::<Token![=]>()?;
            match key.as_str() {
                "name" => set_once(&mut args.name, input.parse::<LitStr>()?, name)?,
                "route_prefix" => {
                    set_once(&mut args.route_prefix, input.parse::<LitStr>()?, name)?;
                }
                "imports" => args.imports.extend(parse_expr_array(input)?),
                "forward_imports" => args.forward_imports.extend(parse_expr_array(input)?),
                "providers" => args.providers.extend(parse_expr_array(input)?),
                "controllers" => args.controllers.extend(parse_type_array(input)?),
                "routes" => args.routes.extend(parse_expr_array(input)?),
                "gateways" => args.gateways.extend(parse_type_array(input)?),
                "message_controllers" | "messages" => {
                    args.message_controllers.extend(parse_type_array(input)?);
                }
                "exports" => args.exports.extend(parse_module_export_array(input)?),
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `name`, `route_prefix`, `imports`, `forward_imports`, `providers`, `controllers`, `routes`, `gateways`, `message_controllers`, `exports`, or `global`",
                    ));
                }
            }
            parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

enum ModuleExportSpec {
    Type(Type),
    Named(LitStr),
}

impl Parse for ModuleExportSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.peek(LitStr) {
            return Ok(Self::Named(input.parse()?));
        }
        Ok(Self::Type(input.parse()?))
    }
}

fn parse_expr_array(input: ParseStream<'_>) -> Result<Vec<Expr>> {
    let content;
    syn::bracketed!(content in input);
    Ok(Punctuated::<Expr, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

fn parse_type_array(input: ParseStream<'_>) -> Result<Vec<Type>> {
    let content;
    syn::bracketed!(content in input);
    Ok(Punctuated::<Type, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

fn parse_module_export_array(input: ParseStream<'_>) -> Result<Vec<ModuleExportSpec>> {
    let content;
    syn::bracketed!(content in input);
    Ok(
        Punctuated::<ModuleExportSpec, Token![,]>::parse_terminated(&content)?
            .into_iter()
            .collect(),
    )
}

struct CatchArgs {
    kinds: Vec<Expr>,
}

impl Parse for CatchArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        Ok(Self {
            kinds: Punctuated::<Expr, Token![,]>::parse_terminated(input)?
                .into_iter()
                .collect(),
        })
    }
}

#[derive(Default)]
struct ControllerOpenApiAttrs {
    tags: Vec<LitStr>,
}

impl ControllerOpenApiAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.tags.iter().map(|tag| quote!(with_tag(#tag))).collect()
    }
}

#[derive(Default)]
struct ControllerMetadataAttrs {
    specs: Vec<MetadataSpec>,
}

impl ControllerMetadataAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs
            .iter()
            .map(|spec| {
                let key = &spec.key;
                let value = &spec.value;
                quote!(with_metadata(#key, #value))
            })
            .collect()
    }
}

#[derive(Clone)]
struct MetadataSpec {
    key: LitStr,
    value: Expr,
}

impl Parse for MetadataSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let key = input.parse::<LitStr>()?;
        input.parse::<Token![,]>()?;
        let value = input.parse::<Expr>()?;
        parse_optional_comma(input)?;
        Ok(Self { key, value })
    }
}

#[derive(Default)]
struct ControllerPipelineAttrs {
    specs: Vec<PipelineSpec>,
}

impl ControllerPipelineAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs.iter().map(PipelineSpec::token).collect()
    }
}

#[derive(Clone)]
struct PipelineSpec {
    kind: PipelineAttrKind,
    expr: Expr,
}

impl PipelineSpec {
    fn token(&self) -> proc_macro2::TokenStream {
        let expr = &self.expr;
        match self.kind {
            PipelineAttrKind::Guard => quote!(with_guard(#expr)),
            PipelineAttrKind::Interceptor => quote!(with_interceptor(#expr)),
            PipelineAttrKind::Filter => quote!(with_filter(#expr)),
            PipelineAttrKind::Pipe => quote!(with_pipe(#expr)),
        }
    }
}

#[derive(Clone, Copy)]
enum PipelineAttrKind {
    Guard,
    Interceptor,
    Filter,
    Pipe,
}

impl PipelineAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "use_guard" => Some(Self::Guard),
            "use_interceptor" => Some(Self::Interceptor),
            "use_filter" => Some(Self::Filter),
            "use_pipe" => Some(Self::Pipe),
            _ => None,
        }
    }
}

#[derive(Default)]
struct ControllerHostAttrs {
    pattern: Option<LitStr>,
}

impl ControllerHostAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.pattern
            .iter()
            .map(|pattern| quote!(with_host(#pattern)))
            .collect()
    }
}

struct HostSpec {
    pattern: LitStr,
}

impl HostSpec {
    fn token(&self) -> proc_macro2::TokenStream {
        let pattern = &self.pattern;
        quote!(with_host(#pattern))
    }
}

#[derive(Default)]
struct ControllerVersionAttrs {
    spec: Option<VersionSpec>,
}

impl ControllerVersionAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(VersionSpec::token).collect()
    }
}

#[derive(Clone)]
enum VersionSpec {
    Version(LitStr),
    Versions(Vec<LitStr>),
    Neutral,
}

impl VersionSpec {
    fn from_attribute(kind: VersionAttrKind, attr: &Attribute) -> Result<Self> {
        match kind {
            VersionAttrKind::Version => {
                attr.parse_args::<LitStr>().map(Self::Version).map_err(|_| {
                    syn::Error::new_spanned(attr, "#[version] requires one string literal argument")
                })
            }
            VersionAttrKind::Versions => {
                let values = attr.parse_args::<VersionList>().map_err(|_| {
                    syn::Error::new_spanned(
                        attr,
                        "#[versions] requires one or more string literal arguments",
                    )
                })?;
                if values.0.is_empty() {
                    Err(syn::Error::new_spanned(
                        attr,
                        "#[versions] requires one or more string literal arguments",
                    ))
                } else {
                    Ok(Self::Versions(values.0))
                }
            }
            VersionAttrKind::Neutral => {
                expect_no_extractor_args(attr, "version_neutral")?;
                Ok(Self::Neutral)
            }
        }
    }

    fn token(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Version(version) => quote!(with_version(#version)),
            Self::Versions(versions) => quote!(with_versions([#(#versions),*])),
            Self::Neutral => quote!(version_neutral()),
        }
    }
}

#[derive(Clone, Copy)]
enum VersionAttrKind {
    Version,
    Versions,
    Neutral,
}

impl VersionAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "version" => Some(Self::Version),
            "versions" => Some(Self::Versions),
            "version_neutral" => Some(Self::Neutral),
            _ => None,
        }
    }
}

struct VersionList(Vec<LitStr>);

impl Parse for VersionList {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let values = Punctuated::<LitStr, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect();
        Ok(Self(values))
    }
}

#[derive(Default)]
struct ControllerSerializationAttrs {
    spec: Option<SerializationSpec>,
}

impl ControllerSerializationAttrs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(SerializationSpec::token).collect()
    }
}

#[derive(Clone, Default)]
struct SerializationSpec {
    include_fields: Vec<LitStr>,
    exclude_fields: Vec<LitStr>,
    skip_null_fields: bool,
}

impl SerializationSpec {
    fn token(&self) -> proc_macro2::TokenStream {
        let mut options = quote!(::a3s_boot::SerializationOptions::new());

        if !self.include_fields.is_empty() {
            let fields = &self.include_fields;
            options = quote! {
                (#options).include_fields([#(#fields),*])
            };
        }

        if !self.exclude_fields.is_empty() {
            let fields = &self.exclude_fields;
            options = quote! {
                (#options).exclude_fields([#(#fields),*])
            };
        }

        if self.skip_null_fields {
            options = quote! {
                (#options).skip_null_fields()
            };
        }

        quote!(with_serialization(#options))
    }
}

impl Parse for SerializationSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut spec = Self::default();

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            let key = name.to_string();
            match key.as_str() {
                "include" => {
                    input.parse::<Token![=]>()?;
                    spec.include_fields.extend(parse_lit_str_array(input)?);
                }
                "exclude" => {
                    input.parse::<Token![=]>()?;
                    spec.exclude_fields.extend(parse_lit_str_array(input)?);
                }
                "skip_null" => {
                    if input.peek(Token![=]) {
                        input.parse::<Token![=]>()?;
                        spec.skip_null_fields = input.parse::<LitBool>()?.value;
                    } else {
                        spec.skip_null_fields = true;
                    }
                }
                _ => {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `include`, `exclude`, or `skip_null`",
                    ));
                }
            }

            if input.is_empty() {
                break;
            }
            input.parse::<Token![,]>()?;
        }

        Ok(spec)
    }
}

fn parse_lit_str_array(input: ParseStream<'_>) -> Result<Vec<LitStr>> {
    let content;
    syn::bracketed!(content in input);
    Ok(Punctuated::<LitStr, Token![,]>::parse_terminated(&content)?
        .into_iter()
        .collect())
}

#[derive(Clone, Copy)]
enum ResponseAttrKind {
    Header,
    Redirect,
}

impl ResponseAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "header" => Some(Self::Header),
            "redirect" => Some(Self::Redirect),
            _ => None,
        }
    }
}

#[derive(Clone)]
enum RouteResponseSpec {
    Header(ResponseHeaderArgs),
    Redirect(RedirectArgs),
}

impl RouteResponseSpec {
    fn token(&self) -> Result<proc_macro2::TokenStream> {
        match self {
            Self::Header(args) => {
                let name = &args.name;
                let value = &args.value;
                Ok(quote!(with_response_header(#name, #value)))
            }
            Self::Redirect(args) => {
                let location = &args.location;
                let status = status_value(args.status.as_ref())?;
                Ok(quote!(with_redirect_status(#status, #location)))
            }
        }
    }
}

#[derive(Clone)]
struct RenderSpec {
    view: LitStr,
}

#[derive(Clone)]
struct ResponseHeaderArgs {
    name: LitStr,
    value: LitStr,
}

impl Parse for ResponseHeaderArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let name = input.parse::<LitStr>()?;
        input.parse::<Token![,]>()?;
        let value = input.parse::<LitStr>()?;
        parse_optional_comma(input)?;
        Ok(Self { name, value })
    }
}

#[derive(Clone)]
struct RedirectArgs {
    location: LitStr,
    status: Option<LitInt>,
}

impl Parse for RedirectArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let location = input.parse::<LitStr>()?;
        let mut status = None;

        if !input.is_empty() {
            input.parse::<Token![,]>()?;
            if input.peek(LitInt) {
                status = Some(input.parse::<LitInt>()?);
            } else {
                let name = input.parse::<Ident>()?;
                if name != "status" {
                    return Err(syn::Error::new_spanned(name, "expected `status`"));
                }
                input.parse::<Token![=]>()?;
                status = Some(input.parse::<LitInt>()?);
            }
            parse_optional_comma(input)?;
        }

        Ok(Self { location, status })
    }
}

#[derive(Clone, Copy)]
enum ValidationAttrKind {
    Validate,
    SkipValidation,
}

impl ValidationAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "validate" => Some(Self::Validate),
            "skip_validation" => Some(Self::SkipValidation),
            _ => None,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Self::Validate => "validate",
            Self::SkipValidation => "skip_validation",
        }
    }
}

#[derive(Clone, Copy, Default)]
struct ControllerValidationAttrs {
    enabled: bool,
}

#[derive(Clone, Copy, Default)]
struct RouteValidationAttrs {
    validate: bool,
    skip: bool,
}

impl RouteValidationAttrs {
    fn is_present(self) -> bool {
        self.validate || self.skip
    }

    fn enabled(self, controller_enabled: bool) -> bool {
        !self.skip && (controller_enabled || self.validate)
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

#[derive(Clone, Copy)]
enum ScheduleJobKind {
    Cron,
    Interval,
    Timeout,
}

impl ScheduleJobKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "cron" => Some(Self::Cron),
            "interval" => Some(Self::Interval),
            "timeout" => Some(Self::Timeout),
            _ => None,
        }
    }

    fn parse_args(self, attr: &Attribute) -> Result<ScheduleJobArgs> {
        match self {
            Self::Cron => attr
                .parse_args::<CronScheduleArgs>()
                .map(ScheduleJobArgs::Cron),
            Self::Interval => attr
                .parse_args::<DurationScheduleArgs>()
                .map(ScheduleJobArgs::Interval),
            Self::Timeout => attr
                .parse_args::<DurationScheduleArgs>()
                .map(ScheduleJobArgs::Timeout),
        }
    }
}

struct ScheduleJobSpec {
    kind: ScheduleJobKind,
    args: ScheduleJobArgs,
}

enum ScheduleJobArgs {
    Cron(CronScheduleArgs),
    Interval(DurationScheduleArgs),
    Timeout(DurationScheduleArgs),
}

impl ScheduleJobArgs {
    fn name_token(&self, method_ident: &Ident) -> proc_macro2::TokenStream {
        let explicit_name = match self {
            Self::Cron(args) => args.name.as_ref(),
            Self::Interval(args) | Self::Timeout(args) => args.name.as_ref(),
        };

        match explicit_name {
            Some(name) => quote!(#name),
            None => {
                let default_name = LitStr::new(&method_ident.to_string(), method_ident.span());
                quote!(#default_name)
            }
        }
    }
}

struct CronScheduleArgs {
    name: Option<LitStr>,
    expression: LitStr,
}

impl Parse for CronScheduleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let first = input.parse::<LitStr>()?;
        if input.is_empty() {
            return Ok(Self {
                name: None,
                expression: first,
            });
        }

        input.parse::<Token![,]>()?;
        let expression = input.parse::<LitStr>()?;
        parse_optional_comma(input)?;
        Ok(Self {
            name: Some(first),
            expression,
        })
    }
}

struct DurationScheduleArgs {
    name: Option<LitStr>,
    millis: LitInt,
}

impl DurationScheduleArgs {
    fn duration_millis(&self) -> Result<u64> {
        self.millis.base10_parse::<u64>()
    }
}

impl Parse for DurationScheduleArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let (name, millis) = if input.peek(LitInt) {
            (None, input.parse::<LitInt>()?)
        } else if input.peek(LitStr) {
            let name = input.parse::<LitStr>()?;
            input.parse::<Token![,]>()?;
            (Some(name), input.parse::<LitInt>()?)
        } else {
            return Err(input.error("expected milliseconds or a job name followed by milliseconds"));
        };

        parse_optional_comma(input)?;
        Ok(Self { name, millis })
    }
}

#[derive(Clone, Copy)]
struct ScheduleMethodInput {
    accepts_context: bool,
}

impl ScheduleMethodInput {
    fn from_method(method: &ImplItemFn) -> Result<Self> {
        let mut inputs = method.sig.inputs.iter();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "scheduled job methods must take &self as their first argument",
            ));
        };

        if receiver.reference.is_none() || receiver.mutability.is_some() {
            return Err(syn::Error::new_spanned(
                receiver,
                "scheduled job methods must use an immutable &self receiver",
            ));
        }

        let mut accepts_context = false;
        for (index, input) in inputs.enumerate() {
            let FnArg::Typed(input) = input else {
                return Err(syn::Error::new_spanned(
                    input,
                    "unexpected receiver argument",
                ));
            };

            if index > 0 {
                return Err(syn::Error::new_spanned(
                    input,
                    "scheduled job methods can accept at most one ScheduleContext argument after &self",
                ));
            }

            let Pat::Ident(_) = input.pat.as_ref() else {
                return Err(syn::Error::new_spanned(
                    &input.pat,
                    "scheduled job arguments must be simple identifiers",
                ));
            };
            accepts_context = true;
        }

        Ok(Self { accepts_context })
    }
}

#[derive(Clone)]
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

#[derive(Clone, Copy)]
enum OpenApiAttrKind {
    Tag,
    Operation,
    Response,
    RequestBody,
    BearerAuth,
    HideFromOpenApi,
}

impl OpenApiAttrKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "tag" => Some(Self::Tag),
            "operation" => Some(Self::Operation),
            "response" => Some(Self::Response),
            "request_body" => Some(Self::RequestBody),
            "bearer_auth" => Some(Self::BearerAuth),
            "hide_from_openapi" => Some(Self::HideFromOpenApi),
            _ => None,
        }
    }

    fn parse_route_spec(self, attr: &Attribute) -> Result<RouteOpenApiSpec> {
        match self {
            Self::Tag => attr.parse_args::<LitStr>().map(RouteOpenApiSpec::Tag),
            Self::Operation => attr
                .parse_args::<OperationArgs>()
                .map(RouteOpenApiSpec::Operation),
            Self::Response => attr
                .parse_args::<ResponseArgs>()
                .map(RouteOpenApiSpec::Response),
            Self::RequestBody => attr
                .parse_args::<RequestBodyArgs>()
                .map(RouteOpenApiSpec::RequestBody),
            Self::BearerAuth => {
                expect_no_extractor_args(attr, "bearer_auth")?;
                Ok(RouteOpenApiSpec::BearerAuth)
            }
            Self::HideFromOpenApi => {
                expect_no_extractor_args(attr, "hide_from_openapi")?;
                Ok(RouteOpenApiSpec::HideFromOpenApi)
            }
        }
    }
}

#[derive(Clone)]
enum RouteOpenApiSpec {
    Tag(LitStr),
    Operation(OperationArgs),
    Response(ResponseArgs),
    RequestBody(RequestBodyArgs),
    BearerAuth,
    HideFromOpenApi,
}

impl RouteOpenApiSpec {
    fn tokens(&self) -> Result<Vec<proc_macro2::TokenStream>> {
        match self {
            Self::Tag(tag) => Ok(vec![quote!(with_tag(#tag))]),
            Self::Operation(args) => Ok(args.tokens()),
            Self::Response(args) => args.tokens().map(|token| vec![token]),
            Self::RequestBody(args) => Ok(vec![args.tokens()]),
            Self::BearerAuth => Ok(vec![quote!(with_bearer_auth())]),
            Self::HideFromOpenApi => Ok(vec![quote!(hide_from_openapi())]),
        }
    }
}

#[derive(Clone, Default)]
struct OperationArgs {
    summary: Option<LitStr>,
    description: Option<LitStr>,
    operation_id: Option<LitStr>,
    deprecated: bool,
}

impl OperationArgs {
    fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        let mut tokens = Vec::new();
        if let Some(summary) = &self.summary {
            tokens.push(quote!(with_summary(#summary)));
        }
        if let Some(description) = &self.description {
            tokens.push(quote!(with_description(#description)));
        }
        if let Some(operation_id) = &self.operation_id {
            tokens.push(quote!(with_operation_id(#operation_id)));
        }
        if self.deprecated {
            tokens.push(quote!(with_deprecated()));
        }
        tokens
    }
}

impl Parse for OperationArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            if name == "deprecated" {
                if args.deprecated {
                    return Err(syn::Error::new_spanned(
                        name,
                        "duplicate `deprecated` option",
                    ));
                }
                args.deprecated = true;
            } else {
                input.parse::<Token![=]>()?;
                let value = input.parse::<LitStr>()?;
                if name == "summary" {
                    set_once(&mut args.summary, value, name)?;
                } else if name == "description" {
                    set_once(&mut args.description, value, name)?;
                } else if name == "operation_id" || name == "id" {
                    set_once(&mut args.operation_id, value, name)?;
                } else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `summary`, `description`, `operation_id`, or `deprecated`",
                    ));
                }
            }
            parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

#[derive(Clone)]
struct ResponseArgs {
    status: LitInt,
    description: Option<LitStr>,
    schema: Option<Type>,
}

impl ResponseArgs {
    fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let status = self.status.base10_parse::<u16>()?;
        let description = match &self.description {
            Some(description) => quote!(#description),
            None => quote!("Success"),
        };

        Ok(match &self.schema {
            Some(schema) => {
                let schema = openapi_schema_tokens(schema);
                quote!(with_json_response(#status, #description, #schema))
            }
            None => quote! {
                with_response(
                    #status,
                    ::a3s_boot::OpenApiResponse::description(#description)
                )
            },
        })
    }
}

impl Parse for ResponseArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut status = None;
        let mut description = None;
        let mut schema = None;

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            if name == "status" {
                set_once(&mut status, input.parse::<LitInt>()?, name)?;
            } else if name == "description" {
                set_once(&mut description, input.parse::<LitStr>()?, name)?;
            } else if name == "schema" || name == "ty" || name == "body" {
                set_once(&mut schema, input.parse::<Type>()?, name)?;
            } else {
                return Err(syn::Error::new_spanned(
                    name,
                    "expected `status`, `description`, or `schema`",
                ));
            }
            parse_optional_comma(input)?;
        }

        let Some(status) = status else {
            return Err(input.error("missing required `status` option"));
        };

        Ok(Self {
            status,
            description,
            schema,
        })
    }
}

#[derive(Clone, Default)]
struct RequestBodyArgs {
    schema: Option<Type>,
    description: Option<LitStr>,
    required: Option<LitBool>,
}

impl RequestBodyArgs {
    fn tokens(&self) -> proc_macro2::TokenStream {
        let schema = self
            .schema
            .as_ref()
            .map(openapi_schema_tokens)
            .unwrap_or_else(|| quote!(::a3s_boot::OpenApiSchema::object()));
        let mut request_body = quote!(::a3s_boot::OpenApiRequestBody::json(#schema));

        if let Some(description) = &self.description {
            request_body = quote!((#request_body).with_description(#description));
        }

        if self
            .required
            .as_ref()
            .is_some_and(|required| !required.value)
        {
            request_body = quote!((#request_body).optional());
        }

        quote!(with_request_body(#request_body))
    }
}

impl Parse for RequestBodyArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            if name == "schema" || name == "ty" || name == "body" {
                set_once(&mut args.schema, input.parse::<Type>()?, name)?;
            } else if name == "description" {
                set_once(&mut args.description, input.parse::<LitStr>()?, name)?;
            } else if name == "required" {
                set_once(&mut args.required, input.parse::<LitBool>()?, name)?;
            } else {
                return Err(syn::Error::new_spanned(
                    name,
                    "expected `schema`, `description`, or `required`",
                ));
            }
            parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

fn set_once<T>(slot: &mut Option<T>, value: T, name: Ident) -> Result<()> {
    if slot.is_some() {
        let message = format!("duplicate `{name}` option");
        return Err(syn::Error::new_spanned(&name, message));
    }
    *slot = Some(value);
    Ok(())
}

fn parse_optional_comma(input: ParseStream<'_>) -> Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    input.parse::<Token![,]>()?;
    Ok(())
}

fn route_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn extractor_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message = format!("#[{name}] must be used on a route method argument inside #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn openapi_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn response_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn render_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn http_code_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn metadata_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn message_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[message_controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn event_attribute_outside_listener(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[event_listener]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn websocket_attribute_outside_gateway(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[websocket_gateway]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn schedule_attribute_outside_schedule(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message = format!("#[{name}] must be used inside an impl block annotated with #[schedule]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn validation_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn pipeline_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn host_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn version_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn serialization_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

fn push_error(slot: &mut Option<syn::Error>, error: syn::Error) {
    if let Some(existing) = slot {
        existing.combine(error);
    } else {
        *slot = Some(error);
    }
}

fn is_type_ident(ty: &Type, ident: &str) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == ident)
}

struct RouteArgs {
    path: LitStr,
    status: Option<LitInt>,
    raw: Option<Ident>,
}

impl RouteArgs {
    fn explicit_status<'a>(&'a self, http_code: Option<&'a LitInt>) -> Result<Option<&'a LitInt>> {
        match (&self.status, http_code) {
            (Some(_), Some(http_code)) => Err(syn::Error::new_spanned(
                http_code,
                "route status cannot be set with both `status = ...` and #[http_code(...)]",
            )),
            (Some(status), None) => Ok(Some(status)),
            (None, Some(http_code)) => Ok(Some(http_code)),
            (None, None) => Ok(None),
        }
    }
}

fn status_value(status: Option<&LitInt>) -> Result<proc_macro2::TokenStream> {
    let Some(status) = status else {
        return Ok(quote!(200));
    };
    let value = status.base10_parse::<u16>()?;
    Ok(quote!(#value))
}

impl Parse for RouteArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let path = input.parse::<LitStr>()?;
        let mut status = None;
        let mut raw = None;

        if !input.is_empty() {
            while !input.is_empty() {
                input.parse::<Token![,]>()?;
                let name = input.parse::<Ident>()?;

                if name == "status" {
                    if status.is_some() {
                        return Err(syn::Error::new_spanned(name, "duplicate `status` option"));
                    }
                    input.parse::<Token![=]>()?;
                    status = Some(input.parse::<LitInt>()?);
                } else if name == "raw" {
                    if raw.is_some() {
                        return Err(syn::Error::new_spanned(name, "duplicate `raw` option"));
                    }
                    raw = Some(name);
                } else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `status = <u16>` or `raw`",
                    ));
                }
            }
        }

        if !input.is_empty() {
            return Err(input.error("unexpected route attribute arguments"));
        }

        Ok(Self { path, status, raw })
    }
}

struct RouteSpec {
    kind: RouteKind,
    args: RouteArgs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum RouteKind {
    All,
    Get,
    Sse,
    Post,
    Put,
    Patch,
    Delete,
    Options,
    Head,
    GetJson,
    PostJson,
    PutJson,
    PatchJson,
    DeleteJson,
}

impl RouteKind {
    fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "all" => Some(Self::All),
            "get" => Some(Self::Get),
            "sse" => Some(Self::Sse),
            "post" => Some(Self::Post),
            "put" => Some(Self::Put),
            "patch" => Some(Self::Patch),
            "delete" => Some(Self::Delete),
            "options" => Some(Self::Options),
            "head" => Some(Self::Head),
            "get_json" => Some(Self::GetJson),
            "post_json" => Some(Self::PostJson),
            "put_json" => Some(Self::PutJson),
            "patch_json" => Some(Self::PatchJson),
            "delete_json" => Some(Self::DeleteJson),
            _ => None,
        }
    }

    fn raw_builder_ident(self) -> Ident {
        match self {
            Self::All => format_ident!("all"),
            Self::Get => format_ident!("get"),
            Self::Sse => format_ident!("get"),
            Self::Post => format_ident!("post"),
            Self::Put => format_ident!("put"),
            Self::Patch => format_ident!("patch"),
            Self::Delete => format_ident!("delete"),
            Self::Options => format_ident!("options"),
            Self::Head => format_ident!("head"),
            Self::GetJson => format_ident!("get"),
            Self::PostJson => format_ident!("post"),
            Self::PutJson => format_ident!("put"),
            Self::PatchJson => format_ident!("patch"),
            Self::DeleteJson => format_ident!("delete"),
        }
    }

    fn json_builder_ident(self) -> Option<Ident> {
        match self {
            Self::All => Some(format_ident!("all_json_with_status")),
            Self::Get | Self::GetJson => Some(format_ident!("get_json_with_status")),
            Self::Post | Self::PostJson => Some(format_ident!("post_json_with_status")),
            Self::Put | Self::PutJson => Some(format_ident!("put_json_with_status")),
            Self::Patch | Self::PatchJson => Some(format_ident!("patch_json_with_status")),
            Self::Delete | Self::DeleteJson => Some(format_ident!("delete_json_with_status")),
            Self::Sse | Self::Options | Self::Head => None,
        }
    }

    fn is_explicit_json(self) -> bool {
        matches!(
            self,
            Self::GetJson | Self::PostJson | Self::PutJson | Self::PatchJson | Self::DeleteJson
        )
    }

    fn flavor(self, raw: bool) -> RouteFlavor {
        if matches!(self, Self::Sse) {
            return RouteFlavor::Sse;
        }

        if raw {
            return RouteFlavor::Raw;
        }

        match self {
            Self::Sse => RouteFlavor::Sse,
            Self::All | Self::Get | Self::GetJson | Self::Delete | Self::DeleteJson => {
                RouteFlavor::JsonRequest
            }
            Self::Post
            | Self::PostJson
            | Self::Put
            | Self::PutJson
            | Self::Patch
            | Self::PatchJson => RouteFlavor::JsonBody,
            Self::Options | Self::Head => RouteFlavor::Raw,
        }
    }
}

#[derive(Clone, Copy)]
enum RouteFlavor {
    Sse,
    Raw,
    JsonRequest,
    JsonBody,
}

#[derive(Clone)]
struct RouteMethodInput {
    args: Vec<MethodArg>,
}

impl RouteMethodInput {
    fn from_method(method: &mut ImplItemFn) -> Result<Self> {
        let mut inputs = method.sig.inputs.iter_mut();
        let Some(FnArg::Receiver(receiver)) = inputs.next() else {
            return Err(syn::Error::new_spanned(
                &method.sig.ident,
                "controller route methods must take &self as their first argument",
            ));
        };

        if receiver.reference.is_none() || receiver.mutability.is_some() {
            return Err(syn::Error::new_spanned(
                receiver,
                "controller route methods must use an immutable &self receiver",
            ));
        }

        let args = inputs
            .map(|input| match input {
                FnArg::Typed(input) => MethodArg::from_pat_type(input),
                FnArg::Receiver(receiver) => Err(syn::Error::new_spanned(
                    receiver,
                    "unexpected receiver argument",
                )),
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(Self { args })
    }

    fn has_extractors(&self) -> bool {
        self.args.iter().any(|arg| arg.extractor.is_some())
    }

    fn into_legacy_arg(self) -> Result<Option<MethodArg>> {
        if self.has_extractors() {
            return Err(syn::Error::new_spanned(
                self.args
                    .iter()
                    .find(|arg| arg.extractor.is_some())
                    .map(|arg| arg.ident.clone())
                    .unwrap_or_else(|| format_ident!("argument")),
                "route methods with extractor attributes must use extractor attributes on every argument",
            ));
        }

        if self.args.len() > 1 {
            return Err(syn::Error::new_spanned(
                self.args[1].ident.clone(),
                "controller route methods without extractor attributes can accept at most one argument after &self",
            ));
        }

        Ok(self.args.into_iter().next())
    }
}

#[derive(Clone)]
struct MethodArg {
    ident: Ident,
    ty: Box<Type>,
    extractor: Option<Extractor>,
}

impl MethodArg {
    fn from_pat_type(input: &mut PatType) -> Result<Self> {
        let ident = match input.pat.as_ref() {
            Pat::Ident(ident) => ident.ident.clone(),
            _ => {
                return Err(syn::Error::new_spanned(
                    &input.pat,
                    "controller route arguments must be simple identifiers",
                ));
            }
        };
        let extractor = take_extractor_attrs(input)?;

        Ok(Self {
            ident,
            ty: input.ty.clone(),
            extractor,
        })
    }
}

#[derive(Clone)]
enum Extractor {
    Body,
    Request,
    Params,
    Param(SingleValueExtractor),
    Query(QueryExtractor),
    Header(SingleValueExtractor),
    Headers,
    HostParam(SingleValueExtractor),
    Ip(Option<Expr>),
    Custom(Expr),
}

#[derive(Clone)]
struct SingleValueExtractor {
    name: LitStr,
    pipe: Option<Expr>,
    default: Option<Expr>,
}

#[derive(Clone)]
struct QueryExtractor {
    name: Option<LitStr>,
    pipe: Option<Expr>,
    default: Option<Expr>,
}

impl Extractor {
    fn from_attribute(attr: &Attribute) -> Result<Option<Self>> {
        let Some(ident) = attr.path().segments.last().map(|segment| &segment.ident) else {
            return Ok(None);
        };

        let extractor = if ident == "body" {
            expect_no_extractor_args(attr, "body")?;
            Self::Body
        } else if ident == "request" {
            expect_no_extractor_args(attr, "request")?;
            Self::Request
        } else if ident == "params" {
            expect_no_extractor_args(attr, "params")?;
            Self::Params
        } else if ident == "param" {
            Self::Param(parse_single_value_extractor(attr, "param")?)
        } else if ident == "query" {
            Self::Query(parse_query_extractor(attr)?)
        } else if ident == "header" {
            Self::Header(parse_single_value_extractor(attr, "header")?)
        } else if ident == "headers" {
            expect_no_extractor_args(attr, "headers")?;
            Self::Headers
        } else if ident == "host_param" {
            Self::HostParam(parse_single_value_extractor(attr, "host_param")?)
        } else if ident == "ip" {
            Self::Ip(parse_optional_pipe_only_extractor(attr, "ip")?)
        } else if ident == "extract" {
            Self::Custom(parse_extractor_expr(attr)?)
        } else {
            return Ok(None);
        };

        Ok(Some(extractor))
    }
}

fn take_extractor_attrs(input: &mut PatType) -> Result<Option<Extractor>> {
    let mut clean_attrs = Vec::new();
    let mut extractor = None;

    for attr in std::mem::take(&mut input.attrs) {
        let Some(parsed) = Extractor::from_attribute(&attr)? else {
            clean_attrs.push(attr);
            continue;
        };

        if extractor.is_some() {
            return Err(syn::Error::new_spanned(
                attr,
                "route arguments can use at most one extractor attribute",
            ));
        }
        extractor = Some(parsed);
    }

    input.attrs = clean_attrs;
    Ok(extractor)
}

fn expect_no_extractor_args(attr: &Attribute, name: &str) -> Result<()> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(()),
        _ => Err(syn::Error::new_spanned(
            attr,
            format!("#[{name}] does not accept arguments"),
        )),
    }
}

fn parse_extractor_expr(attr: &Attribute) -> Result<Expr> {
    attr.parse_args::<Expr>().map_err(|_| {
        syn::Error::new_spanned(attr, "#[extract] requires one request extractor expression")
    })
}

fn parse_single_value_extractor(attr: &Attribute, name: &str) -> Result<SingleValueExtractor> {
    attr.parse_args::<SingleValueExtractorArgs>()
        .map(|args| SingleValueExtractor {
            name: args.name,
            pipe: args.pipe,
            default: args.default,
        })
        .map_err(|_| {
            syn::Error::new_spanned(
                attr,
                format!("#[{name}] requires a string literal and optional `pipe = <expr>` or `default = <expr>`"),
            )
        })
}

fn parse_query_extractor(attr: &Attribute) -> Result<QueryExtractor> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(QueryExtractor {
            name: None,
            pipe: None,
            default: None,
        }),
        _ => attr
            .parse_args::<SingleValueExtractorArgs>()
            .map(|args| QueryExtractor {
                name: Some(args.name),
                pipe: args.pipe,
                default: args.default,
            })
            .map_err(|_| {
                syn::Error::new_spanned(
                    attr,
                    "#[query] accepts no arguments for a DTO or a string literal and optional `pipe = <expr>` or `default = <expr>` for one value",
                )
            }),
    }
}

fn parse_optional_pipe_only_extractor(attr: &Attribute, name: &str) -> Result<Option<Expr>> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(None),
        _ => attr
            .parse_args::<PipeOnlyExtractorArgs>()
            .map(|args| Some(args.pipe))
            .map_err(|_| {
                syn::Error::new_spanned(
                    attr,
                    format!("#[{name}] accepts no arguments or `pipe = <expr>`"),
                )
            }),
    }
}

struct SingleValueExtractorArgs {
    name: LitStr,
    pipe: Option<Expr>,
    default: Option<Expr>,
}

impl Parse for SingleValueExtractorArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let name = input.parse::<LitStr>()?;
        let (pipe, default) = parse_optional_value_options(input)?;
        Ok(Self {
            name,
            pipe,
            default,
        })
    }
}

struct PipeOnlyExtractorArgs {
    pipe: Expr,
}

impl Parse for PipeOnlyExtractorArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let pipe = parse_required_pipe_arg(input)?;
        Ok(Self { pipe })
    }
}

fn parse_optional_value_options(input: ParseStream<'_>) -> Result<(Option<Expr>, Option<Expr>)> {
    let mut pipe = None;
    let mut default = None;

    while !input.is_empty() {
        input.parse::<Token![,]>()?;
        if input.is_empty() {
            break;
        }
        let ident = input.parse::<Ident>()?;
        if ident == "pipe" {
            if pipe.is_some() {
                return Err(syn::Error::new_spanned(ident, "duplicate `pipe` option"));
            }
            input.parse::<Token![=]>()?;
            pipe = Some(input.parse::<Expr>()?);
        } else if ident == "default" {
            if default.is_some() {
                return Err(syn::Error::new_spanned(ident, "duplicate `default` option"));
            }
            input.parse::<Token![=]>()?;
            default = Some(input.parse::<Expr>()?);
        } else {
            return Err(syn::Error::new_spanned(
                ident,
                "expected `pipe` or `default`",
            ));
        }
    }

    Ok((pipe, default))
}

fn parse_required_pipe_arg(input: ParseStream<'_>) -> Result<Expr> {
    let ident = input.parse::<Ident>()?;
    if ident != "pipe" {
        return Err(syn::Error::new_spanned(ident, "expected `pipe`"));
    }
    input.parse::<Token![=]>()?;
    let pipe = input.parse::<Expr>()?;
    parse_optional_comma(input)?;
    Ok(pipe)
}

fn option_inner_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }

    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }

    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }

    match arguments.args.first()? {
        GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    }
}
