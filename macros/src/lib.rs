use proc_macro::TokenStream;
use syn::parse_macro_input;
use syn::{Item, ItemImpl, LitStr};

mod controller;
mod dependency;
mod events;
mod file_upload;
mod messaging;
mod openapi;
mod outside;
mod schedule;
mod util;
mod validation;
mod websocket;

pub(crate) use util::{
    expect_no_extractor_args, is_type_ident, option_inner_type, parse_optional_comma, push_error,
    set_once,
};

use controller::expand_controller;
use dependency::{expand_catch, expand_injectable, expand_module, CatchArgs, ModuleArgs};
use events::expand_event_listener;
use messaging::expand_message_controller;
use outside::*;
use schedule::expand_schedule;
use websocket::{expand_websocket_gateway, WebSocketGatewayArgs};

#[proc_macro_attribute]
pub fn injectable(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[injectable] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item = parse_macro_input!(item as Item);
    match item {
        Item::Struct(item_struct) => expand_injectable(item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[injectable] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn module(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as ModuleArgs);
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => expand_module(args, item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[module] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn catch(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as CatchArgs);
    let item = parse_macro_input!(item as Item);

    match item {
        Item::Struct(item_struct) => expand_catch(args, item_struct)
            .unwrap_or_else(syn::Error::into_compile_error)
            .into(),
        item => syn::Error::new_spanned(item, "#[catch] can only be used on structs")
            .to_compile_error()
            .into(),
    }
}

#[proc_macro_attribute]
pub fn controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    let prefix = parse_macro_input!(attr as LitStr);
    let item_impl = parse_macro_input!(item as ItemImpl);

    expand_controller(prefix, item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn websocket_gateway(attr: TokenStream, item: TokenStream) -> TokenStream {
    let args = parse_macro_input!(attr as WebSocketGatewayArgs);
    let item_impl = parse_macro_input!(item as ItemImpl);

    expand_websocket_gateway(args, item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn message_controller(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[message_controller] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_message_controller(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn event_listener(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[event_listener] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_event_listener(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn schedule(attr: TokenStream, item: TokenStream) -> TokenStream {
    if !attr.is_empty() {
        return syn::Error::new(
            proc_macro2::TokenStream::from(attr)
                .into_iter()
                .next()
                .unwrap()
                .span(),
            "#[schedule] does not accept arguments",
        )
        .to_compile_error()
        .into();
    }

    let item_impl = parse_macro_input!(item as ItemImpl);
    expand_schedule(item_impl)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn subscribe_message(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("subscribe_message", item)
}

#[proc_macro_attribute]
pub fn on_gateway_init(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_init", item)
}

#[proc_macro_attribute]
pub fn on_gateway_connection(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_connection", item)
}

#[proc_macro_attribute]
pub fn on_gateway_disconnect(_attr: TokenStream, item: TokenStream) -> TokenStream {
    websocket_attribute_outside_gateway("on_gateway_disconnect", item)
}

#[proc_macro_attribute]
pub fn cron(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("cron", item)
}

#[proc_macro_attribute]
pub fn interval(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("interval", item)
}

#[proc_macro_attribute]
pub fn timeout(_attr: TokenStream, item: TokenStream) -> TokenStream {
    schedule_attribute_outside_schedule("timeout", item)
}

#[proc_macro_attribute]
pub fn message_pattern(_attr: TokenStream, item: TokenStream) -> TokenStream {
    message_attribute_outside_controller("message_pattern", item)
}

#[proc_macro_attribute]
pub fn event_pattern(_attr: TokenStream, item: TokenStream) -> TokenStream {
    message_attribute_outside_controller("event_pattern", item)
}

#[proc_macro_attribute]
pub fn on_event(_attr: TokenStream, item: TokenStream) -> TokenStream {
    event_attribute_outside_listener("on_event", item)
}

#[proc_macro_attribute]
pub fn all(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("all", item)
}

#[proc_macro_attribute]
pub fn get(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("get", item)
}

#[proc_macro_attribute]
pub fn sse(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("sse", item)
}

#[proc_macro_attribute]
pub fn post(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("post", item)
}

#[proc_macro_attribute]
pub fn put(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("put", item)
}

#[proc_macro_attribute]
pub fn patch(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("patch", item)
}

#[proc_macro_attribute]
pub fn delete(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("delete", item)
}

#[proc_macro_attribute]
pub fn options(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("options", item)
}

#[proc_macro_attribute]
pub fn head(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("head", item)
}

#[proc_macro_attribute]
pub fn get_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("get_json", item)
}

#[proc_macro_attribute]
pub fn post_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("post_json", item)
}

#[proc_macro_attribute]
pub fn put_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("put_json", item)
}

#[proc_macro_attribute]
pub fn patch_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("patch_json", item)
}

#[proc_macro_attribute]
pub fn delete_json(_attr: TokenStream, item: TokenStream) -> TokenStream {
    route_attribute_outside_controller("delete_json", item)
}

#[proc_macro_attribute]
pub fn body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("body", item)
}

#[proc_macro_attribute]
pub fn request(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("request", item)
}

#[proc_macro_attribute]
pub fn param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("param", item)
}

#[proc_macro_attribute]
pub fn params(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("params", item)
}

#[proc_macro_attribute]
pub fn query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("query", item)
}

#[proc_macro_attribute]
pub fn header(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("header", item)
}

#[proc_macro_attribute]
pub fn headers(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("headers", item)
}

#[proc_macro_attribute]
pub fn host_param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("host_param", item)
}

#[proc_macro_attribute]
pub fn ip(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("ip", item)
}

#[proc_macro_attribute]
pub fn uploaded_file(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("uploaded_file", item)
}

#[proc_macro_attribute]
pub fn uploaded_files(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("uploaded_files", item)
}

#[proc_macro_attribute]
pub fn extract(_attr: TokenStream, item: TokenStream) -> TokenStream {
    extractor_attribute_outside_controller("extract", item)
}

#[proc_macro_attribute]
pub fn host(_attr: TokenStream, item: TokenStream) -> TokenStream {
    host_attribute_outside_controller("host", item)
}

#[proc_macro_attribute]
pub fn version(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("version", item)
}

#[proc_macro_attribute]
pub fn versions(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("versions", item)
}

#[proc_macro_attribute]
pub fn version_neutral(_attr: TokenStream, item: TokenStream) -> TokenStream {
    version_attribute_outside_controller("version_neutral", item)
}

#[proc_macro_attribute]
pub fn serialize(_attr: TokenStream, item: TokenStream) -> TokenStream {
    serialization_attribute_outside_controller("serialize", item)
}

#[proc_macro_attribute]
pub fn tag(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("tag", item)
}

#[proc_macro_attribute]
pub fn operation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("operation", item)
}

#[proc_macro_attribute]
pub fn response(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("response", item)
}

#[proc_macro_attribute]
pub fn request_body(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("request_body", item)
}

#[proc_macro_attribute]
pub fn api_param(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("api_param", item)
}

#[proc_macro_attribute]
pub fn api_query(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("api_query", item)
}

#[proc_macro_attribute]
pub fn api_header(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("api_header", item)
}

#[proc_macro_attribute]
pub fn bearer_auth(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("bearer_auth", item)
}

#[proc_macro_attribute]
pub fn hide_from_openapi(_attr: TokenStream, item: TokenStream) -> TokenStream {
    openapi_attribute_outside_controller("hide_from_openapi", item)
}

#[proc_macro_attribute]
pub fn redirect(_attr: TokenStream, item: TokenStream) -> TokenStream {
    response_attribute_outside_controller("redirect", item)
}

#[proc_macro_attribute]
pub fn render(_attr: TokenStream, item: TokenStream) -> TokenStream {
    render_attribute_outside_controller("render", item)
}

#[proc_macro_attribute]
pub fn http_code(_attr: TokenStream, item: TokenStream) -> TokenStream {
    http_code_attribute_outside_controller("http_code", item)
}

#[proc_macro_attribute]
pub fn metadata(_attr: TokenStream, item: TokenStream) -> TokenStream {
    metadata_attribute_outside_controller("metadata", item)
}

#[proc_macro_attribute]
pub fn validate(_attr: TokenStream, item: TokenStream) -> TokenStream {
    validation_attribute_outside_controller("validate", item)
}

#[proc_macro_attribute]
pub fn skip_validation(_attr: TokenStream, item: TokenStream) -> TokenStream {
    validation_attribute_outside_controller("skip_validation", item)
}

#[proc_macro_derive(ValidationSchema, attributes(serde))]
pub fn derive_validation_schema(item: TokenStream) -> TokenStream {
    let item_struct = parse_macro_input!(item as syn::ItemStruct);
    validation::expand_validation_schema(item_struct)
        .unwrap_or_else(syn::Error::into_compile_error)
        .into()
}

#[proc_macro_attribute]
pub fn use_guard(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_guard", item)
}

#[proc_macro_attribute]
pub fn use_interceptor(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_interceptor", item)
}

#[proc_macro_attribute]
pub fn use_filter(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_filter", item)
}

#[proc_macro_attribute]
pub fn use_pipe(_attr: TokenStream, item: TokenStream) -> TokenStream {
    pipeline_attribute_outside_controller("use_pipe", item)
}
