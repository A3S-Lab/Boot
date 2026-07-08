mod attrs;
mod input;
mod routing;

pub(crate) use input::{MethodArg, RouteMethodInput};

use quote::{format_ident, quote};
use syn::{Attribute, Expr, Ident, ImplItem, ImplItemFn, ItemImpl, LitInt, LitStr, Result, Type};

use crate::file_upload;
use crate::openapi::schema_tokens as openapi_schema_tokens;
use crate::openapi::RouteSpec as RouteOpenApiSpec;
use crate::validation::{
    take_controller_validation_attrs, take_route_validation_attrs,
    AttrOptions as ValidationAttrOptions,
};
use crate::{option_inner_type, push_error};

use attrs::*;
use input::{Extractor, SingleValueExtractor};
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
    let (clean_impl_attrs, controller_validation, controller_validation_errors) =
        take_controller_validation_attrs(&item_impl.attrs);
    let (clean_impl_attrs, controller_openapi, controller_openapi_errors) =
        take_controller_openapi_attrs(&clean_impl_attrs);
    let (clean_impl_attrs, controller_metadata, controller_metadata_errors) =
        take_controller_metadata_attrs(&clean_impl_attrs);
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
    let controller_pipeline = controller_pipeline.tokens();
    let controller_host = controller_host.tokens();
    let controller_version = controller_version.tokens();
    let controller_serialization = controller_serialization.tokens();

    for item in &mut item_impl.items {
        let ImplItem::Fn(method) = item else {
            continue;
        };

        let (clean_attrs, method_routes, route_errors) = take_route_attrs(&method.attrs);
        let (clean_attrs, route_validation, validation_errors) =
            take_route_validation_attrs(&clean_attrs);
        let (clean_attrs, openapi_specs, openapi_errors) = take_route_openapi_attrs(&clean_attrs);
        let (clean_attrs, metadata_specs, metadata_errors) =
            take_route_metadata_attrs(&clean_attrs);
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

fn metadata_route_definition(
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

fn response_route_definition(
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

fn pipeline_route_definition(
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

fn host_route_definition(
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

fn version_route_definition(
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

fn serialization_route_definition(
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

fn validation_route_definition(
    mut route_definition: proc_macro2::TokenStream,
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    validation_options: Option<ValidationAttrOptions>,
    validation_skipped: bool,
) -> Result<proc_macro2::TokenStream> {
    if validation_skipped {
        return Ok(quote! {
            (#route_definition).without_validation()
        });
    }

    let Some(options) = validation_options else {
        return Ok(route_definition);
    };

    for token in extractor_validation_tokens(input, flavor, options) {
        route_definition = quote! {
            (#route_definition).#token
        };
    }

    if options.is_empty() {
        Ok(quote! {
            (#route_definition).with_validation()
        })
    } else {
        let options = options.token();
        Ok(quote! {
            (#route_definition).with_validation_options(#options)
        })
    }
}

fn extractor_validation_tokens(
    input: &RouteMethodInput,
    flavor: RouteFlavor,
    options: ValidationAttrOptions,
) -> Vec<proc_macro2::TokenStream> {
    let mut tokens = Vec::new();
    let use_options = !options.is_empty();
    let options_token = options.token();

    if matches!(flavor, RouteFlavor::JsonBody) && !input.has_extractors() {
        if let Some(arg) = input.args.first() {
            let ty = &arg.ty;
            if use_options {
                tokens.push(quote! {
                    with_body_validation_options::<#ty>(#options_token)
                });
            } else {
                tokens.push(quote! {
                    with_body_validation::<#ty>()
                });
            }
        }
    }

    for arg in &input.args {
        let Some(extractor) = &arg.extractor else {
            continue;
        };
        let ty = &arg.ty;

        match extractor {
            Extractor::Body => {
                if use_options {
                    tokens.push(quote! {
                        with_body_validation_options::<#ty>(#options_token)
                    });
                } else {
                    tokens.push(quote! {
                        with_body_validation::<#ty>()
                    });
                }
            }
            Extractor::Params => {
                if use_options {
                    tokens.push(quote! {
                        with_params_validation_options::<#ty>(#options_token)
                    });
                } else {
                    tokens.push(quote! {
                        with_params_validation::<#ty>()
                    });
                }
            }
            Extractor::Query(query) => {
                if query.name.is_none() {
                    if use_options {
                        tokens.push(quote! {
                            with_query_validation_options::<#ty>(#options_token)
                        });
                    } else {
                        tokens.push(quote! {
                            with_query_validation::<#ty>()
                        });
                    }
                }
            }
            Extractor::Request
            | Extractor::Param(_)
            | Extractor::Header(_)
            | Extractor::Headers
            | Extractor::HostParam(_)
            | Extractor::Ip(_)
            | Extractor::UploadedFile(_)
            | Extractor::UploadedFiles(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
}

fn raw_or_json_request_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    Ok(match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |#ident: #ty| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move { #controller_name.#method_ident(#ident).await }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move { #controller_name.#method_ident().await }
                }
            }
        },
    })
}

fn json_body_handler(method_ident: &Ident, input: MethodArg) -> proc_macro2::TokenStream {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let MethodArg { ident, ty, .. } = input;
    quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |#ident: #ty| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move { #controller_name.#method_ident(#ident).await }
            }
        }
    }
}

fn extracted_raw_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#extractors)*
                    #controller_name.#method_ident(#(#args),*).await
                }
            }
        }
    })
}

