use quote::quote;
use syn::parse::{Parse, ParseStream};
use syn::{Attribute, Fields, Ident, LitBool, LitStr, Meta, Result, Token};

pub(crate) fn expand_validation_schema(
    item_struct: syn::ItemStruct,
) -> Result<proc_macro2::TokenStream> {
    let struct_ident = &item_struct.ident;
    let fields = match &item_struct.fields {
        Fields::Named(fields) => &fields.named,
        _ => {
            return Err(syn::Error::new_spanned(
                &item_struct,
                "ValidationSchema can only be derived for structs with named fields",
            ));
        }
    };

    let mut field_names = Vec::new();
    for field in fields {
        let ident = field.ident.as_ref().expect("named fields checked above");
        field_names.push(validation_schema_field_name(ident, &field.attrs)?);
    }

    let (impl_generics, ty_generics, where_clause) = item_struct.generics.split_for_impl();
    Ok(quote! {
        impl #impl_generics ::a3s_boot::ValidationSchema for #struct_ident #ty_generics #where_clause {
            fn allowed_fields() -> &'static [&'static str] {
                &[#(#field_names),*]
            }
        }
    })
}

fn validation_schema_field_name(ident: &Ident, attrs: &[Attribute]) -> Result<LitStr> {
    let mut name = ident.to_string();

    for attr in attrs {
        if !attr.path().is_ident("serde") {
            continue;
        }

        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("rename") {
                meta.input.parse::<Token![=]>()?;
                name = meta.input.parse::<LitStr>()?.value();
            } else if meta.input.peek(Token![=]) {
                meta.input.parse::<Token![=]>()?;
                let _ = meta.input.parse::<syn::Expr>()?;
            }
            Ok(())
        })?;
    }

    Ok(LitStr::new(&name, ident.span()))
}

#[derive(Clone, Copy)]
pub(crate) enum AttrKind {
    Validate,
    SkipValidation,
}

impl AttrKind {
    pub(crate) fn from_attribute(attr: &Attribute) -> Option<Self> {
        let ident = attr.path().segments.last()?.ident.to_string();
        match ident.as_str() {
            "validate" => Some(Self::Validate),
            "skip_validation" => Some(Self::SkipValidation),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Validate => "validate",
            Self::SkipValidation => "skip_validation",
        }
    }
}

#[derive(Clone, Copy, Default)]
pub(crate) struct AttrOptions {
    transform: bool,
    whitelist: bool,
    forbid_non_whitelisted: bool,
}

impl AttrOptions {
    pub(crate) fn is_empty(self) -> bool {
        !self.transform && !self.whitelist && !self.forbid_non_whitelisted
    }

    pub(crate) fn merge(self, other: Self) -> Self {
        Self {
            transform: self.transform || other.transform,
            whitelist: self.whitelist || other.whitelist,
            forbid_non_whitelisted: self.forbid_non_whitelisted || other.forbid_non_whitelisted,
        }
    }

    pub(crate) fn token(self) -> proc_macro2::TokenStream {
        let transform = self
            .transform
            .then(|| quote!(.transform(true)))
            .unwrap_or_default();
        let whitelist = self
            .whitelist
            .then(|| quote!(.whitelist(true)))
            .unwrap_or_default();
        let forbid_non_whitelisted = self
            .forbid_non_whitelisted
            .then(|| quote!(.forbid_non_whitelisted(true)))
            .unwrap_or_default();
        quote! {
            ::a3s_boot::ValidationOptions::new() #transform #whitelist #forbid_non_whitelisted
        }
    }
}

impl Parse for AttrOptions {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let mut options = Self::default();

        while !input.is_empty() {
            let name = input.parse::<Ident>()?;
            let enabled = if input.peek(Token![=]) {
                input.parse::<Token![=]>()?;
                input.parse::<LitBool>()?.value
            } else {
                true
            };

            match name.to_string().as_str() {
                "transform" => options.transform = enabled,
                "whitelist" => options.whitelist = enabled,
                "forbid_non_whitelisted" | "forbidNonWhitelisted" => {
                    options.forbid_non_whitelisted = enabled;
                }
                _ => {
                    return Err(syn::Error::new_spanned(name, "unsupported validate option"));
                }
            }

            crate::parse_optional_comma(input)?;
        }

        Ok(options)
    }
}

pub(crate) fn parse_options(attr: &Attribute) -> Result<AttrOptions> {
    match &attr.meta {
        Meta::Path(_) => Ok(AttrOptions::default()),
        Meta::List(_) => attr.parse_args::<AttrOptions>(),
        Meta::NameValue(_) => Err(syn::Error::new_spanned(
            attr,
            "#[validate] expects flags such as #[validate(whitelist)]",
        )),
    }
}

pub(crate) fn take_controller_validation_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, ControllerAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut validation = ControllerAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = AttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            AttrKind::Validate => {
                let options = match parse_options(attr) {
                    Ok(options) => options,
                    Err(error) => {
                        errors.push(error);
                        continue;
                    }
                };
                if validation.enabled {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "duplicate #[validate] attribute",
                    ));
                } else {
                    validation.enabled = true;
                    validation.options = options;
                }
            }
            AttrKind::SkipValidation => errors.push(syn::Error::new_spanned(
                attr,
                "#[skip_validation] is only supported on route methods",
            )),
        }
    }

    (clean_attrs, validation, errors)
}

pub(crate) fn take_route_validation_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, RouteAttrs, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut validation = RouteAttrs::default();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = AttrKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match kind {
            AttrKind::Validate => {
                let options = match parse_options(attr) {
                    Ok(options) => options,
                    Err(error) => {
                        errors.push(error);
                        continue;
                    }
                };
                if validation.validate {
                    errors.push(syn::Error::new_spanned(
                        attr,
                        "duplicate #[validate] attribute",
                    ));
                } else {
                    validation.validate = true;
                    validation.options = Some(options);
                }
            }
            AttrKind::SkipValidation => {
                if let Err(error) = crate::expect_no_extractor_args(attr, kind.name()) {
                    errors.push(error);
                    continue;
                }
                validation.skip = true;
            }
        }
    }

    if validation.validate && validation.skip {
        errors.push(syn::Error::new(
            proc_macro2::Span::call_site(),
            "route methods cannot use both #[validate] and #[skip_validation]",
        ));
    }

    (clean_attrs, validation, errors)
}

#[derive(Clone, Copy, Default)]
pub(crate) struct ControllerAttrs {
    pub(crate) enabled: bool,
    pub(crate) options: AttrOptions,
}

#[derive(Clone, Copy, Default)]
pub(crate) struct RouteAttrs {
    pub(crate) validate: bool,
    pub(crate) skip: bool,
    pub(crate) options: Option<AttrOptions>,
}

impl RouteAttrs {
    pub(crate) fn is_present(self) -> bool {
        self.validate || self.skip
    }

    pub(crate) fn enabled_options(self, controller: ControllerAttrs) -> Option<AttrOptions> {
        if self.skip || (!controller.enabled && !self.validate) {
            return None;
        }

        let options = self
            .options
            .map(|options| controller.options.merge(options))
            .unwrap_or(controller.options);
        Some(options)
    }
}
