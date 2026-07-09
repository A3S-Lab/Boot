use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{bracketed, Attribute, Ident, LitStr, Result, Token};

#[derive(Clone, Default)]
pub(crate) struct ApiSecurityArgs {
    name: Option<LitStr>,
    scopes: Vec<LitStr>,
}

impl ApiSecurityArgs {
    pub(crate) fn tokens(&self) -> proc_macro2::TokenStream {
        let name = self.name.as_ref().expect("checked during parsing");
        let scopes = self
            .scopes
            .iter()
            .map(|scope| quote!(#scope.to_string()))
            .collect::<Vec<_>>();

        quote!(with_api_security(#name, vec![#(#scopes),*]))
    }
}

impl Parse for ApiSecurityArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        if input.peek(LitStr) {
            args.name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
        }

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "scheme" {
                crate::set_once(&mut args.name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "scopes" {
                if !args.scopes.is_empty() {
                    return Err(syn::Error::new_spanned(ident, "duplicate `scopes` option"));
                }
                args.scopes = parse_string_array(input)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `scheme`, or `scopes`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        if args.name.is_none() {
            return Err(input.error("missing required security scheme name"));
        }

        Ok(args)
    }
}

#[derive(Clone, Default)]
pub(crate) struct BearerAuthArgs {
    name: Option<LitStr>,
}

impl BearerAuthArgs {
    pub(crate) fn tokens(&self) -> proc_macro2::TokenStream {
        match &self.name {
            Some(name) => quote!(with_bearer_auth_named(#name)),
            None => quote!(with_bearer_auth()),
        }
    }
}

impl Parse for BearerAuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        if input.peek(LitStr) {
            args.name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
        }

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "scheme" {
                crate::set_once(&mut args.name, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name` or `scheme`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ApiCookieAuthArgs {
    name: Option<LitStr>,
    scheme: Option<LitStr>,
    description: Option<LitStr>,
}

impl ApiCookieAuthArgs {
    pub(crate) fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| LitStr::new("sid", proc_macro2::Span::call_site()));
        let scheme = self
            .scheme
            .clone()
            .unwrap_or_else(|| LitStr::new("cookieAuth", proc_macro2::Span::call_site()));
        Ok(api_key_auth_tokens(
            &scheme,
            quote!(::a3s_boot::OpenApiApiKeyLocation::Cookie),
            &name,
            self.description.as_ref(),
        ))
    }
}

impl Parse for ApiCookieAuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        if input.peek(LitStr) {
            args.name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
        }

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "cookie" {
                crate::set_once(&mut args.name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "scheme" {
                crate::set_once(&mut args.scheme, input.parse::<LitStr>()?, ident)?;
            } else if ident == "description" {
                crate::set_once(&mut args.description, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `cookie`, `scheme`, or `description`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

#[derive(Clone, Default)]
pub(crate) struct ApiKeyAuthArgs {
    name: Option<LitStr>,
    scheme: Option<LitStr>,
    location: Option<LitStr>,
    description: Option<LitStr>,
}

impl ApiKeyAuthArgs {
    pub(crate) fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let name = self
            .name
            .clone()
            .unwrap_or_else(|| LitStr::new("x-api-key", proc_macro2::Span::call_site()));
        let scheme = self
            .scheme
            .clone()
            .unwrap_or_else(|| LitStr::new("apiKeyAuth", proc_macro2::Span::call_site()));
        let location = match self.location.as_ref().map(LitStr::value).as_deref() {
            None | Some("header") => quote!(::a3s_boot::OpenApiApiKeyLocation::Header),
            Some("query") => quote!(::a3s_boot::OpenApiApiKeyLocation::Query),
            Some("cookie") => quote!(::a3s_boot::OpenApiApiKeyLocation::Cookie),
            Some(_) => {
                return Err(syn::Error::new_spanned(
                    self.location.as_ref().expect("checked above"),
                    "expected `header`, `query`, or `cookie`",
                ));
            }
        };

        Ok(api_key_auth_tokens(
            &scheme,
            location,
            &name,
            self.description.as_ref(),
        ))
    }
}

impl Parse for ApiKeyAuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut args = Self::default();

        if input.peek(LitStr) {
            args.name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
        }

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "key" {
                crate::set_once(&mut args.name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "scheme" {
                crate::set_once(&mut args.scheme, input.parse::<LitStr>()?, ident)?;
            } else if ident == "location" || ident == "in" {
                crate::set_once(&mut args.location, input.parse::<LitStr>()?, ident)?;
            } else if ident == "description" {
                crate::set_once(&mut args.description, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `key`, `scheme`, `location`, `in`, or `description`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        Ok(args)
    }
}

pub(crate) fn parse_args_or_default<T>(attr: &Attribute) -> Result<T>
where
    T: Parse + Default,
{
    if matches!(attr.meta, syn::Meta::Path(_)) {
        Ok(T::default())
    } else {
        attr.parse_args::<T>()
    }
}

fn parse_string_array(input: ParseStream<'_>) -> Result<Vec<LitStr>> {
    let content;
    bracketed!(content in input);

    let mut items = Vec::new();
    while !content.is_empty() {
        items.push(content.parse::<LitStr>()?);
        crate::parse_optional_comma(&content)?;
    }

    Ok(items)
}

fn api_key_auth_tokens(
    scheme: &LitStr,
    location: proc_macro2::TokenStream,
    name: &LitStr,
    description: Option<&LitStr>,
) -> proc_macro2::TokenStream {
    let mut security_scheme = quote! {
        ::a3s_boot::OpenApiSecurityScheme::api_key(#location, #name)
    };

    if let Some(description) = description {
        security_scheme = quote! {
            (#security_scheme).with_description(#description)
        };
    }

    quote! {
        with_security_scheme(#scheme, #security_scheme)
            .with_api_security(#scheme, ::std::vec::Vec::<::std::string::String>::new())
    }
}
