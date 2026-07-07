use crate::{BootError, BootRequest, Result};
use bytes::Bytes;
use futures_util::stream;

/// Limits applied while parsing a multipart form.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultipartOptions {
    max_body_size: Option<usize>,
    max_field_size: Option<usize>,
    max_file_size: Option<usize>,
    max_fields: Option<usize>,
    max_files: Option<usize>,
}

impl MultipartOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_max_body_size(mut self, size: usize) -> Self {
        self.max_body_size = Some(size);
        self
    }

    pub fn with_max_field_size(mut self, size: usize) -> Self {
        self.max_field_size = Some(size);
        self
    }

    pub fn with_max_file_size(mut self, size: usize) -> Self {
        self.max_file_size = Some(size);
        self
    }

    pub fn with_max_fields(mut self, count: usize) -> Self {
        self.max_fields = Some(count);
        self
    }

    pub fn with_max_files(mut self, count: usize) -> Self {
        self.max_files = Some(count);
        self
    }

    pub fn max_body_size(&self) -> Option<usize> {
        self.max_body_size
    }

    pub fn max_field_size(&self) -> Option<usize> {
        self.max_field_size
    }

    pub fn max_file_size(&self) -> Option<usize> {
        self.max_file_size
    }

    pub fn max_fields(&self) -> Option<usize> {
        self.max_fields
    }

    pub fn max_files(&self) -> Option<usize> {
        self.max_files
    }
}

/// Parsed multipart form containing text fields and uploaded files.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MultipartForm {
    fields: Vec<MultipartField>,
    files: Vec<UploadedFile>,
}

impl MultipartForm {
    pub fn fields(&self) -> &[MultipartField] {
        &self.fields
    }

    pub fn files(&self) -> &[UploadedFile] {
        &self.files
    }

    pub fn field(&self, name: &str) -> Option<&MultipartField> {
        self.fields.iter().find(|field| field.name == name)
    }

    pub fn field_values(&self, name: &str) -> Vec<&str> {
        self.fields
            .iter()
            .filter(|field| field.name == name)
            .map(|field| field.value.as_str())
            .collect()
    }

    pub fn file(&self, name: &str) -> Option<&UploadedFile> {
        self.files.iter().find(|file| file.name == name)
    }

    pub fn files_by_name(&self, name: &str) -> Vec<&UploadedFile> {
        self.files.iter().filter(|file| file.name == name).collect()
    }
}

/// Text field parsed from a multipart form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultipartField {
    name: String,
    value: String,
    content_type: Option<String>,
}

impl MultipartField {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn value(&self) -> &str {
        &self.value
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }
}

/// Uploaded file parsed from a multipart form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UploadedFile {
    name: String,
    file_name: String,
    content_type: Option<String>,
    bytes: Vec<u8>,
}

impl UploadedFile {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn file_name(&self) -> &str {
        &self.file_name
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn size(&self) -> usize {
        self.bytes.len()
    }

    pub fn text(&self) -> Result<String> {
        String::from_utf8(self.bytes.clone())
            .map_err(|error| BootError::BadRequest(error.to_string()))
    }
}

impl BootRequest {
    /// Parse this request body as `multipart/form-data`.
    pub async fn multipart_form(&self) -> Result<MultipartForm> {
        self.multipart_form_with_options(MultipartOptions::default())
            .await
    }

    /// Parse this request body as `multipart/form-data` with explicit limits.
    pub async fn multipart_form_with_options(
        &self,
        options: MultipartOptions,
    ) -> Result<MultipartForm> {
        self.require_multipart_content_type()?;
        if options
            .max_body_size
            .is_some_and(|max_body_size| self.body().len() > max_body_size)
        {
            return Err(BootError::PayloadTooLarge(format!(
                "multipart body exceeds {} bytes",
                options.max_body_size.unwrap_or_default()
            )));
        }

        let boundary = multipart_boundary(self.content_type())?;
        let body = Bytes::copy_from_slice(self.body());
        let stream = stream::once(async move { Ok::<Bytes, std::io::Error>(body) });
        let mut multipart = multer::Multipart::new(stream, boundary);
        let mut form = MultipartForm::default();

        while let Some(field) = multipart
            .next_field()
            .await
            .map_err(|error| BootError::BadRequest(format!("invalid multipart form: {error}")))?
        {
            let name = field.name().unwrap_or_default().to_string();
            let file_name = field.file_name().map(str::to_string);
            let content_type = field.content_type().map(ToString::to_string);
            let bytes = field.bytes().await.map_err(|error| {
                BootError::BadRequest(format!("invalid multipart field: {error}"))
            })?;

            if let Some(file_name) = file_name {
                enforce_count_limit(form.files.len() + 1, options.max_files, "multipart files")?;
                enforce_size_limit(bytes.len(), options.max_file_size, "multipart file")?;
                form.files.push(UploadedFile {
                    name,
                    file_name,
                    content_type,
                    bytes: bytes.to_vec(),
                });
            } else {
                enforce_count_limit(
                    form.fields.len() + 1,
                    options.max_fields,
                    "multipart fields",
                )?;
                enforce_size_limit(bytes.len(), options.max_field_size, "multipart field")?;
                let value = String::from_utf8(bytes.to_vec())
                    .map_err(|error| BootError::BadRequest(error.to_string()))?;
                form.fields.push(MultipartField {
                    name,
                    value,
                    content_type,
                });
            }
        }

        Ok(form)
    }

    pub fn require_multipart_content_type(&self) -> Result<()> {
        let Some(content_type) = self.content_type() else {
            return Err(BootError::UnsupportedMediaType(
                "expected multipart/form-data content type".to_string(),
            ));
        };

        if content_type
            .split(';')
            .next()
            .unwrap_or_default()
            .trim()
            .eq_ignore_ascii_case("multipart/form-data")
        {
            return Ok(());
        }

        Err(BootError::UnsupportedMediaType(format!(
            "expected multipart/form-data content type, got {content_type}"
        )))
    }
}

fn multipart_boundary(content_type: Option<&str>) -> Result<String> {
    let content_type = content_type.ok_or_else(|| {
        BootError::UnsupportedMediaType("expected multipart/form-data content type".to_string())
    })?;
    multer::parse_boundary(content_type)
        .map_err(|error| BootError::BadRequest(format!("invalid multipart boundary: {error}")))
}

fn enforce_size_limit(size: usize, limit: Option<usize>, subject: &str) -> Result<()> {
    if limit.is_some_and(|limit| size > limit) {
        return Err(BootError::PayloadTooLarge(format!(
            "{subject} exceeds {} bytes",
            limit.unwrap_or_default()
        )));
    }
    Ok(())
}

fn enforce_count_limit(count: usize, limit: Option<usize>, subject: &str) -> Result<()> {
    if limit.is_some_and(|limit| count > limit) {
        return Err(BootError::PayloadTooLarge(format!(
            "{subject} exceeds {} entries",
            limit.unwrap_or_default()
        )));
    }
    Ok(())
}
