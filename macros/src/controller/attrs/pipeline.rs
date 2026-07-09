use quote::quote;
use syn::{Attribute, Expr};

pub(in crate::controller) fn take_controller_pipeline_attrs(
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

pub(in crate::controller) fn take_route_pipeline_attrs(
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

#[derive(Default)]
pub(in crate::controller) struct ControllerPipelineAttrs {
    specs: Vec<PipelineSpec>,
}

impl ControllerPipelineAttrs {
    pub(in crate::controller) fn tokens(&self) -> Vec<proc_macro2::TokenStream> {
        self.specs.iter().map(PipelineSpec::token).collect()
    }
}

#[derive(Clone)]
pub(in crate::controller) struct PipelineSpec {
    kind: PipelineAttrKind,
    expr: Expr,
}

impl PipelineSpec {
    pub(in crate::controller) fn token(&self) -> proc_macro2::TokenStream {
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
