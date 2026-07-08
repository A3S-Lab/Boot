use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, Expr, Ident, LitBool, LitInt, LitStr, Result, Token};

use super::routing::status_value;
use crate::openapi::{AttrKind as OpenApiAttrKind, RouteSpec as RouteOpenApiSpec};
use crate::{expect_no_extractor_args, parse_optional_comma};

pub(super) fn take_controller_openapi_attrs(
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

pub(super) fn take_route_openapi_attrs(
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

pub(super) fn take_controller_metadata_attrs(
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

pub(super) fn take_route_metadata_attrs(
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

pub(super) fn take_route_http_code_attrs(
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

pub(super) fn take_route_response_attrs(
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

pub(super) fn take_route_render_attrs(
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

pub(super) fn take_controller_pipeline_attrs(
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

pub(super) fn take_route_pipeline_attrs(
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

pub(super) fn take_controller_host_attrs(
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

pub(super) fn take_route_host_attrs(
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

pub(super) fn take_controller_version_attrs(
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

pub(super) fn take_route_version_attrs(
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

pub(super) fn take_controller_serialization_attrs(
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

pub(super) fn take_route_serialization_attrs(
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
pub(super) struct ControllerOpenApiAttrs {
    tags: Vec<LitStr>,
}

impl ControllerOpenApiAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.tags.iter().map(|tag| quote!(with_tag(#tag))).collect()
    }
}

#[derive(Default)]
pub(super) struct ControllerMetadataAttrs {
    specs: Vec<MetadataSpec>,
}

impl ControllerMetadataAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
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
pub(super) struct MetadataSpec {
    pub(super) key: LitStr,
    pub(super) value: Expr,
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
pub(super) struct ControllerPipelineAttrs {
    specs: Vec<PipelineSpec>,
}

impl ControllerPipelineAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs.iter().map(PipelineSpec::token).collect()
    }
}

#[derive(Clone)]
pub(super) struct PipelineSpec {
    kind: PipelineAttrKind,
    expr: Expr,
}

impl PipelineSpec {
    pub(super) fn token(&self) -> proc_macro2::TokenStream {
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
pub(super) struct ControllerHostAttrs {
    pattern: Option<LitStr>,
}

impl ControllerHostAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.pattern
            .iter()
            .map(|pattern| quote!(with_host(#pattern)))
            .collect()
    }
}

pub(super) struct HostSpec {
    pattern: LitStr,
}

impl HostSpec {
    pub(super) fn token(&self) -> proc_macro2::TokenStream {
        let pattern = &self.pattern;
        quote!(with_host(#pattern))
    }
}

#[derive(Default)]
pub(super) struct ControllerVersionAttrs {
    spec: Option<VersionSpec>,
}

impl ControllerVersionAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(VersionSpec::token).collect()
    }
}

#[derive(Clone)]
pub(super) enum VersionSpec {
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

    pub(super) fn token(&self) -> proc_macro2::TokenStream {
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
pub(super) struct ControllerSerializationAttrs {
    spec: Option<SerializationSpec>,
}

impl ControllerSerializationAttrs {
    pub(super) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(SerializationSpec::token).collect()
    }
}

#[derive(Clone, Default)]
pub(super) struct SerializationSpec {
    include_fields: Vec<LitStr>,
    exclude_fields: Vec<LitStr>,
    skip_null_fields: bool,
}

impl SerializationSpec {
    pub(super) fn token(&self) -> proc_macro2::TokenStream {
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
pub(super) enum RouteResponseSpec {
    Header(ResponseHeaderArgs),
    Redirect(RedirectArgs),
}

impl RouteResponseSpec {
    pub(super) fn token(&self) -> Result<proc_macro2::TokenStream> {
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
pub(super) struct RenderSpec {
    pub(super) view: LitStr,
}

#[derive(Clone)]
pub(super) struct ResponseHeaderArgs {
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
pub(super) struct RedirectArgs {
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
