use quote::format_ident;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Expr, FnArg, Ident, ImplItemFn, LitStr, Pat, PatType, Result, Token, Type};

use crate::file_upload;
use crate::{expect_no_extractor_args, parse_optional_comma};

#[derive(Clone)]
pub(crate) struct RouteMethodInput {
    pub(crate) args: Vec<MethodArg>,
}

impl RouteMethodInput {
    pub(crate) fn from_method(method: &mut ImplItemFn) -> Result<Self> {
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

    pub(crate) fn has_extractors(&self) -> bool {
        self.args.iter().any(|arg| arg.extractor.is_some())
    }

    pub(crate) fn into_legacy_arg(self) -> Result<Option<MethodArg>> {
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
pub(crate) struct MethodArg {
    pub(crate) ident: Ident,
    pub(crate) ty: Box<Type>,
    pub(crate) extractor: Option<Extractor>,
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
pub(crate) enum Extractor {
    Body(BodyExtractor),
    Request,
    Params,
    Param(SingleValueExtractor),
    Query(QueryExtractor),
    Header(SingleValueExtractor),
    Headers,
    Cookie(SingleValueExtractor),
    Cookies,
    HostParam(SingleValueExtractor),
    Ip(Option<Expr>),
    Response,
    Session,
    UploadedFile(file_upload::UploadedFileExtractor),
    UploadedFiles(file_upload::UploadedFileExtractor),
    Custom(Expr),
}

#[derive(Clone)]
pub(crate) enum BodyExtractor {
    Whole,
    Field(SingleValueExtractor),
}

#[derive(Clone)]
pub(crate) struct SingleValueExtractor {
    pub(crate) name: LitStr,
    pub(crate) pipe: Option<Expr>,
    pub(crate) default: Option<Expr>,
}

#[derive(Clone)]
pub(crate) struct QueryExtractor {
    pub(crate) name: Option<LitStr>,
    pub(crate) pipe: Option<Expr>,
    pub(crate) default: Option<Expr>,
}

impl Extractor {
    fn from_attribute(attr: &Attribute) -> Result<Option<Self>> {
        let Some(ident) = attr.path().segments.last().map(|segment| &segment.ident) else {
            return Ok(None);
        };

        let extractor = if ident == "body" {
            Self::Body(parse_body_extractor(attr)?)
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
        } else if ident == "cookie" {
            Self::Cookie(parse_single_value_extractor(attr, "cookie")?)
        } else if ident == "cookies" {
            expect_no_extractor_args(attr, "cookies")?;
            Self::Cookies
        } else if ident == "host_param" {
            Self::HostParam(parse_single_value_extractor(attr, "host_param")?)
        } else if ident == "ip" {
            Self::Ip(parse_optional_pipe_only_extractor(attr, "ip")?)
        } else if ident == "res" {
            expect_no_extractor_args(attr, "res")?;
            Self::Response
        } else if ident == "session" {
            expect_no_extractor_args(attr, "session")?;
            Self::Session
        } else if ident == "uploaded_file" {
            Self::UploadedFile(file_upload::parse_uploaded_file_extractor(
                attr,
                "uploaded_file",
            )?)
        } else if ident == "uploaded_files" {
            Self::UploadedFiles(file_upload::parse_uploaded_file_extractor(
                attr,
                "uploaded_files",
            )?)
        } else if ident == "extract" {
            Self::Custom(parse_extractor_expr(attr)?)
        } else {
            return Ok(None);
        };

        Ok(Some(extractor))
    }
}

fn parse_body_extractor(attr: &Attribute) -> Result<BodyExtractor> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(BodyExtractor::Whole),
        _ => parse_single_value_extractor(attr, "body").map(BodyExtractor::Field),
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
