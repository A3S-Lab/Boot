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

#[derive(Clone)]
pub(crate) struct OAuth2AuthArgs {
    scheme: LitStr,
    flow: OAuth2FlowKind,
    authorization_url: Option<LitStr>,
    token_url: Option<LitStr>,
    refresh_url: Option<LitStr>,
    scopes: Vec<LitStr>,
    description: Option<LitStr>,
}

impl OAuth2AuthArgs {
    pub(crate) fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let scheme = &self.scheme;
        let scopes = self
            .scopes
            .iter()
            .map(|scope| quote!(#scope.to_string()))
            .collect::<Vec<_>>();
        let flow_scopes = self
            .scopes
            .iter()
            .map(|scope| quote!((#scope, "")))
            .collect::<Vec<_>>();
        let flow = self.flow.tokens(
            self.authorization_url.as_ref(),
            self.token_url.as_ref(),
            self.refresh_url.as_ref(),
            &flow_scopes,
        )?;
        let mut security_scheme = quote!(::a3s_boot::OpenApiSecurityScheme::oauth2(#flow));

        if let Some(description) = &self.description {
            security_scheme = quote!((#security_scheme).with_description(#description));
        }

        Ok(quote! {
            with_security_scheme(#scheme, #security_scheme)
                .with_api_security(#scheme, vec![#(#scopes),*])
        })
    }
}

impl Parse for OAuth2AuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut scheme = if input.peek(LitStr) {
            let scheme = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
            scheme
        } else {
            None
        };
        let mut flow = None;
        let mut authorization_url = None;
        let mut token_url = None;
        let mut refresh_url = None;
        let mut scopes = Vec::new();
        let mut description = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "scheme" {
                crate::set_once(&mut scheme, input.parse::<LitStr>()?, ident)?;
            } else if ident == "flow" {
                crate::set_once(&mut flow, OAuth2FlowKind::parse(input)?, ident)?;
            } else if ident == "authorization_url" || ident == "authorizationUrl" {
                crate::set_once(&mut authorization_url, input.parse::<LitStr>()?, ident)?;
            } else if ident == "token_url" || ident == "tokenUrl" {
                crate::set_once(&mut token_url, input.parse::<LitStr>()?, ident)?;
            } else if ident == "refresh_url" || ident == "refreshUrl" {
                crate::set_once(&mut refresh_url, input.parse::<LitStr>()?, ident)?;
            } else if ident == "scopes" {
                if !scopes.is_empty() {
                    return Err(syn::Error::new_spanned(ident, "duplicate `scopes` option"));
                }
                scopes = parse_string_array(input)?;
            } else if ident == "description" {
                crate::set_once(&mut description, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `scheme`, `flow`, `authorization_url`, `token_url`, `refresh_url`, `scopes`, or `description`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        let scheme =
            scheme.unwrap_or_else(|| LitStr::new("oauth2", proc_macro2::Span::call_site()));
        let flow = flow.unwrap_or(OAuth2FlowKind::AuthorizationCode);
        flow.validate(authorization_url.as_ref(), token_url.as_ref())?;

        Ok(Self {
            scheme,
            flow,
            authorization_url,
            token_url,
            refresh_url,
            scopes,
            description,
        })
    }
}

#[derive(Clone, Copy)]
enum OAuth2FlowKind {
    Implicit,
    Password,
    ClientCredentials,
    AuthorizationCode,
}

impl OAuth2FlowKind {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let value = input.parse::<LitStr>()?;
        match value.value().as_str() {
            "implicit" => Ok(Self::Implicit),
            "password" => Ok(Self::Password),
            "client_credentials" | "clientCredentials" => Ok(Self::ClientCredentials),
            "authorization_code" | "authorizationCode" => Ok(Self::AuthorizationCode),
            _ => Err(syn::Error::new_spanned(
                value,
                "expected `implicit`, `password`, `client_credentials`, or `authorization_code`",
            )),
        }
    }

    fn validate(
        self,
        authorization_url: Option<&LitStr>,
        token_url: Option<&LitStr>,
    ) -> Result<()> {
        match self {
            Self::Implicit if authorization_url.is_none() => Err(syn::Error::new(
                proc_macro2::Span::call_site(),
                "`implicit` OAuth2 flow requires `authorization_url`",
            )),
            Self::Password | Self::ClientCredentials if token_url.is_none() => {
                Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "this OAuth2 flow requires `token_url`",
                ))
            }
            Self::AuthorizationCode if authorization_url.is_none() || token_url.is_none() => {
                Err(syn::Error::new(
                    proc_macro2::Span::call_site(),
                    "`authorization_code` OAuth2 flow requires `authorization_url` and `token_url`",
                ))
            }
            _ => Ok(()),
        }
    }

    fn tokens(
        self,
        authorization_url: Option<&LitStr>,
        token_url: Option<&LitStr>,
        refresh_url: Option<&LitStr>,
        scopes: &[proc_macro2::TokenStream],
    ) -> Result<proc_macro2::TokenStream> {
        fn with_refresh_url(
            flow: proc_macro2::TokenStream,
            refresh_url: Option<&LitStr>,
        ) -> proc_macro2::TokenStream {
            match refresh_url {
                Some(refresh_url) => quote!((#flow).with_refresh_url(#refresh_url)),
                None => flow,
            }
        }

        Ok(match self {
            Self::Implicit => {
                let authorization_url = authorization_url.expect("validated during parsing");
                let flow = with_refresh_url(
                    quote! {
                        ::a3s_boot::OpenApiOAuthFlow::implicit(#authorization_url, [#(#scopes),*])
                    },
                    refresh_url,
                );
                quote! {
                    ::a3s_boot::OpenApiOAuthFlows::new().with_implicit(#flow)
                }
            }
            Self::Password => {
                let token_url = token_url.expect("validated during parsing");
                let flow = with_refresh_url(
                    quote! {
                        ::a3s_boot::OpenApiOAuthFlow::password(#token_url, [#(#scopes),*])
                    },
                    refresh_url,
                );
                quote! {
                    ::a3s_boot::OpenApiOAuthFlows::new().with_password(#flow)
                }
            }
            Self::ClientCredentials => {
                let token_url = token_url.expect("validated during parsing");
                let flow = with_refresh_url(
                    quote! {
                        ::a3s_boot::OpenApiOAuthFlow::client_credentials(#token_url, [#(#scopes),*])
                    },
                    refresh_url,
                );
                quote! {
                    ::a3s_boot::OpenApiOAuthFlows::new().with_client_credentials(#flow)
                }
            }
            Self::AuthorizationCode => {
                let authorization_url = authorization_url.expect("validated during parsing");
                let token_url = token_url.expect("validated during parsing");
                let flow = with_refresh_url(
                    quote! {
                        ::a3s_boot::OpenApiOAuthFlow::authorization_code(
                            #authorization_url,
                            #token_url,
                            [#(#scopes),*],
                        )
                    },
                    refresh_url,
                );
                quote! {
                    ::a3s_boot::OpenApiOAuthFlows::new().with_authorization_code(#flow)
                }
            }
        })
    }
}

#[derive(Clone)]
pub(crate) struct OpenIdConnectAuthArgs {
    scheme: LitStr,
    url: LitStr,
    scopes: Vec<LitStr>,
    description: Option<LitStr>,
}

impl OpenIdConnectAuthArgs {
    pub(crate) fn tokens(&self) -> proc_macro2::TokenStream {
        let scheme = &self.scheme;
        let url = &self.url;
        let scopes = self
            .scopes
            .iter()
            .map(|scope| quote!(#scope.to_string()))
            .collect::<Vec<_>>();
        let mut security_scheme = quote!(::a3s_boot::OpenApiSecurityScheme::open_id_connect(#url));

        if let Some(description) = &self.description {
            security_scheme = quote!((#security_scheme).with_description(#description));
        }

        quote! {
            with_security_scheme(#scheme, #security_scheme)
                .with_api_security(#scheme, vec![#(#scopes),*])
        }
    }
}

impl Parse for OpenIdConnectAuthArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut scheme = if input.peek(LitStr) {
            let scheme = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
            scheme
        } else {
            None
        };
        let mut url = None;
        let mut scopes = Vec::new();
        let mut description = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "scheme" {
                crate::set_once(&mut scheme, input.parse::<LitStr>()?, ident)?;
            } else if ident == "url"
                || ident == "open_id_connect_url"
                || ident == "openIdConnectUrl"
            {
                crate::set_once(&mut url, input.parse::<LitStr>()?, ident)?;
            } else if ident == "scopes" {
                if !scopes.is_empty() {
                    return Err(syn::Error::new_spanned(ident, "duplicate `scopes` option"));
                }
                scopes = parse_string_array(input)?;
            } else if ident == "description" {
                crate::set_once(&mut description, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `scheme`, `url`, `open_id_connect_url`, `scopes`, or `description`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        let scheme =
            scheme.unwrap_or_else(|| LitStr::new("openId", proc_macro2::Span::call_site()));
        let Some(url) = url else {
            return Err(input.error("missing required `url` option"));
        };

        Ok(Self {
            scheme,
            url,
            scopes,
            description,
        })
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
