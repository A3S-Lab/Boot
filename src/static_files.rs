use crate::{
    BootError, BootRequest, BootResponse, HttpMethod, Module, ProviderDefinition, ProviderToken,
    Result, RouteDefinition,
};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;

const STATIC_PATH_PARAM: &str = "path";

/// Options for serving static files through [`StaticModule`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaticFileOptions {
    serve_root: String,
    index_file: Option<String>,
    fallback_file: Option<String>,
    cache_control: Option<String>,
    dotfiles: bool,
}

impl Default for StaticFileOptions {
    fn default() -> Self {
        Self {
            serve_root: "/".to_string(),
            index_file: Some("index.html".to_string()),
            fallback_file: None,
            cache_control: None,
            dotfiles: false,
        }
    }
}

impl StaticFileOptions {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_serve_root(mut self, serve_root: impl Into<String>) -> Self {
        self.serve_root = normalize_serve_root(serve_root.into());
        self
    }

    pub fn with_index_file(mut self, file_name: impl Into<String>) -> Self {
        self.index_file = Some(file_name.into());
        self
    }

    pub fn without_index_file(mut self) -> Self {
        self.index_file = None;
        self
    }

    pub fn with_fallback_file(mut self, file_name: impl Into<String>) -> Self {
        self.fallback_file = Some(file_name.into());
        self
    }

    pub fn without_fallback_file(mut self) -> Self {
        self.fallback_file = None;
        self
    }

    pub fn with_cache_control(mut self, value: impl Into<String>) -> Self {
        self.cache_control = Some(value.into());
        self
    }

    pub fn serve_dotfiles(mut self) -> Self {
        self.dotfiles = true;
        self
    }

    pub fn serve_root(&self) -> &str {
        &self.serve_root
    }

    pub fn index_file(&self) -> Option<&str> {
        self.index_file.as_deref()
    }

    pub fn fallback_file(&self) -> Option<&str> {
        self.fallback_file.as_deref()
    }

    pub fn cache_control(&self) -> Option<&str> {
        self.cache_control.as_deref()
    }

    pub fn serves_dotfiles(&self) -> bool {
        self.dotfiles
    }
}

/// Provider-backed service that reads files from a configured root directory.
#[derive(Debug, Clone)]
pub struct StaticFileService {
    root: Arc<PathBuf>,
    options: StaticFileOptions,
}

impl StaticFileService {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_options(root, StaticFileOptions::default())
    }

    pub fn with_options(root: impl Into<PathBuf>, options: StaticFileOptions) -> Self {
        Self {
            root: Arc::new(root.into()),
            options,
        }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn options(&self) -> &StaticFileOptions {
        &self.options
    }

    pub async fn serve(&self, request: BootRequest) -> Result<BootResponse> {
        let request_path = request.param(STATIC_PATH_PARAM).unwrap_or_default();
        let response = self.serve_path(request_path, request.method()).await?;
        if request.method() == HttpMethod::Head {
            return Ok(
                BootResponse::empty(response.status()).with_headers(response_header_map(&response))
            );
        }
        Ok(response)
    }

    pub async fn serve_path(&self, request_path: &str, method: HttpMethod) -> Result<BootResponse> {
        if method != HttpMethod::Get && method != HttpMethod::Head {
            return Err(BootError::MethodNotAllowed(method.as_str().to_string()));
        }

        let relative_path = sanitize_relative_path(request_path, self.options.dotfiles)?;
        let root = tokio::fs::canonicalize(self.root.as_path())
            .await
            .map_err(|error| static_io_error("static root", self.root.as_path(), error))?;
        let requested_path = root.join(&relative_path);

        if let Some(response) = self.try_file_response(&root, &requested_path).await? {
            return Ok(response);
        }

        if relative_path.as_os_str().is_empty() {
            return Err(BootError::NotFound("static file was not found".to_string()));
        }

        if let Some(fallback_file) = &self.options.fallback_file {
            let fallback_path = root.join(sanitize_relative_path(fallback_file, true)?);
            if let Some(response) = self.try_file_response(&root, &fallback_path).await? {
                return Ok(response);
            }
        }

        Err(BootError::NotFound("static file was not found".to_string()))
    }

    async fn try_file_response(&self, root: &Path, path: &Path) -> Result<Option<BootResponse>> {
        let mut candidate = path.to_path_buf();
        let metadata = match tokio::fs::metadata(&candidate).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(static_io_error("static file", &candidate, error)),
        };

        if metadata.is_dir() {
            let Some(index_file) = &self.options.index_file else {
                return Ok(None);
            };
            candidate = candidate.join(index_file);
        }

        let canonical = match tokio::fs::canonicalize(&candidate).await {
            Ok(canonical) => canonical,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => return Err(static_io_error("static file", &candidate, error)),
        };
        if !canonical.starts_with(root) {
            return Err(BootError::Forbidden(
                "static file resolved outside the configured root".to_string(),
            ));
        }

        let metadata = tokio::fs::metadata(&canonical)
            .await
            .map_err(|error| static_io_error("static file", &canonical, error))?;
        if !metadata.is_file() {
            return Ok(None);
        }

        let body = tokio::fs::read(&canonical)
            .await
            .map_err(|error| static_io_error("static file", &canonical, error))?;
        let mut response = BootResponse::new(200, body)
            .with_content_type(content_type_for(&canonical))
            .with_content_length(metadata.len());
        if let Some(cache_control) = &self.options.cache_control {
            response = response.with_header("cache-control", cache_control);
        }
        Ok(Some(response))
    }
}

