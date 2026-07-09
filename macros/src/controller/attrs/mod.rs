mod cache;
mod host;
mod http_code;
mod metadata;
mod openapi;
mod pipeline;
mod response;
mod serialization;
mod version;

pub(in crate::controller) use cache::{
    take_controller_cache_attrs, take_route_cache_attrs, CacheSpec,
};
pub(in crate::controller) use host::{take_controller_host_attrs, take_route_host_attrs, HostSpec};
pub(in crate::controller) use http_code::take_route_http_code_attrs;
pub(crate) use metadata::{
    take_controller_metadata_attrs, take_route_metadata_attrs, MetadataSpec,
};
pub(in crate::controller) use openapi::{take_controller_openapi_attrs, take_route_openapi_attrs};
pub(crate) use pipeline::{
    take_controller_pipeline_attrs, take_route_pipeline_attrs, PipelineSpec,
};
pub(in crate::controller) use response::{
    take_route_render_attrs, take_route_response_attrs, RenderSpec, RouteResponseSpec,
};
pub(in crate::controller) use serialization::{
    take_controller_serialization_attrs, take_route_serialization_attrs, SerializationSpec,
};
pub(in crate::controller) use version::{
    take_controller_version_attrs, take_route_version_attrs, VersionSpec,
};

use syn::Attribute;

fn is_attribute_named(attr: &Attribute, name: &str) -> bool {
    attr.path()
        .segments
        .last()
        .is_some_and(|segment| segment.ident == name)
}
