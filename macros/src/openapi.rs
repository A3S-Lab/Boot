use crate::openapi_security::{
    parse_args_or_default, ApiCookieAuthArgs, ApiKeyAuthArgs, ApiSecurityArgs, BearerAuthArgs,
    OAuth2AuthArgs, OpenIdConnectAuthArgs,
};
use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{
    Attribute, Expr, GenericArgument, Ident, LitBool, LitInt, LitStr, PathArguments, Result, Token,
    Type,
};

#[derive(Clone, Copy)]
pub(crate) enum AttrKind {
    Tag,
    Operation,
    Response,
    RequestBody,
    ApiParam,
    ApiQuery,
    ApiHeader,
    ApiResponseHeader,
    ApiSecurity,
    ApiCookieAuth,
    ApiKeyAuth,
    BearerAuth,
    OAuth2Auth,
    OpenIdConnectAuth,
    ApiExtraModel,
    ApiExtension,
    HideFromOpenApi,
}

impl AttrKind {
    pub(crate) fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "tag" => Some(Self::Tag),
            "operation" => Some(Self::Operation),
            "response" => Some(Self::Response),
            "request_body" => Some(Self::RequestBody),
            "api_param" => Some(Self::ApiParam),
            "api_query" => Some(Self::ApiQuery),
            "api_header" => Some(Self::ApiHeader),
            "api_response_header" => Some(Self::ApiResponseHeader),
            "api_security" => Some(Self::ApiSecurity),
            "api_cookie_auth" => Some(Self::ApiCookieAuth),
            "api_key_auth" => Some(Self::ApiKeyAuth),
            "bearer_auth" => Some(Self::BearerAuth),
            "oauth2_auth" => Some(Self::OAuth2Auth),
            "open_id_connect_auth" => Some(Self::OpenIdConnectAuth),
            "api_extra_model" => Some(Self::ApiExtraModel),
            "api_extension" => Some(Self::ApiExtension),
            "hide_from_openapi" => Some(Self::HideFromOpenApi),
            _ => None,
        }
    }

    pub(crate) fn parse_route_spec(self, attr: &Attribute) -> Result<RouteSpec> {
        match self {
            Self::Tag => attr.parse_args::<LitStr>().map(RouteSpec::Tag),
            Self::Operation => attr.parse_args::<OperationArgs>().map(RouteSpec::Operation),
            Self::Response => attr.parse_args::<ResponseArgs>().map(RouteSpec::Response),
            Self::RequestBody => attr
                .parse_args::<RequestBodyArgs>()
                .map(RouteSpec::RequestBody),
            Self::ApiParam => attr.parse_args::<OpenApiParameterArgs>().map(|args| {
                RouteSpec::Parameter(OpenApiParameterSpec {
                    kind: OpenApiParameterSpecKind::Path,
                    args,
                })
            }),
            Self::ApiQuery => attr.parse_args::<OpenApiParameterArgs>().map(|args| {
                RouteSpec::Parameter(OpenApiParameterSpec {
                    kind: OpenApiParameterSpecKind::Query,
                    args,
                })
            }),
            Self::ApiHeader => attr.parse_args::<OpenApiParameterArgs>().map(|args| {
                RouteSpec::Parameter(OpenApiParameterSpec {
                    kind: OpenApiParameterSpecKind::Header,
                    args,
                })
            }),
            Self::ApiResponseHeader => attr
                .parse_args::<OpenApiResponseHeaderArgs>()
                .map(RouteSpec::ResponseHeader),
            Self::ApiSecurity => attr
                .parse_args::<ApiSecurityArgs>()
                .map(RouteSpec::ApiSecurity),
            Self::ApiCookieAuth => {
                parse_args_or_default::<ApiCookieAuthArgs>(attr).map(RouteSpec::ApiCookieAuth)
            }
            Self::ApiKeyAuth => {
                parse_args_or_default::<ApiKeyAuthArgs>(attr).map(RouteSpec::ApiKeyAuth)
            }
            Self::BearerAuth => {
                parse_args_or_default::<BearerAuthArgs>(attr).map(RouteSpec::BearerAuth)
            }
            Self::OAuth2Auth => attr
                .parse_args::<OAuth2AuthArgs>()
                .map(RouteSpec::OAuth2Auth),
            Self::OpenIdConnectAuth => attr
                .parse_args::<OpenIdConnectAuthArgs>()
                .map(RouteSpec::OpenIdConnectAuth),
            Self::ApiExtraModel => attr
                .parse_args::<ApiExtraModelArgs>()
                .map(RouteSpec::ApiExtraModel),
            Self::ApiExtension => attr
                .parse_args::<ApiExtensionArgs>()
                .map(RouteSpec::ApiExtension),
            Self::HideFromOpenApi => {
                crate::expect_no_extractor_args(attr, "hide_from_openapi")?;
                Ok(RouteSpec::HideFromOpenApi)
            }
        }
    }
}

