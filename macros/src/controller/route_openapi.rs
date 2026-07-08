use super::input::{Extractor, RouteMethodInput};
use super::routing::RouteFlavor;
use crate::file_upload;
use crate::openapi::schema_tokens as openapi_schema_tokens;
use crate::openapi::RouteSpec as RouteOpenApiSpec;
use crate::option_inner_type;
use quote::quote;
use syn::{Expr, LitStr, Result, Type};

pub(super) fn openapi_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    json_success_status: Option<proc_macro2::TokenStream>,
    specs: &[RouteOpenApiSpec],
) -> Result<proc_macro2::TokenStream> {
    if let Some(status) = json_success_status {
        route_definition = quote! {
            (#route_definition).with_response(
                #status,
                ::a3s_boot::OpenApiResponse::description("Success")
            )
        };
    }

    for token in extractor_openapi_tokens(input, flavor) {
        route_definition = quote! {
            (#route_definition).#token
        };
    }

    for spec in specs {
        for token in spec.tokens()? {
            route_definition = quote! {
                (#route_definition).#token
            };
        }
    }

    Ok(route_definition)
}

fn extractor_openapi_tokens(
    input: &RouteMethodInput,
    flavor: RouteFlavor,
) -> Vec<proc_macro2::TokenStream> {
    let mut tokens = Vec::new();

    if matches!(flavor, RouteFlavor::JsonBody) && !input.has_extractors() {
        if let Some(arg) = input.args.first() {
            let schema = openapi_schema_tokens(&arg.ty);
            tokens.push(quote! {
                with_json_request_body(#schema)
            });
        }
    }

    if let Some(token) = uploaded_files_openapi_token(input) {
        tokens.push(token);
    }

    for arg in &input.args {
        let Some(extractor) = &arg.extractor else {
            continue;
        };

        match extractor {
            Extractor::Body => {
                let schema = openapi_schema_tokens(&arg.ty);
                tokens.push(quote! {
                    with_json_request_body(#schema)
                });
            }
            Extractor::Param(spec) => tokens.push(single_value_extractor_openapi_tokens(
                &spec.name,
                &arg.ty,
                spec.pipe.as_ref(),
                spec.default.as_ref(),
                SingleValueOpenApiKind::Path,
            )),
            Extractor::Query(spec) => {
                if let Some(name) = &spec.name {
                    tokens.push(single_value_extractor_openapi_tokens(
                        name,
                        &arg.ty,
                        spec.pipe.as_ref(),
                        spec.default.as_ref(),
                        SingleValueOpenApiKind::Query,
                    ));
                }
            }
            Extractor::Header(spec) => tokens.push(single_value_extractor_openapi_tokens(
                &spec.name,
                &arg.ty,
                spec.pipe.as_ref(),
                spec.default.as_ref(),
                SingleValueOpenApiKind::Header,
            )),
            Extractor::Request
            | Extractor::Params
            | Extractor::Headers
            | Extractor::HostParam(_)
            | Extractor::Ip(_)
            | Extractor::Response
            | Extractor::Session
            | Extractor::UploadedFile(_)
            | Extractor::UploadedFiles(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
}

fn single_value_extractor_schema_type(extractor_ty: &Type, pipe: Option<&Expr>) -> Type {
    if pipe.is_some() {
        return syn::parse_quote!(String);
    }

    extractor_ty.clone()
}

fn single_value_extractor_required(ty: &Type) -> bool {
    option_inner_type(ty).is_none()
}

fn single_value_extractor_schema(ty: &Type, pipe: Option<&Expr>) -> proc_macro2::TokenStream {
    let schema_ty = single_value_extractor_schema_type(ty, pipe);
    openapi_schema_tokens(&schema_ty)
}

fn single_value_extractor_required_schema(
    ty: &Type,
    pipe: Option<&Expr>,
    default: Option<&Expr>,
) -> (bool, proc_macro2::TokenStream) {
    (
        default.is_none() && single_value_extractor_required(ty),
        single_value_extractor_schema(ty, pipe),
    )
}

fn single_value_extractor_openapi_tokens(
    name: &LitStr,
    ty: &Type,
    pipe: Option<&Expr>,
    default: Option<&Expr>,
    kind: SingleValueOpenApiKind,
) -> proc_macro2::TokenStream {
    match kind {
        SingleValueOpenApiKind::Path => {
            let schema = single_value_extractor_schema(ty, pipe);
            quote! {
                with_path_parameter(#name, #schema)
            }
        }
        SingleValueOpenApiKind::Query => {
            let (required, schema) = single_value_extractor_required_schema(ty, pipe, default);
            quote! {
                with_query_parameter(#name, #required, #schema)
            }
        }
        SingleValueOpenApiKind::Header => {
            let (required, schema) = single_value_extractor_required_schema(ty, pipe, default);
            quote! {
                with_header_parameter(#name, #required, #schema)
            }
        }
    }
}

enum SingleValueOpenApiKind {
    Path,
    Query,
    Header,
}

fn uploaded_files_openapi_token(input: &RouteMethodInput) -> Option<proc_macro2::TokenStream> {
    let fields = input
        .args
        .iter()
        .filter_map(|arg| match &arg.extractor {
            Some(Extractor::UploadedFile(spec)) => Some(file_upload::UploadOpenApiField {
                name: spec.name.clone(),
                multiple: false,
                required: option_inner_type(&arg.ty).is_none(),
            }),
            Some(Extractor::UploadedFiles(spec)) => Some(file_upload::UploadOpenApiField {
                name: spec.name.clone(),
                multiple: true,
                required: true,
            }),
            _ => None,
        })
        .collect();

    file_upload::multipart_openapi_token(fields)
}
