use quote::quote;
use syn::{Attribute, Ident, LitStr, Result, Type};

#[derive(Clone)]
pub(crate) struct UploadedFileExtractor {
    pub(crate) name: LitStr,
}

pub(crate) struct UploadOpenApiField {
    pub(crate) name: LitStr,
    pub(crate) multiple: bool,
    pub(crate) required: bool,
}

pub(crate) fn parse_uploaded_file_extractor(
    attr: &Attribute,
    name: &str,
) -> Result<UploadedFileExtractor> {
    attr.parse_args::<LitStr>()
        .map(|name| UploadedFileExtractor { name })
        .map_err(|_| {
            syn::Error::new_spanned(
                attr,
                format!("#[{name}] requires a multipart field-name string literal"),
            )
        })
}

pub(crate) fn uploaded_file_extractor_tokens(
    ident: Ident,
    ty: Box<Type>,
    spec: UploadedFileExtractor,
    optional: bool,
) -> proc_macro2::TokenStream {
    let name = spec.name;
    if optional {
        quote! {
            let #ident: #ty = __a3s_boot_multipart_form.file(#name).cloned();
        }
    } else {
        quote! {
            let #ident: #ty = __a3s_boot_multipart_form
                .file(#name)
                .cloned()
                .ok_or_else(|| ::a3s_boot::BootError::BadRequest(
                    format!("missing uploaded file: {}", #name)
                ))?;
        }
    }
}

pub(crate) fn uploaded_files_extractor_tokens(
    ident: Ident,
    ty: Box<Type>,
    spec: UploadedFileExtractor,
) -> proc_macro2::TokenStream {
    let name = spec.name;
    quote! {
        let #ident: #ty = __a3s_boot_multipart_form
            .files_by_name(#name)
            .into_iter()
            .cloned()
            .collect();
    }
}

pub(crate) fn multipart_openapi_token(
    fields: Vec<UploadOpenApiField>,
) -> Option<proc_macro2::TokenStream> {
    if fields.is_empty() {
        return None;
    }

    let mut properties = Vec::new();
    let mut required = Vec::new();

    for field in fields {
        let name = field.name;
        if field.multiple {
            properties.push(quote! {
                (
                    #name,
                    ::a3s_boot::OpenApiSchema::array(
                        ::a3s_boot::OpenApiSchema::binary_file()
                    )
                )
            });
        } else {
            properties.push(quote! {
                (#name, ::a3s_boot::OpenApiSchema::binary_file())
            });
        }

        if field.required {
            required.push(quote!(#name));
        }
    }

    let required = if required.is_empty() {
        quote!(::std::iter::empty::<&str>())
    } else {
        quote!([#(#required),*])
    };

    Some(quote! {
        with_request_body_content_type(
            "multipart/form-data",
            ::a3s_boot::OpenApiSchema::object_with_properties(
                [#(#properties),*],
                #required,
            )
        )
    })
}