#[derive(Clone)]
pub(crate) enum RouteSpec {
    Tag(LitStr),
    Operation(OperationArgs),
    Response(ResponseArgs),
    RequestBody(RequestBodyArgs),
    Parameter(OpenApiParameterSpec),
    ResponseHeader(OpenApiResponseHeaderArgs),
    ApiSecurity(ApiSecurityArgs),
    ApiCookieAuth(ApiCookieAuthArgs),
    ApiKeyAuth(ApiKeyAuthArgs),
    BearerAuth(BearerAuthArgs),
    OAuth2Auth(OAuth2AuthArgs),
    OpenIdConnectAuth(OpenIdConnectAuthArgs),
    ApiExtraModel(ApiExtraModelArgs),
    ApiExtension(ApiExtensionArgs),
    HideFromOpenApi,
}

impl RouteSpec {
    pub(crate) fn tokens(&self) -> Result<Vec<proc_macro2::TokenStream>> {
        match self {
            Self::Tag(tag) => Ok(vec![quote!(with_tag(#tag))]),
            Self::Operation(args) => Ok(args.tokens()),
            Self::Response(args) => args.tokens().map(|token| vec![token]),
            Self::RequestBody(args) => Ok(vec![args.tokens()]),
            Self::Parameter(spec) => spec.tokens().map(|token| vec![token]),
            Self::ResponseHeader(args) => args.tokens().map(|token| vec![token]),
            Self::ApiSecurity(args) => Ok(vec![args.tokens()]),
            Self::ApiCookieAuth(args) => args.tokens().map(|token| vec![token]),
            Self::ApiKeyAuth(args) => args.tokens().map(|token| vec![token]),
            Self::BearerAuth(args) => Ok(vec![args.tokens()]),
            Self::OAuth2Auth(args) => args.tokens().map(|token| vec![token]),
            Self::OpenIdConnectAuth(args) => Ok(vec![args.tokens()]),
            Self::ApiExtraModel(args) => Ok(vec![args.tokens()]),
            Self::ApiExtension(args) => Ok(vec![args.tokens()]),
            Self::HideFromOpenApi => Ok(vec![quote!(hide_from_openapi())]),
        }
    }
}

pub(crate) fn schema_tokens(ty: &Type) -> proc_macro2::TokenStream {
    if let Some(inner) = crate::option_inner_type(ty) {
        return schema_tokens(&inner);
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
                let inner_schema = schema_tokens(inner);
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

#[derive(Clone, Default)]
pub(crate) struct OperationArgs {
    summary: Option<LitStr>,
    description: Option<LitStr>,
    operation_id: Option<LitStr>,
    server_url: Option<LitStr>,
    server_description: Option<LitStr>,
    external_docs_description: Option<LitStr>,
    external_docs_url: Option<LitStr>,
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
        if let Some(server_url) = &self.server_url {
            if let Some(server_description) = &self.server_description {
                tokens.push(quote!(with_openapi_server_description(
                    #server_url,
                    #server_description
                )));
            } else {
                tokens.push(quote!(with_openapi_server(#server_url)));
            }
        }
        if let (Some(description), Some(url)) =
            (&self.external_docs_description, &self.external_docs_url)
        {
            tokens.push(quote!(with_openapi_external_docs(#description, #url)));
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
                    crate::set_once(&mut args.summary, value, name)?;
                } else if name == "description" {
                    crate::set_once(&mut args.description, value, name)?;
                } else if name == "operation_id" || name == "id" {
                    crate::set_once(&mut args.operation_id, value, name)?;
                } else if name == "server_url" || name == "serverUrl" {
                    crate::set_once(&mut args.server_url, value, name)?;
                } else if name == "server_description" || name == "serverDescription" {
                    crate::set_once(&mut args.server_description, value, name)?;
                } else if name == "external_docs_description" || name == "externalDocsDescription" {
                    crate::set_once(&mut args.external_docs_description, value, name)?;
                } else if name == "external_docs_url" || name == "externalDocsUrl" {
                    crate::set_once(&mut args.external_docs_url, value, name)?;
                } else {
                    return Err(syn::Error::new_spanned(
                        name,
                        "expected `summary`, `description`, `operation_id`, `server_url`, `server_description`, `external_docs_description`, `external_docs_url`, or `deprecated`",
                    ));
                }
            }
            crate::parse_optional_comma(input)?;
        }

        if args.server_description.is_some() && args.server_url.is_none() {
            return Err(input.error("`server_description` requires `server_url`"));
        }
        if args.external_docs_description.is_some() != args.external_docs_url.is_some() {
            return Err(input.error(
                "`external_docs_description` and `external_docs_url` must be provided together",
            ));
        }

        Ok(args)
    }
}

#[derive(Clone)]
pub(crate) struct ResponseArgs {
    status: LitInt,
    description: Option<LitStr>,
    schema: Option<Type>,
    content_type: Option<LitStr>,
    example: Option<Expr>,
    example_name: Option<LitStr>,
}

impl ResponseArgs {
    fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let status = self.status.base10_parse::<u16>()?;
        let description = match &self.description {
            Some(description) => quote!(#description),
            None => quote!("Success"),
        };

        Ok(
            if self.schema.is_some()
                || self.content_type.is_some()
                || self.example.is_some()
                || self.example_name.is_some()
            {
                let schema = self
                    .schema
                    .as_ref()
                    .map(schema_tokens)
                    .unwrap_or_else(|| quote!(::a3s_boot::OpenApiSchema::object()));
                let content_type = self
                    .content_type
                    .as_ref()
                    .map(|content_type| quote!(#content_type))
                    .unwrap_or_else(|| quote!("application/json"));

                match (&self.example_name, &self.example) {
                    (Some(example_name), Some(example)) => {
                        quote!(try_with_response_content_type_named_example(#status, #description, #content_type, #schema, #example_name, #example)?)
                    }
                    (None, Some(example)) => {
                        quote!(try_with_response_content_type_example(#status, #description, #content_type, #schema, #example)?)
                    }
                    (None, None) => {
                        quote!(with_response_content_type(#status, #description, #content_type, #schema))
                    }
                    (Some(_), None) => unreachable!("validated during parsing"),
                }
            } else {
                quote! {
                with_response(
                    #status,
                    ::a3s_boot::OpenApiResponse::description(#description)
                )
                }
            },
        )
    }
}

impl Parse for ResponseArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut status = None;
        let mut description = None;
        let mut schema = None;
        let mut content_type = None;
        let mut example = None;
        let mut example_name = None;

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            if name == "status" {
                crate::set_once(&mut status, input.parse::<LitInt>()?, name)?;
            } else if name == "description" {
                crate::set_once(&mut description, input.parse::<LitStr>()?, name)?;
            } else if name == "schema" || name == "ty" || name == "body" {
                crate::set_once(&mut schema, input.parse::<Type>()?, name)?;
            } else if name == "content_type" || name == "contentType" || name == "media_type" {
                crate::set_once(&mut content_type, input.parse::<LitStr>()?, name)?;
            } else if name == "example" {
                crate::set_once(&mut example, input.parse::<Expr>()?, name)?;
            } else if name == "example_name" || name == "exampleName" {
                crate::set_once(&mut example_name, input.parse::<LitStr>()?, name)?;
            } else {
                return Err(syn::Error::new_spanned(
                    name,
                    "expected `status`, `description`, `schema`, `content_type`, `example`, or `example_name`",
                ));
            }
            crate::parse_optional_comma(input)?;
        }

        let Some(status) = status else {
            return Err(input.error("missing required `status` option"));
        };
        if example_name.is_some() && example.is_none() {
            return Err(input.error("`example_name` requires `example`"));
        }

        Ok(Self {
            status,
            description,
            schema,
            content_type,
            example,
            example_name,
        })
    }
}

#[derive(Clone, Default)]
pub(crate) struct RequestBodyArgs {
    schema: Option<Type>,
    content_type: Option<LitStr>,
    description: Option<LitStr>,
    required: Option<LitBool>,
    example: Option<Expr>,
    example_name: Option<LitStr>,
}

impl RequestBodyArgs {
    fn tokens(&self) -> proc_macro2::TokenStream {
        let schema = self
            .schema
            .as_ref()
            .map(schema_tokens)
            .unwrap_or_else(|| quote!(::a3s_boot::OpenApiSchema::object()));
        let content_type = self
            .content_type
            .as_ref()
            .map(|content_type| quote!(#content_type))
            .unwrap_or_else(|| quote!("application/json"));
        let mut request_body = match (&self.content_type, &self.example_name, &self.example) {
            (_, Some(example_name), Some(example)) => {
                quote!(::a3s_boot::OpenApiRequestBody::content(#content_type, #schema).try_with_content_named_example(#content_type, #example_name, #example)?)
            }
            (_, None, Some(example)) => {
                quote!(::a3s_boot::OpenApiRequestBody::try_content_example(#content_type, #schema, #example)?)
            }
            (Some(_), None, None) => {
                quote!(::a3s_boot::OpenApiRequestBody::content(#content_type, #schema))
            }
            (None, None, None) => quote!(::a3s_boot::OpenApiRequestBody::json(#schema)),
            (_, Some(_), None) => unreachable!("validated during parsing"),
        };

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
                crate::set_once(&mut args.schema, input.parse::<Type>()?, name)?;
            } else if name == "content_type" || name == "contentType" || name == "media_type" {
                crate::set_once(&mut args.content_type, input.parse::<LitStr>()?, name)?;
            } else if name == "description" {
                crate::set_once(&mut args.description, input.parse::<LitStr>()?, name)?;
            } else if name == "required" {
                crate::set_once(&mut args.required, input.parse::<LitBool>()?, name)?;
            } else if name == "example" {
                crate::set_once(&mut args.example, input.parse::<Expr>()?, name)?;
            } else if name == "example_name" || name == "exampleName" {
                crate::set_once(&mut args.example_name, input.parse::<LitStr>()?, name)?;
            } else {
                return Err(syn::Error::new_spanned(
                    name,
                    "expected `schema`, `content_type`, `description`, `required`, `example`, or `example_name`",
                ));
            }
            crate::parse_optional_comma(input)?;
        }
        if args.example_name.is_some() && args.example.is_none() {
            return Err(input.error("`example_name` requires `example`"));
        }

        Ok(args)
    }
}

#[derive(Clone)]
pub(crate) struct OpenApiResponseHeaderArgs {
    status: LitInt,
    name: LitStr,
    schema: Option<Type>,
    description: Option<LitStr>,
}

impl OpenApiResponseHeaderArgs {
    fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let status = self.status.base10_parse::<u16>()?;
        let name = &self.name;
        let schema = self
            .schema
            .as_ref()
            .map(schema_tokens)
            .unwrap_or_else(|| quote!(::a3s_boot::OpenApiSchema::string()));
        let mut header = quote!(::a3s_boot::OpenApiHeader::new(#schema));

        if let Some(description) = &self.description {
            header = quote!((#header).with_description(#description));
        }

        Ok(quote!(with_openapi_response_header(#status, #name, #header)))
    }
}

impl Parse for OpenApiResponseHeaderArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut status = None;
        let mut name = None;
        let mut schema = None;
        let mut description = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "status" {
                crate::set_once(&mut status, input.parse::<LitInt>()?, ident)?;
            } else if ident == "name" || ident == "header" {
                crate::set_once(&mut name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "schema" || ident == "ty" || ident == "type" {
                crate::set_once(&mut schema, input.parse::<Type>()?, ident)?;
            } else if ident == "description" {
                crate::set_once(&mut description, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `status`, `name`, `schema`, or `description`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        let Some(status) = status else {
            return Err(input.error("missing required `status` option"));
        };
        let Some(name) = name else {
            return Err(input.error("missing required `name` option"));
        };

        Ok(Self {
            status,
            name,
            schema,
            description,
        })
    }
}

#[derive(Clone)]
pub(crate) struct ApiExtraModelArgs {
    name: LitStr,
    schema: Expr,
}

impl ApiExtraModelArgs {
    pub(crate) fn tokens(&self) -> proc_macro2::TokenStream {
        let name = &self.name;
        let schema = &self.schema;
        quote!(with_schema_component(#name, #schema))
    }
}

impl Parse for ApiExtraModelArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut name = if input.peek(LitStr) {
            let name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
            name
        } else {
            None
        };
        let mut schema = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "model" {
                crate::set_once(&mut name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "schema" {
                crate::set_once(&mut schema, input.parse::<Expr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `model`, or `schema`",
                ));
            }

            crate::parse_optional_comma(input)?;
        }

        let Some(name) = name else {
            return Err(input.error("missing required `name` option"));
        };
        let Some(schema) = schema else {
            return Err(input.error("missing required `schema` option"));
        };

        Ok(Self { name, schema })
    }
}

#[derive(Clone)]
pub(crate) struct ApiExtensionArgs {
    name: LitStr,
    value: Expr,
}

impl ApiExtensionArgs {
    pub(crate) fn tokens(&self) -> proc_macro2::TokenStream {
        let name = &self.name;
        let value = &self.value;
        quote!(try_with_openapi_extension(#name, #value)?)
    }
}

impl Parse for ApiExtensionArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut name = if input.peek(LitStr) {
            let name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
            name
        } else {
            None
        };
        let mut value = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;

            if ident == "name" || ident == "extension" {
                crate::set_once(&mut name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "value" {
                crate::set_once(&mut value, input.parse::<Expr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(ident, "expected `name` or `value`"));
            }

            crate::parse_optional_comma(input)?;
        }

        let Some(name) = name else {
            return Err(input.error("missing required `name` option"));
        };
        let Some(value) = value else {
            return Err(input.error("missing required `value` option"));
        };

        Ok(Self { name, value })
    }
}

#[derive(Clone, Copy)]
enum OpenApiParameterSpecKind {
    Path,
    Query,
    Header,
}

#[derive(Clone)]
pub(crate) struct OpenApiParameterSpec {
    kind: OpenApiParameterSpecKind,
    args: OpenApiParameterArgs,
}

impl OpenApiParameterSpec {
    fn tokens(&self) -> Result<proc_macro2::TokenStream> {
        let name = &self.args.name;
        let schema = self
            .args
            .schema
            .as_ref()
            .map(schema_tokens)
            .unwrap_or_else(|| quote!(::a3s_boot::OpenApiSchema::string()));
        let required = match &self.args.required {
            Some(required) => required.value,
            None => true,
        };

        if matches!(self.kind, OpenApiParameterSpecKind::Path) && !required {
            return Err(syn::Error::new_spanned(
                self.args.required.as_ref().expect("checked above"),
                "OpenAPI path parameters are always required",
            ));
        }

        let mut parameter = match self.kind {
            OpenApiParameterSpecKind::Path => {
                quote!(::a3s_boot::OpenApiParameter::path(#name, #schema))
            }
            OpenApiParameterSpecKind::Query => {
                quote!(::a3s_boot::OpenApiParameter::query(#name, #required, #schema))
            }
            OpenApiParameterSpecKind::Header => {
                quote!(::a3s_boot::OpenApiParameter::header(#name, #required, #schema))
            }
        };

        if let Some(description) = &self.args.description {
            parameter = quote!((#parameter).with_description(#description));
        }
        if self.args.deprecated.as_ref().is_some_and(LitBool::value) {
            parameter = quote!((#parameter).with_deprecated());
        }
        if self
            .args
            .allow_reserved
            .as_ref()
            .is_some_and(LitBool::value)
        {
            parameter = quote!((#parameter).with_allow_reserved());
        }
        if let Some(style) = &self.args.style {
            parameter = quote!((#parameter).with_style(#style));
        }
        if let Some(explode) = &self.args.explode {
            let explode = explode.value;
            parameter = quote!((#parameter).with_explode(#explode));
        }
        parameter = match (&self.args.example_name, &self.args.example) {
            (Some(example_name), Some(example)) => {
                quote!((#parameter).try_with_named_example(#example_name, #example)?)
            }
            (None, Some(example)) => quote!((#parameter).try_with_example(#example)?),
            (None, None) => parameter,
            (Some(_), None) => unreachable!("validated during parsing"),
        };

        Ok(quote!(with_parameter(#parameter)))
    }
}

#[derive(Clone)]
struct OpenApiParameterArgs {
    name: LitStr,
    schema: Option<Type>,
    description: Option<LitStr>,
    required: Option<LitBool>,
    deprecated: Option<LitBool>,
    allow_reserved: Option<LitBool>,
    style: Option<LitStr>,
    explode: Option<LitBool>,
    example: Option<Expr>,
    example_name: Option<LitStr>,
}

impl Parse for OpenApiParameterArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut name = if input.peek(LitStr) {
            let name = Some(input.parse::<LitStr>()?);
            crate::parse_optional_comma(input)?;
            name
        } else {
            None
        };
        let mut schema = None;
        let mut description = None;
        let mut required = None;
        let mut deprecated = None;
        let mut allow_reserved = None;
        let mut style = None;
        let mut explode = None;
        let mut example = None;
        let mut example_name = None;

        while !input.is_empty() {
            let ident = input.parse::<Ident>()?;
            input.parse::<Token![=]>()?;
            if ident == "name" {
                crate::set_once(&mut name, input.parse::<LitStr>()?, ident)?;
            } else if ident == "schema" || ident == "ty" || ident == "type" {
                crate::set_once(&mut schema, input.parse::<Type>()?, ident)?;
            } else if ident == "description" {
                crate::set_once(&mut description, input.parse::<LitStr>()?, ident)?;
            } else if ident == "required" {
                crate::set_once(&mut required, input.parse::<LitBool>()?, ident)?;
            } else if ident == "deprecated" {
                crate::set_once(&mut deprecated, input.parse::<LitBool>()?, ident)?;
            } else if ident == "allow_reserved" || ident == "allowReserved" {
                crate::set_once(&mut allow_reserved, input.parse::<LitBool>()?, ident)?;
            } else if ident == "style" {
                crate::set_once(&mut style, input.parse::<LitStr>()?, ident)?;
            } else if ident == "explode" {
                crate::set_once(&mut explode, input.parse::<LitBool>()?, ident)?;
            } else if ident == "example" {
                crate::set_once(&mut example, input.parse::<Expr>()?, ident)?;
            } else if ident == "example_name" || ident == "exampleName" {
                crate::set_once(&mut example_name, input.parse::<LitStr>()?, ident)?;
            } else {
                return Err(syn::Error::new_spanned(
                    ident,
                    "expected `name`, `schema`, `description`, `required`, `deprecated`, `allow_reserved`, `style`, `explode`, `example`, or `example_name`",
                ));
            }
            crate::parse_optional_comma(input)?;
        }

        let Some(name) = name else {
            return Err(input.error("missing required `name` option"));
        };
        if example_name.is_some() && example.is_none() {
            return Err(input.error("`example_name` requires `example`"));
        }

        Ok(Self {
            name,
            schema,
            description,
            required,
            deprecated,
            allow_reserved,
            style,
            explode,
            example,
            example_name,
        })
    }
}
