use proc_macro2::TokenStream;
use syn::parse::{Parse, ParseStream};
use syn::punctuated::Punctuated;
use syn::{parenthesized, Attribute, Path, Result, Token};

pub(crate) fn expand_apply_decorators_attrs(
    attrs: &[Attribute],
) -> (Vec<Attribute>, Vec<syn::Error>) {
    let mut expanded = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        if !is_apply_decorators_attribute(attr) {
            expanded.push(attr.clone());
            continue;
        }

        match attr.parse_args::<ApplyDecoratorsArgs>() {
            Ok(args) => expanded.extend(args.into_attributes()),
            Err(error) => errors.push(error),
        }
    }

    (expanded, errors)
}

fn is_apply_decorators_attribute(attr: &Attribute) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == "apply_decorators")
}

struct ApplyDecoratorsArgs {
    decorators: Vec<AppliedDecorator>,
}

impl ApplyDecoratorsArgs {
    fn into_attributes(self) -> Vec<Attribute> {
        self.decorators
            .into_iter()
            .map(AppliedDecorator::into_attribute)
            .collect()
    }
}

impl Parse for ApplyDecoratorsArgs {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let decorators = Punctuated::<AppliedDecorator, Token![,]>::parse_terminated(input)?
            .into_iter()
            .collect::<Vec<_>>();

        if decorators.is_empty() {
            return Err(input.error("expected at least one decorator"));
        }

        Ok(Self { decorators })
    }
}

struct AppliedDecorator {
    path: Path,
    args: Option<TokenStream>,
}

impl AppliedDecorator {
    fn into_attribute(self) -> Attribute {
        let path = self.path;
        match self.args {
            Some(args) => syn::parse_quote!(#[#path(#args)]),
            None => syn::parse_quote!(#[#path]),
        }
    }
}

impl Parse for AppliedDecorator {
    fn parse(input: ParseStream<'_>) -> Result<Self> {
        let path = input.parse::<Path>()?;
        let args = if input.peek(syn::token::Paren) {
            let content;
            parenthesized!(content in input);
            Some(content.parse::<TokenStream>()?)
        } else {
            None
        };

        Ok(Self { path, args })
    }
}
