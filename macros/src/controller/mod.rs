pub(crate) mod attrs;
mod handler_arguments;
mod handlers;
mod input;
mod route_definition;
mod route_openapi;
mod route_validation;
mod routing;

pub(crate) use input::{MethodArg, ProtocolExtractor, ProtocolPayloadExtractor, RouteMethodInput};

use quote::quote;
use syn::{Attribute, ImplItem, ImplItemFn, ItemImpl, LitInt, LitStr, Result};

use crate::decorators::expand_apply_decorators_attrs;
use crate::openapi::RouteSpec as RouteOpenApiSpec;
use crate::push_error;
use crate::validation::{
    take_controller_validation_attrs, take_route_validation_attrs,
    AttrOptions as ValidationAttrOptions,
};

use attrs::*;
use handlers::{
    extracted_json_response_handler, extracted_raw_handler, extracted_sse_handler,
    json_body_handler, raw_or_json_request_handler, rendered_view_handler,
};
use route_definition::{
    cache_route_definition, host_route_definition, metadata_route_definition,
    pipeline_route_definition, response_route_definition, serialization_route_definition,
    version_route_definition,
};
use route_openapi::openapi_route_definition;
use route_validation::validation_route_definition;
use routing::{status_value, RouteArgs, RouteFlavor, RouteKind, RouteSpec};

pub(crate) fn expand_controller(
    prefix: LitStr,
    mut item_impl: ItemImpl,
) -> Result<proc_macro2::TokenStream> {
    if item_impl.trait_.is_some() {
        return Err(syn::Error::new_spanned(
            &item_impl,
            "#[controller] can only be used on inherent impl blocks",
        ));
    }

    let self_ty = item_impl.self_ty.clone();
    let mut routes = Vec::new();
    let mut errors: Option<syn::Error> = None;
    let (impl_attrs, impl_decorator_errors) = expand_apply_decorators_attrs(&item_impl.attrs);
    for error in impl_decorator_errors {
        push_error(&mut errors, error);
    }
    let (clean_impl_attrs, controller_validation, controller_validation_errors) =
        take_controller_validation_attrs(&impl_attrs);
    let (clean_impl_attrs, controller_openapi, controller_openapi_errors) =
        take_controller_openapi_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_metadata, controller_metadata_errors) =
        take_controller_metadata_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_cache, controller_cache_errors) =
        take_controller_cache_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_pipeline, controller_pipeline_errors) =
        take_controller_pipeline_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_host, controller_host_errors) =
        take_controller_host_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_version, controller_version_errors) =
        take_controller_version_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_serialization, controller_serialization_errors) =
        take_controller_serialization_attrs(&clean_impl_attrs);
    item_impl.attrs = clean_impl_attrs;
    for error in controller_validation_errors {
        push_error(&mut errors, error);
    }
    for error in controller_openapi_errors {
        push_error(&mut errors, error);
    }
    for error in controller_metadata_errors {
        push_error(&mut errors, error);
    }
    for error in controller_cache_errors {
        push_error(&mut errors, error);
    }
    for error in controller_pipeline_errors {
        push_error(&mut errors, error);
    }
    for error in controller_host_errors {
        push_error(&mut errors, error);
    }
    for error in controller_version_errors {
        push_error(&mut errors, error);
    }
    for error in controller_serialization_errors {
        push_error(&mut errors, error);
    }
    let controller_openapi = controller_openapi.tokens();
    let controller_metadata = controller_metadata.tokens();
    let controller_cache = controller_cache.tokens();
    let controller_pipeline = controller_pipeline.tokens();
    let controller_host = controller_host.tokens();
    let controller_version = controller_version.tokens();
    let controller_serialization = controller_serialization.tokens();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (method_attrs, decorator_errors) = expand_apply_decorators_attrs(&method.attrs);
        for error in decorator_errors {
            push_error(&mut errors, error);
        }
        let (clean_attrs, method_routes, route_errors) = take_route_attrs(&method_attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        let (clean_attrs, openapi_specs, openapi_errors) = take_route_openapi_attrs(&clean_attrs);
        let (clean_attrs, metadata_specs, metadata_errors) =
            take_route_metadata_attrs(&clean_attrs);
        let (clean_attrs, cache_specs, cache_errors) = take_route_cache_attrs(&clean_attrs);
        let (clean_attrs, http_code, http_code_errors) = take_route_http_code_attrs(&clean_attrs);
        let (clean_attrs, response_specs, response_errors) =
            take_route_response_attrs(&clean_attrs);
        let (clean_attrs, render_spec, render_errors) = take_route_render_attrs(&clean_attrs);
        let (clean_attrs, pipeline_specs, pipeline_errors) =
            take_route_pipeline_attrs(&clean_attrs);
        let (clean_attrs, host_specs, host_errors) = take_route_host_attrs(&clean_attrs);
        let (clean_attrs, version_specs, version_errors) = take_route_version_attrs(&clean_attrs);
        let (clean_attrs, serialization_specs, serialization_errors) =
            take_route_serialization_attrs(&clean_attrs);
        method.attrs = clean_attrs;
        for error in route_errors {
            push_error(&mut errors, error);
        }
        for error in validation_errors {
            push_error(&mut errors, error);
        }
        for error in openapi_errors {
            push_error(&mut errors, error);
        }
        for error in metadata_errors {
            push_error(&mut errors, error);
        }
        for error in cache_errors {
            push_error(&mut errors, error);
        }
        for error in http_code_errors {
            push_error(&mut errors, error);
        }
        for error in response_errors {
            push_error(&mut errors, error);
        }
        for error in render_errors {
            push_error(&mut errors, error);
        }
        for error in pipeline_errors {
            push_error(&mut errors, error);
        }
        for error in host_errors {
            push_error(&mut errors, error);
        }
        for error in version_errors {
            push_error(&mut errors, error);
        }
        for error in serialization_errors {
            push_error(&mut errors, error);
        }
        if method_routes.is_empty() && !openapi_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "OpenAPI route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !response_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "response route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && render_spec.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "render route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !pipeline_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "pipeline route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && host_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "host route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && version_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "version route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && serialization_specs.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "serialization route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && http_code.is_some() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "http_code route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !metadata_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "metadata route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && !cache_specs.is_empty() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "cache route attributes must be used on route methods",
                ),
            );
        }
        if method_routes.is_empty() && route_validation.is_present() {
            push_error(
                &mut errors,
                syn::Error::new_spanned(
                    &method.sig.ident,
                    "validation route attributes must be used on route methods",
                ),
            );
        }

        let input = if method_routes.is_empty() {
            None
        } else {
            match RouteMethodInput::from_method(method) {
                Ok(input) => Some(input),
                Err(error) => {
                    push_error(&mut errors, error);
                    None
                }
            }
        };

        for route in method_routes {
            let Some(input) = input.clone() else {
                continue;
            };
            let validation_options = route_validation.enabled_options(controller_validation);
            let validation_skipped = route_validation.skip;
            match route_registration(
                route,
                method,
                input,
                validation_options,
                validation_skipped,
                &metadata_specs,
                &cache_specs,
                http_code.as_ref(),
                &response_specs,
                render_spec.as_ref(),
                &pipeline_specs,
                host_specs.as_ref(),
                version_specs.as_ref(),
                serialization_specs.as_ref(),
                &openapi_specs,
            ) {
                Ok(registration) => routes.push(registration),
                Err(error) => push_error(&mut errors, error),
            }
        }
    }

    if let Some(error) = errors {
        return Err(error);
    }

    Ok(quote! {
        #item_impl

        impl #self_ty {
            pub fn controller(
                self: ::std::sync::Arc<Self>,
            ) -> ::a3s_boot::Result<::a3s_boot::ControllerDefinition> {
                let mut __a3s_boot_controller =
                    ::a3s_boot::ControllerDefinition::new(#prefix)?;
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_openapi;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_metadata?;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_cache;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_pipeline;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_host?;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_version;
                )*
                #(
                    __a3s_boot_controller = __a3s_boot_controller.#controller_serialization;
                )*
                #(
                    __a3s_boot_controller = #routes;
                )*
                Ok(__a3s_boot_controller)
            }
        }
    })
}

