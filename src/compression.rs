use crate::{BootResponse, BoxFuture, ExecutionContext, Interceptor, Result};
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::Write;

/// Response compression settings used by [`CompressionInterceptor`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompressionOptions {
    min_size: usize,
}

impl Default for CompressionOptions {
    fn default() -> Self {
        Self { min_size: 1024 }
    }
}

impl CompressionOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_min_size(mut self, min_size: usize) -> Self {
        self.min_size = min_size;
        self
    }

    pub fn min_size(&self) -> usize {
        self.min_size
    }
}

/// Interceptor that applies gzip response compression when the client accepts it.
#[derive(Debug, Clone, Default)]
pub struct CompressionInterceptor {
    options: CompressionOptions,
}

impl CompressionInterceptor {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_options(options: CompressionOptions) -> Self {
        Self { options }
    }

    pub fn options(&self) -> &CompressionOptions {
        &self.options
    }
}

impl Interceptor for CompressionInterceptor {
    fn after(
        &self,
        context: ExecutionContext,
        response: BootResponse,
    ) -> BoxFuture<'static, Result<BootResponse>> {
        let options = self.options.clone();
        Box::pin(async move { compress_response(response, &context, &options) })
    }
}

fn compress_response(
    mut response: BootResponse,
    context: &ExecutionContext,
    options: &CompressionOptions,
) -> Result<BootResponse> {
    if response.is_streaming()
        || !response.has_body()
        || !response.allows_body()
        || response.body().len() < options.min_size
        || response.header("content-encoding").is_some()
        || !accepts_gzip(context.request.header_values("accept-encoding"))
        || !is_compressible_response(&response)
    {
        return Ok(response);
    }

    let had_content_length = !response.header_values("content-length").is_empty();
    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(response.body())?;
    response.body = encoder.finish()?;
    response = response.with_header("content-encoding", "gzip");
    response = ensure_vary_accept_encoding(response);

    if had_content_length {
        response.headers.remove("content-length");
        response
            .appended_headers
            .retain(|(name, _)| !name.eq_ignore_ascii_case("content-length"));
        let content_length = response.body.len() as u64;
        response = response.with_content_length(content_length);
    }

    Ok(response)
}

fn accepts_gzip(values: Vec<&str>) -> bool {
    let mut gzip_q = None;
    let mut wildcard_q = None;

    for value in values {
        for coding in value.split(',') {
            let mut parts = coding.split(';');
            let name = parts.next().map(str::trim).unwrap_or_default();
            let mut q = 1.0;

            for parameter in parts {
                let Some((key, value)) = parameter.trim().split_once('=') else {
                    continue;
                };
                if key.trim().eq_ignore_ascii_case("q") {
                    q = value.trim().parse::<f32>().unwrap_or(0.0);
                }
            }

            if name.eq_ignore_ascii_case("gzip") {
                gzip_q = Some(q);
            } else if name == "*" {
                wildcard_q = Some(q);
            }
        }
    }

    gzip_q.or(wildcard_q).is_some_and(|q| q > 0.0)
}

fn is_compressible_response(response: &BootResponse) -> bool {
    let Some(content_type) = response.content_type() else {
        return false;
    };
    let media_type = content_type
        .split(';')
        .next()
        .unwrap_or_default()
        .trim()
        .to_ascii_lowercase();

    media_type.starts_with("text/")
        || matches!(
            media_type.as_str(),
            "application/json"
                | "application/javascript"
                | "application/xml"
                | "application/xhtml+xml"
                | "application/rss+xml"
                | "application/atom+xml"
                | "image/svg+xml"
        )
        || media_type.ends_with("+json")
        || media_type.ends_with("+xml")
}

fn ensure_vary_accept_encoding(response: BootResponse) -> BootResponse {
    let Some(vary) = response.header("vary").map(str::to_string) else {
        return response.with_header("vary", "accept-encoding");
    };
    let has_accept_encoding = vary
        .split(',')
        .map(str::trim)
        .any(|value| value == "*" || value.eq_ignore_ascii_case("accept-encoding"));

    if has_accept_encoding {
        response
    } else {
        response.with_header("vary", format!("{vary}, accept-encoding"))
    }
}
