use crate::{BootError, Result};
use futures_core::Stream;
use std::fmt;
use std::pin::Pin;

pub type StreamableFileStream = Pin<Box<dyn Stream<Item = Result<Vec<u8>>> + Send + 'static>>;

/// Options for Nest-style streamable file responses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamableFileOptions {
    content_type: Option<String>,
    content_disposition: Option<String>,
    content_length: Option<u64>,
}

impl StreamableFileOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = Some(content_type.into());
        self
    }

    pub fn with_content_disposition(mut self, content_disposition: impl Into<String>) -> Self {
        self.content_disposition = Some(content_disposition.into());
        self
    }

    pub fn with_attachment(self, file_name: impl AsRef<str>) -> Result<Self> {
        Ok(self.with_content_disposition(content_disposition("attachment", file_name.as_ref())?))
    }

    pub fn with_inline(self, file_name: impl AsRef<str>) -> Result<Self> {
        Ok(self.with_content_disposition(content_disposition("inline", file_name.as_ref())?))
    }

    pub fn with_content_length(mut self, content_length: u64) -> Self {
        self.content_length = Some(content_length);
        self
    }

    pub fn content_type(&self) -> Option<&str> {
        self.content_type.as_deref()
    }

    pub fn content_disposition(&self) -> Option<&str> {
        self.content_disposition.as_deref()
    }

    pub fn content_length(&self) -> Option<u64> {
        self.content_length
    }
}

/// Adapter-neutral file response body, similar to Nest's `StreamableFile`.
pub struct StreamableFile {
    body: StreamableFileBody,
    options: StreamableFileOptions,
}

impl StreamableFile {
    pub fn bytes(body: impl Into<Vec<u8>>) -> Self {
        Self {
            body: StreamableFileBody::Bytes(body.into()),
            options: StreamableFileOptions::default(),
        }
    }

    pub fn stream<S>(stream: S) -> Self
    where
        S: Stream<Item = Result<Vec<u8>>> + Send + 'static,
    {
        Self {
            body: StreamableFileBody::Stream(Box::pin(stream)),
            options: StreamableFileOptions::default(),
        }
    }

    pub fn with_options(mut self, options: StreamableFileOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.options = self.options.with_content_type(content_type);
        self
    }

    pub fn with_content_disposition(mut self, content_disposition: impl Into<String>) -> Self {
        self.options = self.options.with_content_disposition(content_disposition);
        self
    }

    pub fn with_attachment(mut self, file_name: impl AsRef<str>) -> Result<Self> {
        self.options = self.options.with_attachment(file_name)?;
        Ok(self)
    }

    pub fn with_inline(mut self, file_name: impl AsRef<str>) -> Result<Self> {
        self.options = self.options.with_inline(file_name)?;
        Ok(self)
    }

    pub fn with_content_length(mut self, content_length: u64) -> Self {
        self.options = self.options.with_content_length(content_length);
        self
    }

    pub fn options(&self) -> &StreamableFileOptions {
        &self.options
    }

    pub(crate) fn into_parts(self) -> (StreamableFileBody, StreamableFileOptions) {
        (self.body, self.options)
    }
}

impl fmt::Debug for StreamableFile {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let body = match &self.body {
            StreamableFileBody::Bytes(bytes) => {
                return f
                    .debug_struct("StreamableFile")
                    .field("body", &format_args!("{} bytes", bytes.len()))
                    .field("options", &self.options)
                    .finish();
            }
            StreamableFileBody::Stream(_) => "stream",
        };

        f.debug_struct("StreamableFile")
            .field("body", &body)
            .field("options", &self.options)
            .finish()
    }
}

pub(crate) enum StreamableFileBody {
    Bytes(Vec<u8>),
    Stream(StreamableFileStream),
}

fn content_disposition(disposition_type: &str, file_name: &str) -> Result<String> {
    validate_file_name(file_name)?;
    let quoted = quote_file_name(&ascii_file_name_fallback(file_name));
    let mut value = format!("{disposition_type}; filename=\"{quoted}\"");
    if !file_name.is_ascii() || quoted != file_name {
        value.push_str("; filename*=UTF-8''");
        value.push_str(&percent_encode_attr(file_name));
    }
    Ok(value)
}

fn validate_file_name(file_name: &str) -> Result<()> {
    if file_name.is_empty() {
        return Err(BootError::Internal("file name cannot be empty".to_string()));
    }

    if file_name
        .bytes()
        .any(|byte| byte == b'\r' || byte == b'\n' || byte == 0)
    {
        return Err(BootError::Internal(
            "file name contains invalid characters".to_string(),
        ));
    }

    Ok(())
}

fn ascii_file_name_fallback(file_name: &str) -> String {
    let value = file_name
        .chars()
        .map(|character| {
            if character.is_ascii()
                && !character.is_ascii_control()
                && !matches!(character, '"' | '\\' | '/' | ';')
            {
                character
            } else {
                '_'
            }
        })
        .collect::<String>()
        .trim_matches(|character: char| character == '_' || character.is_ascii_whitespace())
        .to_string();

    if value.is_empty() {
        "download".to_string()
    } else {
        value
    }
}

fn quote_file_name(file_name: &str) -> String {
    file_name
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .to_string()
}

fn percent_encode_attr(value: &str) -> String {
    let mut encoded = String::new();
    for byte in value.bytes() {
        if is_attr_char(byte) {
            encoded.push(char::from(byte));
        } else {
            encoded.push_str(&format!("%{byte:02X}"));
        }
    }
    encoded
}

fn is_attr_char(byte: u8) -> bool {
    matches!(
        byte,
        b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'#'
            | b'$'
            | b'&'
            | b'+'
            | b'-'
            | b'.'
            | b'^'
            | b'_'
            | b'`'
            | b'|'
            | b'~'
    )
}
