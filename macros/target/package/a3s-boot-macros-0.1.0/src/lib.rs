use proc_macro::TokenStream;
use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{
    parse_macro_input, Attribute, FnArg, Ident, ImplItem, ImplItemFn, Item, ItemImpl, LitInt,
    LitStr, Pat, PatType, Result, Token,
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
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    let prefix = parse_macro_input!(attr as LitStr);
    let item_impl = parse_macro_input!(item as ItemImpl);

    expand_controller(prefix, item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
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

fn expand_injectable(item_struct: syn::ItemStruct) -> Result<proc_macro2::TokenStream> {
    let ident = &item_struct.ident;
    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();

    Ok(quote! {
        #item_struct

        impl #impl_generics #ident #ty_generics #where_clause {
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

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, method_routes, route_errors) = take_route_attrs(&method.attrs);
        method.attrs = clean_attrs;
        for error in route_errors {
            push_error(&mut errors, error);
        }

        for route in method_routes {
            match route_registration(route, method) {
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
                    __a3s_boot_controller = #routes;
                )*
                Ok(__a3s_boot_controller)
            }
        }
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

fn route_registration(route: RouteSpec, method: &ImplItemFn) -> Result<proc_macro2::TokenStream> {
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &method.sig.fn_token,
            "controller route methods must be async",
        ));
    }

    let method_ident = &method.sig.ident;
    let input = RouteMethodInput::from_method(method)?;
    let status = route.args.status_value()?;
    let path = route.args.path;

    let raw = route.args.raw.is_some();
    if raw && route.kind.is_explicit_json() {
        return Err(syn::Error::new_spanned(
            route.args.raw.unwrap(),
            "raw is not supported on *_json route attributes",
        ));
    }

    match route.kind.flavor(raw) {
        RouteFlavor::Sse => {
            if route.args.status.is_some() {
                return Err(syn::Error::new_spanned(
                    route.args.status.unwrap(),
                    "status is not supported on SSE route attributes",
                ));
            }
            if route.args.raw.is_some() {
                return Err(syn::Error::new_spanned(
                    route.args.raw.unwrap(),
                    "raw is not supported on SSE route attributes",
                ));
            }
            let handler = raw_or_json_request_handler(method_ident, input);
            Ok(quote! {
                __a3s_boot_controller.sse(#path, #handler)?
            })
        }
        RouteFlavor::Raw => {
            if route.args.status.is_some() {
                return Err(syn::Error::new_spanned(
                    route.args.status.unwrap(),
                    "status is only supported on JSON route attributes",
                ));
            }
            let builder = route.kind.raw_builder_ident();
            let handler = raw_or_json_request_handler(method_ident, input);
            Ok(quote! {
                __a3s_boot_controller.#builder(#path, #handler)?
            })
        }
        RouteFlavor::JsonRequest => {
            let builder = route.kind.json_builder_ident().ok_or_else(|| {
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "this HTTP method does not support JSON route inference",
                )
            })?;
            let handler = raw_or_json_request_handler(method_ident, input);
            Ok(quote! {
                __a3s_boot_controller.#builder(#path, #status, #handler)?
            })
        }
        RouteFlavor::JsonBody => {
            let Some(input) = input.arg else {
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
            Ok(quote! {
                __a3s_boot_controller.#builder(#path, #status, #handler)?
            })
        }
    }
}

fn raw_or_json_request_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    match input.arg {
        Some(MethodArg { ident, ty }) => quote! {
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
    }
}

fn json_body_handler(method_ident: &Ident, input: MethodArg) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let MethodArg { ident, ty } = input;
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

fn push_error(slot: &mut Option<syn::Error>, error: syn::Error) {
    if let Some(existing) = slot {
        existing.combine(error);
    } else {
        *slot = Some(error);
    }
}

struct RouteArgs {
    path: LitStr,
    status: Option<LitInt>,
    raw: Option<Ident>,
}

impl RouteArgs {
    fn status_value(&self) -> Result<proc_macro2::TokenStream> {
        let Some(status) = &self.status else {
            return Ok(quote!(200));
        };
        let value = status.base10_parse::<u16>()?;
        Ok(quote!(#value))
    }
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

#[derive(Clone, Copy)]
enum RouteKind {
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
            Self::Get | Self::GetJson | Self::Delete | Self::DeleteJson => RouteFlavor::JsonRequest,
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

enum RouteFlavor {
    Sse,
    Raw,
    JsonRequest,
    JsonBody,
}

struct RouteMethodInput {
    arg: Option<MethodArg>,
}

impl RouteMethodInput {
    fn from_method(method: &ImplItemFn) -> Result<Self> {
        let mut inputs = method.sig.inputs.iter();
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

        match args.len() {
            0 => Ok(Self { arg: None }),
            1 => Ok(Self {
                arg: args.into_iter().next(),
            }),
            _ => Err(syn::Error::new_spanned(
                &method.sig.inputs,
                "controller route methods can accept at most one argument after &self",
            )),
        }
    }
}

struct MethodArg {
    ident: Ident,
    ty: Box<syn::Type>,
}

impl MethodArg {
    fn from_pat_type(input: &PatType) -> Result<Self> {
        let Pat::Ident(ident) = input.pat.as_ref() else {
            return Err(syn::Error::new_spanned(
                &input.pat,
                "controller route arguments must be simple identifiers",
            ));
        };

        Ok(Self {
            ident: ident.ident.clone(),
            ty: input.ty.clone(),
        })
    }
}