fn take_route_attrs(attrs: &[Attribute]) -> (Vec<Attribute>, Vec<RouteSpec>, Vec<syn::Error>) {
    let mut clean_attrs = Vec::new();
    let mut routes = Vec::new();
    let mut errors = Vec::new();

    for attr in attrs {
        let Some(kind) = RouteKind::from_attribute(attr) else {
            clean_attrs.push(attr.clone());
            continue;
        };

        match attr.parse_args::<RouteArgs>() {
            Ok(args) => routes.push(RouteSpec { kind, args }),
            Err(error) => errors.push(error),
        }
    }

    (clean_attrs, routes, errors)
}

fn route_registration(
    route: RouteSpec,
    method: &ImplItemFn,
    input: RouteMethodInput,
    validation_options: Option<ValidationAttrOptions>,
    validation_skipped: bool,
    metadata_specs: &[MetadataSpec],
    cache_specs: &[CacheSpec],
    http_code: Option<&LitInt>,
    response_specs: &[RouteResponseSpec],
    render_spec: Option<&RenderSpec>,
    pipeline_specs: &[PipelineSpec],
    host_spec: Option<&HostSpec>,
    version_spec: Option<&VersionSpec>,
    serialization_spec: Option<&SerializationSpec>,
    openapi_specs: &[RouteOpenApiSpec],
) -> Result<proc_macro2::TokenStream> {
    if method.sig.asyncness.is_none() {
        return Err(syn::Error::new_spanned(
            &method.sig.fn_token,
            "controller route methods must be async",
        ));
    }

    let method_ident = &method.sig.ident;
    let explicit_status = route.args.explicit_status(http_code)?;
    let status = status_value(explicit_status)?;
    let path = route.args.path.clone();
    let metadata_input = input.clone();

    let raw = route.args.raw.is_some();
    if raw && route.kind.is_explicit_json() {
        return Err(syn::Error::new_spanned(
            route.args.raw.unwrap(),
            "raw is not supported on *_json route attributes",
        ));
    }
    if let Some(render_spec) = render_spec {
        if raw {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on raw route attributes",
            ));
        }
        if route.kind == RouteKind::Sse {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on SSE route attributes",
            ));
        }
        if route.kind.is_explicit_json() {
            return Err(syn::Error::new_spanned(
                &render_spec.view,
                "render is not supported on *_json route attributes",
            ));
        }
    }

    let flavor = route.kind.flavor(raw);
    let mut json_success_status = None;
    let route_definition = if let Some(render_spec) = render_spec {
        let builder = route.kind.raw_builder_ident();
        let view = &render_spec.view;
        let handler = rendered_view_handler(method_ident, input.clone(), view, status.clone())?;
        quote! {
            ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                .with_response(#status, ::a3s_boot::OpenApiResponse::description("Success"))
                .with_metadata("render:view", #view)?
        }
    } else {
        match flavor {
            RouteFlavor::Sse => {
                if let Some(status) = explicit_status {
                    return Err(syn::Error::new_spanned(
                        status,
                        "status is not supported on SSE route attributes",
                    ));
                }
                if route.args.raw.is_some() {
                    return Err(syn::Error::new_spanned(
                        route.args.raw.unwrap(),
                        "raw is not supported on SSE route attributes",
                    ));
                }
                let handler = if input.has_extractors() {
                    extracted_sse_handler(method_ident, input)?
                } else {
                    raw_or_json_request_handler(method_ident, input)?
                };
                quote! {
                    ::a3s_boot::RouteDefinition::sse(#path, #handler)?
                }
            }
            RouteFlavor::Raw => {
                if let Some(status) = explicit_status {
                    return Err(syn::Error::new_spanned(
                        status,
                        "status is only supported on JSON route attributes",
                    ));
                }
                let builder = route.kind.raw_builder_ident();
                let handler = if input.has_extractors() {
                    extracted_raw_handler(method_ident, input)?
                } else {
                    raw_or_json_request_handler(method_ident, input)?
                };
                quote! {
                    ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                }
            }
            RouteFlavor::JsonRequest => {
                if input.has_extractors() {
                    let builder = route.kind.raw_builder_ident();
                    let handler =
                        extracted_json_response_handler(method_ident, input, status.clone())?;
                    json_success_status = Some(status.clone());
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                    }
                } else {
                    let builder = route.kind.json_builder_ident().ok_or_else(|| {
                        syn::Error::new_spanned(
                            &method.sig.ident,
                            "this HTTP method does not support JSON route inference",
                        )
                    })?;
                    let handler = raw_or_json_request_handler(method_ident, input)?;
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #status, #handler)?
                    }
                }
            }
            RouteFlavor::JsonBody => {
                if input.has_extractors() {
                    let builder = route.kind.raw_builder_ident();
                    let handler =
                        extracted_json_response_handler(method_ident, input, status.clone())?;
                    json_success_status = Some(status.clone());
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #handler)?
                    }
                } else {
                    let Some(input) = input.into_legacy_arg()? else {
                        return Err(syn::Error::new_spanned(
                            &method.sig.ident,
                            "JSON body routes must accept one DTO argument after &self",
                        ));
                    };
                    let builder = route.kind.json_builder_ident().ok_or_else(|| {
                        syn::Error::new_spanned(
                            &method.sig.ident,
                            "this HTTP method does not support JSON route inference",
                        )
                    })?;
                    let handler = json_body_handler(method_ident, input);
                    quote! {
                        ::a3s_boot::RouteDefinition::#builder(#path, #status, #handler)?
                    }
                }
            }
        }
    };

    let route_definition = validation_route_definition(
        route_definition,
        &metadata_input,
        flavor,
        validation_options,
        validation_skipped,
    )?;

    let route_definition = metadata_route_definition(route_definition, metadata_specs);

    let route_definition = cache_route_definition(route_definition, cache_specs);

    let route_definition = pipeline_route_definition(route_definition, pipeline_specs);

    let route_definition = host_route_definition(route_definition, host_spec);

    let route_definition = version_route_definition(route_definition, version_spec);

    let route_definition = serialization_route_definition(route_definition, serialization_spec);

    let route_definition = response_route_definition(route_definition, response_specs)?;

    let route_definition = openapi_route_definition(
        route_definition,
        &metadata_input,
        flavor,
        json_success_status,
        openapi_specs,
    )?;

    Ok(quote! {
        __a3s_boot_controller.route(#route_definition)?
    })
}