fn extracted_json_response_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
    status: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    __a3s_boot_request.require_accepts_json()?;
                    #(#extractors)*
                    let __a3s_boot_body = #controller_name.#method_ident(#(#args),*).await?;
                    ::a3s_boot::BootResponse::json_with_status(#status, &__a3s_boot_body)
                }
            }
        }
    })
}

fn rendered_view_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
    view: &LitStr,
    status: proc_macro2::TokenStream,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);

    if input.has_extractors() {
        let (extractors, args) = extracted_arguments(input)?;
        return Ok(quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        #(#extractors)*
                        let __a3s_boot_context =
                            #controller_name.#method_ident(#(#args),*).await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        });
    }

    Ok(match input.into_legacy_arg()? {
        Some(MethodArg { ident, ty, .. }) => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        let #ident: #ty = __a3s_boot_request.clone();
                        let __a3s_boot_context = #controller_name.#method_ident(#ident).await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        },
        None => quote! {
            {
                let #controller_name = ::std::sync::Arc::clone(&self);
                move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                    let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                    async move {
                        let __a3s_boot_renderer = __a3s_boot_request
                            .get::<::a3s_boot::ViewRenderer>()?;
                        let __a3s_boot_context = #controller_name.#method_ident().await?;
                        __a3s_boot_renderer
                            .render_response_with_status(#status, #view, &__a3s_boot_context)
                            .await
                    }
                }
            }
        },
    })
}

fn extracted_sse_handler(
    method_ident: &Ident,
    input: RouteMethodInput,
) -> Result<proc_macro2::TokenStream> {
    let controller_name = format_ident!("__a3s_boot_{}", method_ident);
    let (extractors, args) = extracted_arguments(input)?;

    Ok(quote! {
        {
            let #controller_name = ::std::sync::Arc::clone(&self);
            move |__a3s_boot_request: ::a3s_boot::BootRequest| {
                let #controller_name = ::std::sync::Arc::clone(&#controller_name);
                async move {
                    #(#extractors)*
                    #controller_name.#method_ident(#(#args),*).await
                }
            }
        }
    })
}

fn extracted_arguments(
    input: RouteMethodInput,
) -> Result<(Vec<proc_macro2::TokenStream>, Vec<Ident>)> {
    let mut body_arg: Option<Ident> = None;
    let mut multipart_arg: Option<Ident> = None;
    let mut extractors = Vec::new();
    let mut args = Vec::new();

    for arg in input.args {
        let extractor = arg.extractor.clone().ok_or_else(|| {
            syn::Error::new_spanned(
                &arg.ident,
                "all route arguments must use extractor attributes when any extractor is used",
            )
        })?;

        if matches!(extractor, Extractor::Body) {
            if multipart_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine #[body] with multipart upload extractors",
                ));
            }
            if let Some(existing) = body_arg {
                return Err(syn::Error::new_spanned(
                    existing,
                    "route methods can accept at most one #[body] argument",
                ));
            }
            body_arg = Some(arg.ident.clone());
        }
        if matches!(
            extractor,
            Extractor::UploadedFile(_) | Extractor::UploadedFiles(_)
        ) {
            if body_arg.is_some() {
                return Err(syn::Error::new_spanned(
                    arg.ident,
                    "route methods cannot combine multipart upload extractors with #[body]",
                ));
            }
            multipart_arg = Some(arg.ident.clone());
        }

        args.push(arg.ident.clone());
        extractors.push(extractor_tokens(arg, extractor));
    }

    if multipart_arg.is_some() {
        extractors.insert(
            0,
            quote! {
                let __a3s_boot_multipart_form = __a3s_boot_request.multipart_form().await?;
            },
        );
    }

    Ok((extractors, args))
}

