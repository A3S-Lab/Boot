use proc_macro::TokenStream;
use quote::quote;

pub(crate) fn route_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn extractor_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message = format!("#[{name}] must be used on a route method argument inside #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn openapi_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn decorator_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn response_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn render_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn http_code_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn metadata_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn message_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[message_controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn event_attribute_outside_listener(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[event_listener]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn websocket_attribute_outside_gateway(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[websocket_gateway]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn schedule_attribute_outside_schedule(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message = format!("#[{name}] must be used inside an impl block annotated with #[schedule]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn validation_attribute_outside_controller(
    name: &str,
    item: TokenStream,
) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn pipeline_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn host_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn version_attribute_outside_controller(name: &str, item: TokenStream) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}

pub(crate) fn serialization_attribute_outside_controller(
    name: &str,
    item: TokenStream,
) -> TokenStream {
    let item = proc_macro2::TokenStream::from(item);
    let message =
        format!("#[{name}] must be used inside an impl block annotated with #[controller]");
    quote! {
        compile_error!(#message);
        #item
    }
    .into()
}
