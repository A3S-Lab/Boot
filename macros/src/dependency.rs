use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{
    Attribute, Expr, Fields, GenericArgument, Ident, LitBool, LitStr, PathArguments, Result, Token,
    Type,
};

use crate::{parse_optional_comma, set_once};

pub(crate) fn expand_injectable(
    mut item_struct: syn::ItemStruct,
) -> Result<proc_macro2::TokenStream> {
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

pub(crate) fn expand_module(
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

pub(crate) fn expand_catch(
    args: CatchArgs,
    item_struct: syn::ItemStruct,
) -> Result<proc_macro2::TokenStream> {
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
            | "HttpException"
            | "Forbidden"
            | "Unauthorized"
            | "BadRequest"
            | "RequestTimeout"
            | "Conflict"
            | "Gone"
            | "PreconditionFailed"
            | "PayloadTooLarge"
            | "UnsupportedMediaType"
            | "NotAcceptable"
            | "ImATeapot"
            | "UnprocessableEntity"
            | "TooManyRequests"
            | "InternalServerError"
            | "NotImplemented"
            | "BadGateway"
            | "ServiceUnavailable"
            | "GatewayTimeout"
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

#[derive(Default)]
pub(crate) struct ModuleArgs {
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
    Type(Box<Type>),
    Named(LitStr),
}

impl Parse for ModuleExportSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.peek(LitStr) {
            return Ok(Self::Named(input.parse()?));
        }
        Ok(Self::Type(Box::new(input.parse()?)))
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

pub(crate) struct CatchArgs {
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