fn extractor_tokens(arg: MethodArg, extractor: Extractor) -> proc_macro2::TokenStream {
    let MethodArg { ident, ty, .. } = arg;
    match extractor {
        Extractor::Body => quote! {
            __a3s_boot_request.require_json_content_type()?;
            let #ident: #ty = __a3s_boot_request.json::<#ty>()?;
        },
        Extractor::Request => quote! {
            let #ident: #ty = __a3s_boot_request.clone();
        },
        Extractor::Params => quote! {
            let #ident: #ty = __a3s_boot_request.params::<#ty>()?;
        },
        Extractor::Param(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.param_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_param_as::<#value_ty>(#name)),
            )
        }
        Extractor::Query(spec) => {
            if let Some(name) = spec.name {
                single_value_extractor_tokens(
                    ident,
                    ty,
                    spec.pipe,
                    spec.default,
                    |value_ty| quote!(__a3s_boot_request.query_value_as::<#value_ty>(#name)),
                    |value_ty| quote!(__a3s_boot_request.optional_query_value_as::<#value_ty>(#name)),
                )
            } else {
                quote! {
                    let #ident: #ty = __a3s_boot_request.query::<#ty>()?;
                }
            }
        }
        Extractor::Header(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.header_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_header_as::<#value_ty>(#name)),
            )
        }
        Extractor::Headers => quote! {
            let #ident: #ty = __a3s_boot_request.headers.clone();
        },
        Extractor::HostParam(spec) => {
            let SingleValueExtractor {
                name,
                pipe,
                default,
            } = spec;
            single_value_extractor_tokens(
                ident,
                ty,
                pipe,
                default,
                |value_ty| quote!(__a3s_boot_request.host_param_as::<#value_ty>(#name)),
                |value_ty| quote!(__a3s_boot_request.optional_host_param_as::<#value_ty>(#name)),
            )
        }
        Extractor::Ip(pipe) => single_value_extractor_tokens(
            ident,
            ty,
            pipe,
            None,
            |value_ty| quote!(__a3s_boot_request.ip_as::<#value_ty>()),
            |value_ty| quote!(__a3s_boot_request.optional_ip_as::<#value_ty>()),
        ),
        Extractor::UploadedFile(spec) => file_upload::uploaded_file_extractor_tokens(
            ident,
            ty.clone(),
            spec,
            option_inner_type(&ty).is_some(),
        ),
        Extractor::UploadedFiles(spec) => {
            file_upload::uploaded_files_extractor_tokens(ident, ty, spec)
        }
        Extractor::Custom(extractor) => quote! {
            let #ident: #ty = ::a3s_boot::extract_request_value::<#ty, _>(&__a3s_boot_request, #extractor)?;
        },
    }
}

fn single_value_extractor_tokens<Required, Optional>(
    ident: Ident,
    ty: Box<Type>,
    pipe: Option<Expr>,
    default: Option<Expr>,
    required: Required,
    optional: Optional,
) -> proc_macro2::TokenStream
where
    Required: FnOnce(&Type) -> proc_macro2::TokenStream,
    Optional: FnOnce(&Type) -> proc_macro2::TokenStream,
{
    if let Some(pipe) = pipe {
        if let Some(inner) = option_inner_type(&ty) {
            let value = optional(&parse_string_type());
            if let Some(default) = default {
                quote! {
                    let #ident: #ty = match #value? {
                        Some(__a3s_boot_value) => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                __a3s_boot_value,
                                #pipe,
                            )?)
                        }
                        None => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                ::std::string::ToString::to_string(&(#default)),
                                #pipe,
                            )?)
                        }
                    };
                }
            } else {
                quote! {
                    let #ident: #ty = match #value? {
                        Some(__a3s_boot_value) => {
                            Some(::a3s_boot::transform_request_value::<String, #inner, _>(
                                __a3s_boot_value,
                                #pipe,
                            )?)
                        }
                        None => None,
                    };
                }
            }
        } else if let Some(default) = default {
            let value = optional(&parse_string_type());
            quote! {
                let __a3s_boot_value = match #value? {
                    Some(__a3s_boot_value) => {
                        __a3s_boot_value
                    }
                    None => ::std::string::ToString::to_string(&(#default)),
                };
                let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                    __a3s_boot_value,
                    #pipe,
                )?;
            }
        } else {
            let value = required(&parse_string_type());
            quote! {
                let #ident: #ty = ::a3s_boot::transform_request_value::<String, #ty, _>(
                    #value?,
                    #pipe,
                )?;
            }
        }
    } else if let Some(inner) = option_inner_type(&ty) {
        let value = optional(&inner);
        if let Some(default) = default {
            quote! {
                let #ident: #ty = match #value? {
                    Some(__a3s_boot_value) => Some(__a3s_boot_value),
                    None => Some(#default),
                };
            }
        } else {
            quote! {
                let #ident: #ty = #value?;
            }
        }
    } else if let Some(default) = default {
        let value = optional(&ty);
        quote! {
            let #ident: #ty = match #value? {
                Some(__a3s_boot_value) => __a3s_boot_value,
                None => #default,
            };
        }
    } else {
        let value = required(&ty);
        quote! {
            let #ident: #ty = #value?;
        }
    }
}

fn parse_string_type() -> Type {
    syn::parse_quote!(String)
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

fn openapi_route_definition(
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
            | Extractor::UploadedFile(_)
            | Extractor::UploadedFiles(_)
            | Extractor::Custom(_) => {}
        }
    }

    tokens
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
