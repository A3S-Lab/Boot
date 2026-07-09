use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, LitInt, LitStr, Result, Token};

use super::is_attribute_named;
use crate::controller::routing::status_value;
use crate::parse_optional_comma;

pub(in crate::controller) fn take_route_response_attrs(
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

pub(in crate::controller) fn take_route_render_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Option<RenderSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut spec = None;
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_attribute_named(attr, "render") {
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
pub(in crate::controller) enum RouteResponseSpec {
    Header(ResponseHeaderArgs),
    Redirect(RedirectArgs),
}

impl RouteResponseSpec {
    pub(in crate::controller) fn token(&self) -> Result<proc_macro2::TokenStream> {
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
pub(in crate::controller) struct RenderSpec {
    pub(in crate::controller) view: LitStr,
}

#[derive(Clone)]
pub(in crate::controller) struct ResponseHeaderArgs {
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
pub(in crate::controller) struct RedirectArgs {
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
