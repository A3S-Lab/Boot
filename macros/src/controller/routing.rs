use quote::{format_ident, quote};
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Ident, LitInt, LitStr, Result, Token};

#[derive(Clone)]
pub(super) struct RouteArgs {
    pub(super) path: LitStr,
    pub(super) status: Option<LitInt>,
    pub(super) raw: Option<Ident>,
}

impl RouteArgs {
    pub(super) fn explicit_status<'a>(
        &'a self,
        http_code: Option<&'a LitInt>,
    ) -> Result<Option<&'a LitInt>> {
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

pub(super) fn status_value(status: Option<&LitInt>) -> Result<proc_macro2::TokenStream> {
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

#[derive(Clone)]
pub(super) struct RouteSpec {
    pub(super) kind: RouteKind,
    pub(super) args: RouteArgs,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum RouteKind {
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
    pub(super) fn http_method_ident(self) -> Ident {
        match self {
            Self::All => format_ident!("All"),
            Self::Get | Self::Sse | Self::GetJson => format_ident!("Get"),
            Self::Post | Self::PostJson => format_ident!("Post"),
            Self::Put | Self::PutJson => format_ident!("Put"),
            Self::Patch | Self::PatchJson => format_ident!("Patch"),
            Self::Delete | Self::DeleteJson => format_ident!("Delete"),
            Self::Options => format_ident!("Options"),
            Self::Head => format_ident!("Head"),
        }
    }

    pub(super) fn from_attribute(attr: &Attribute) -> Option<Self> {
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

    pub(super) fn raw_builder_ident(self) -> Ident {
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

    pub(super) fn json_builder_ident(self) -> Option<Ident> {
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

    pub(super) fn is_explicit_json(self) -> bool {
        matches!(
            self,
            Self::GetJson | Self::PostJson | Self::PutJson | Self::PatchJson | Self::DeleteJson
        )
    }

    pub(super) fn flavor(self, raw: bool) -> RouteFlavor {
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
pub(super) enum RouteFlavor {
    Sse,
    Raw,
    JsonRequest,
    JsonBody,
}
