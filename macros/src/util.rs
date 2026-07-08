use syn::parse::ParseStream;
use syn::{Attribute, GenericArgument, Ident, PathArguments, Result, Token, Type};

pub(crate) fn set_once<T>(slot: &mut Option<T>, value: T, name: Ident) -> Result<()> {
    if slot.is_some() {
        let message = format!("duplicate `{name}` option");
        return Err(syn::Error::new_spanned(&name, message));
    }
    *slot = Some(value);
    Ok(())
}

pub(crate) fn parse_optional_comma(input: ParseStream<'_>) -> Result<()> {
    if input.is_empty() {
        return Ok(());
    }
    input.parse::<Token![,]>()?;
    Ok(())
}

pub(crate) fn push_error(slot: &mut Option<syn::Error>, error: syn::Error) {
    if let Some(existing) = slot {
        existing.combine(error);
    } else {
        *slot = Some(error);
    }
}

pub(crate) fn is_type_ident(ty: &Type, ident: &str) -> bool {
    let Type::Path(type_path) = ty else {
        return false;
    };
    type_path
        .path
        .segments
        .last()
        .is_some_and(|segment| segment.ident == ident)
}

pub(crate) fn expect_no_extractor_args(attr: &Attribute, name: &str) -> Result<()> {
    match &attr.meta {
        syn::Meta::Path(_) => Ok(()),
        _ => Err(syn::Error::new_spanned(
            attr,
            format!("#[{name}] does not accept arguments"),
        )),
    }
}

pub(crate) fn option_inner_type(ty: &Type) -> Option<Type> {
    let Type::Path(type_path) = ty else {
        return None;
    };
    if type_path.qself.is_some() {
        return None;
    }

    let segment = type_path.path.segments.last()?;
    if segment.ident != "Option" {
        return None;
    }

    let PathArguments::AngleBracketed(arguments) = &segment.arguments else {
        return None;
    };
    if arguments.args.len() != 1 {
        return None;
    }

    match arguments.args.first()? {
        GenericArgument::Type(inner) => Some(inner.clone()),
        _ => None,
    }
}
