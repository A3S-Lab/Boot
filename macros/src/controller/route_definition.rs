use super::attrs::{
    HostSpec, MetadataSpec, PipelineSpec, RouteResponseSpec, SerializationSpec, VersionSpec,
};
use quote::quote;
use syn::Result;

pub(super) fn metadata_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    metadata_specs: &[MetadataSpec],
) -> proc_macro2::TokenStream {
    for spec in metadata_specs {
        let key = &spec.key;
        let value = &spec.value;
        route_definition = quote! {
            (#route_definition).with_metadata(#key, #value)?
        };
    }
    route_definition
}

pub(super) fn response_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    response_specs: &[RouteResponseSpec],
) -> Result<proc_macro2::TokenStream> {
    for spec in response_specs {
        let token = spec.token()?;
        route_definition = quote! {
            (#route_definition).#token
        };
    }
    Ok(route_definition)
}

pub(super) fn pipeline_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    pipeline_specs: &[PipelineSpec],
) -> proc_macro2::TokenStream {
    for spec in pipeline_specs {
        let token = spec.token();
        route_definition = quote! {
            (#route_definition).#token
        };
    }
    route_definition
}

pub(super) fn host_route_definition(
    route_definition: proc_macro2::TokenStream,
    host_spec: Option<&HostSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = host_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token?
    }
}

pub(super) fn version_route_definition(
    route_definition: proc_macro2::TokenStream,
    version_spec: Option<&VersionSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = version_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token
    }
}

pub(super) fn serialization_route_definition(
    route_definition: proc_macro2::TokenStream,
    serialization_spec: Option<&SerializationSpec>,
) -> proc_macro2::TokenStream {
    let Some(spec) = serialization_spec else {
        return route_definition;
    };
    let token = spec.token();
    quote! {
        (#route_definition).#token
    }
}
