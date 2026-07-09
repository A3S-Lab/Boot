use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, LitInt, LitStr, Result, Token};

use super::is_attribute_named;
use crate::parse_optional_comma;

pub(in crate::controller) fn take_controller_cache_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerCacheAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut cache = ControllerCacheAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        if is_attribute_named(attr, "cache_key") {
            match attr.parse_args::<CacheKeySpec>() {
                Ok(spec) => cache.specs.push(CacheSpec::Key(spec)),
                Err(error) => errors.push(error),
            }
            continue;
        }

        if is_attribute_named(attr, "cache_ttl") {
            match attr.parse_args::<CacheTtlSpec>() {
                Ok(spec) => cache.specs.push(CacheSpec::Ttl(spec)),
                Err(error) => errors.push(error),
            }
            continue;
        }

        clean_attrs.push(attr.clone());
    }

    (clean_attrs, cache, errors)
}

pub(in crate::controller) fn take_route_cache_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<CacheSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut specs = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        if is_attribute_named(attr, "cache_key") {
            match attr.parse_args::<CacheKeySpec>() {
                Ok(spec) => specs.push(CacheSpec::Key(spec)),
                Err(error) => errors.push(error),
            }
            continue;
        }

        if is_attribute_named(attr, "cache_ttl") {
            match attr.parse_args::<CacheTtlSpec>() {
                Ok(spec) => specs.push(CacheSpec::Ttl(spec)),
                Err(error) => errors.push(error),
            }
            continue;
        }

        clean_attrs.push(attr.clone());
    }

    (clean_attrs, specs, errors)
}

#[derive(Default)]
pub(in crate::controller) struct ControllerCacheAttrs {
    specs: Vec<CacheSpec>,
}

impl ControllerCacheAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs.iter().map(CacheSpec::token).collect()
    }
}

#[derive(Clone)]
pub(in crate::controller) enum CacheSpec {
    Key(CacheKeySpec),
    Ttl(CacheTtlSpec),
}

impl CacheSpec {
    pub(in crate::controller) fn token(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Key(spec) => {
                let key = &spec.key;
                quote!(with_cache_key(#key))
            }
            Self::Ttl(spec) => {
                let duration = spec.duration_tokens();
                quote!(with_cache_ttl(#duration))
            }
        }
    }
}

#[derive(Clone)]
pub(in crate::controller) struct CacheKeySpec {
    key: LitStr,
}

impl Parse for CacheKeySpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let key = input.parse::<LitStr>()?;
        parse_optional_comma(input)?;
        Ok(Self { key })
    }
}

#[derive(Clone)]
pub(in crate::controller) enum CacheTtlSpec {
    Seconds(LitInt),
    Milliseconds(LitInt),
}

impl CacheTtlSpec {
    fn duration_tokens(&self) -> proc_macro2::TokenStream {
        match self {
            Self::Seconds(value) => quote!(::std::time::Duration::from_secs(#value)),
            Self::Milliseconds(value) => quote!(::std::time::Duration::from_millis(#value)),
        }
    }
}

impl Parse for CacheTtlSpec {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        if input.peek(LitInt) {
            let seconds = input.parse::<LitInt>()?;
            parse_optional_comma(input)?;
            return Ok(Self::Seconds(seconds));
        }

        let name = input.parse::<syn::Ident>()?;
        input.parse::<Token![=]>()?;
        let value = input.parse::<LitInt>()?;
        parse_optional_comma(input)?;

        match name.to_string().as_str() {
            "seconds" | "secs" => Ok(Self::Seconds(value)),
            "milliseconds" | "millis" | "ms" => Ok(Self::Milliseconds(value)),
            _ => Err(syn::Error::new_spanned(
                name,
                "expected seconds or milliseconds",
            )),
        }
    }
}