/// Module that serves static files, similar to Nest's `ServeStaticModule`.
#[derive(Debug, Clone)]
pub struct StaticModule {
    name: &'static str,
    service: StaticFileService,
    global: bool,
}

impl StaticModule {
    pub fn new(name: &'static str, root: impl Into<PathBuf>) -> Self {
        Self::with_options(name, root, StaticFileOptions::default())
    }

    pub fn with_options(
        name: &'static str,
        root: impl Into<PathBuf>,
        options: StaticFileOptions,
    ) -> Self {
        Self {
            name,
            service: StaticFileService::with_options(root, options),
            global: false,
        }
    }

    pub fn with_serve_root(mut self, serve_root: impl Into<String>) -> Self {
        self.service.options = self.service.options.clone().with_serve_root(serve_root);
        self
    }

    pub fn with_cache_control(mut self, value: impl Into<String>) -> Self {
        self.service.options = self.service.options.clone().with_cache_control(value);
        self
    }

    pub fn with_fallback_file(mut self, file_name: impl Into<String>) -> Self {
        self.service.options = self.service.options.clone().with_fallback_file(file_name);
        self
    }

    pub fn without_index_file(mut self) -> Self {
        self.service.options = self.service.options.clone().without_index_file();
        self
    }

    pub fn serve_dotfiles(mut self) -> Self {
        self.service.options = self.service.options.clone().serve_dotfiles();
        self
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }

    pub fn service(&self) -> StaticFileService {
        self.service.clone()
    }
}

impl Module for StaticModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(self.service.clone())])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<StaticFileService>()])
    }

    fn is_global(&self) -> bool {
        self.global
    }

    fn routes(&self) -> Result<Vec<RouteDefinition>> {
        let path = static_route_path(self.service.options.serve_root());
        let service = self.service.clone();
        Ok(vec![
            RouteDefinition::get(path.clone(), move |request: BootRequest| {
                let service = service.clone();
                async move { service.serve(request).await }
            })?,
            RouteDefinition::new(HttpMethod::Head, path, {
                let service = self.service.clone();
                move |request: BootRequest| {
                    let service = service.clone();
                    async move { service.serve(request).await }
                }
            })?,
        ])
    }
}

fn static_route_path(serve_root: &str) -> String {
    let serve_root = normalize_serve_root(serve_root.to_string());
    if serve_root == "/" {
        "/{*path}".to_string()
    } else {
        format!("{serve_root}/{{*path}}")
    }
}

fn normalize_serve_root(serve_root: String) -> String {
    let trimmed = serve_root.trim();
    if trimmed.is_empty() || trimmed == "/" {
        return "/".to_string();
    }

    let with_prefix = if trimmed.starts_with('/') {
        trimmed.to_string()
    } else {
        format!("/{trimmed}")
    };
    with_prefix.trim_end_matches('/').to_string()
}

fn sanitize_relative_path(path: &str, dotfiles: bool) -> Result<PathBuf> {
    let mut clean = PathBuf::new();
    let path = path.trim_start_matches('/');
    if path.is_empty() {
        return Ok(clean);
    }

    for component in Path::new(path).components() {
        match component {
            Component::Normal(part) => {
                let part = part.to_string_lossy();
                if !dotfiles && part.starts_with('.') {
                    return Err(BootError::Forbidden(
                        "static dotfiles are not served by default".to_string(),
                    ));
                }
                clean.push(part.as_ref());
            }
            Component::CurDir => {}
            Component::ParentDir | Component::RootDir | Component::Prefix(_) => {
                return Err(BootError::Forbidden(
                    "static file path escapes the configured root".to_string(),
                ));
            }
        }
    }

    Ok(clean)
}

fn content_type_for(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|extension| extension.to_str())
        .map(|extension| extension.to_ascii_lowercase())
        .as_deref()
    {
        Some("avif") => "image/avif",
        Some("css") => "text/css; charset=utf-8",
        Some("csv") => "text/csv; charset=utf-8",
        Some("gif") => "image/gif",
        Some("htm") | Some("html") => "text/html; charset=utf-8",
        Some("ico") => "image/x-icon",
        Some("jpg") | Some("jpeg") => "image/jpeg",
        Some("js") | Some("mjs") => "text/javascript; charset=utf-8",
        Some("json") | Some("map") => "application/json",
        Some("pdf") => "application/pdf",
        Some("png") => "image/png",
        Some("svg") => "image/svg+xml",
        Some("txt") => "text/plain; charset=utf-8",
        Some("wasm") => "application/wasm",
        Some("webp") => "image/webp",
        Some("xml") => "application/xml",
        _ => "application/octet-stream",
    }
}

fn response_header_map(response: &BootResponse) -> std::collections::BTreeMap<String, String> {
    response
        .header_entries()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect()
}

fn static_io_error(subject: &str, path: &Path, error: std::io::Error) -> BootError {
    if error.kind() == std::io::ErrorKind::NotFound {
        return BootError::NotFound(format!("{subject} was not found"));
    }

    BootError::Io(std::io::Error::new(
        error.kind(),
        format!("{subject} `{}`: {error}", path.display()),
    ))
}
