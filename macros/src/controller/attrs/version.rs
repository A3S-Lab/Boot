use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{Attribute, LitStr, Result, Token};

use crate::expect_no_extractor_args;

pub(in crate::controller) fn take_controller_version_attrs(
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

pub(in crate::controller) fn take_route_version_attrs(
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

#[derive(Default)]
pub(in crate::controller) struct ControllerVersionAttrs {
    spec: Option<VersionSpec>,
}

impl ControllerVersionAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.spec.iter().map(VersionSpec::token).collect()
    }
}

#[derive(Clone)]
pub(in crate::controller) enum VersionSpec {
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

    pub(in crate::controller) fn token(&self) -> proc_macro2::TokenStream {
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
