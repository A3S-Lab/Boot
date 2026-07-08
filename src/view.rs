use crate::{
    BootError, BootResponse, BoxFuture, Module, ProviderDefinition, ProviderToken, Result,
};
use serde::Serialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

const DEFAULT_VIEW_CONTENT_TYPE: &str = "text/html; charset=utf-8";

/// Rendering backend for Nest-style MVC view responses.
pub trait ViewEngine: Send + Sync + 'static {
    fn render(&self, view: String, context: Value) -> BoxFuture<'static, Result<String>>;
}

impl<F, Fut> ViewEngine for F
where
    F: Fn(String, Value) -> Fut + Send + Sync + 'static,
    Fut: std::future::Future<Output = Result<String>> + Send + 'static,
{
    fn render(&self, view: String, context: Value) -> BoxFuture<'static, Result<String>> {
        Box::pin(self(view, context))
    }
}

/// Provider that renders view names into HTML responses.
#[derive(Clone)]
pub struct ViewRenderer {
    engine: Arc<dyn ViewEngine>,
    content_type: String,
}

impl ViewRenderer {
    pub fn new<E>(engine: E) -> Self
    where
        E: ViewEngine,
    {
        Self::from_engine_arc(Arc::new(engine))
    }

    pub fn from_engine_arc(engine: Arc<dyn ViewEngine>) -> Self {
        Self {
            engine,
            content_type: DEFAULT_VIEW_CONTENT_TYPE.to_string(),
        }
    }

    pub fn with_content_type(mut self, content_type: impl Into<String>) -> Self {
        self.content_type = content_type.into();
        self
    }

    pub fn content_type(&self) -> &str {
        &self.content_type
    }

    pub async fn render<T>(&self, view: impl Into<String>, context: &T) -> Result<String>
    where
        T: Serialize,
    {
        let view = view.into();
        let context = serde_json::to_value(context).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize view context `{view}`: {error}"
            ))
        })?;
        self.render_value(view, context).await
    }

    pub async fn render_value(&self, view: impl Into<String>, context: Value) -> Result<String> {
        self.engine.render(view.into(), context).await
    }

    pub async fn render_response<T>(
        &self,
        view: impl Into<String>,
        context: &T,
    ) -> Result<BootResponse>
    where
        T: Serialize,
    {
        self.render_response_with_status(200, view, context).await
    }

    pub async fn render_response_with_status<T>(
        &self,
        status: u16,
        view: impl Into<String>,
        context: &T,
    ) -> Result<BootResponse>
    where
        T: Serialize,
    {
        let view = view.into();
        let context = serde_json::to_value(context).map_err(|error| {
            BootError::Internal(format!(
                "failed to serialize view context `{view}`: {error}"
            ))
        })?;
        self.render_value_response_with_status(status, view, context)
            .await
    }

    pub async fn render_value_response(
        &self,
        view: impl Into<String>,
        context: Value,
    ) -> Result<BootResponse> {
        self.render_value_response_with_status(200, view, context)
            .await
    }

    pub async fn render_value_response_with_status(
        &self,
        status: u16,
        view: impl Into<String>,
        context: Value,
    ) -> Result<BootResponse> {
        let html = self.render_value(view, context).await?;
        Ok(BootResponse::html_with_status(status, html).with_content_type(self.content_type()))
    }
}

impl fmt::Debug for ViewRenderer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ViewRenderer")
            .field("content_type", &self.content_type)
            .finish_non_exhaustive()
    }
}

/// Module that registers a [`ViewRenderer`] provider.
#[derive(Debug, Clone)]
pub struct ViewModule {
    name: &'static str,
    renderer: ViewRenderer,
    global: bool,
}

impl ViewModule {
    pub fn new<E>(name: &'static str, engine: E) -> Self
    where
        E: ViewEngine,
    {
        Self::from_renderer(name, ViewRenderer::new(engine))
    }

    pub fn from_renderer(name: &'static str, renderer: ViewRenderer) -> Self {
        Self {
            name,
            renderer,
            global: false,
        }
    }

    pub fn global(mut self) -> Self {
        self.global = true;
        self
    }

    pub fn renderer(&self) -> ViewRenderer {
        self.renderer.clone()
    }
}

impl Module for ViewModule {
    fn name(&self) -> &'static str {
        self.name
    }

    fn providers(&self) -> Result<Vec<ProviderDefinition>> {
        Ok(vec![ProviderDefinition::singleton(self.renderer.clone())])
    }

    fn exports(&self) -> Result<Vec<ProviderToken>> {
        Ok(vec![ProviderToken::of::<ViewRenderer>()])
    }

    fn is_global(&self) -> bool {
        self.global
    }
}

/// Small string-template engine for tests and lightweight HTML responses.
#[derive(Debug, Clone, Default)]
pub struct StringTemplateViewEngine {
    templates: Arc<BTreeMap<String, String>>,
}

impl StringTemplateViewEngine {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn from_templates<I, K, V>(templates: I) -> Self
    where
        I: IntoIterator<Item = (K, V)>,
        K: Into<String>,
        V: Into<String>,
    {
        Self {
            templates: Arc::new(
                templates
                    .into_iter()
                    .map(|(name, template)| (name.into(), template.into()))
                    .collect(),
            ),
        }
    }

    pub fn with_template(mut self, name: impl Into<String>, template: impl Into<String>) -> Self {
        Arc::make_mut(&mut self.templates).insert(name.into(), template.into());
        self
    }

    pub fn template(&self, name: &str) -> Option<&str> {
        self.templates.get(name).map(String::as_str)
    }
}

impl ViewEngine for StringTemplateViewEngine {
    fn render(&self, view: String, context: Value) -> BoxFuture<'static, Result<String>> {
        let template = self.templates.get(&view).cloned();
        Box::pin(async move {
            let Some(template) = template else {
                return Err(BootError::NotFound(format!("view was not found: {view}")));
            };
            Ok(render_string_template(&template, &context))
        })
    }
}

fn render_string_template(template: &str, context: &Value) -> String {
    let mut rendered = String::new();
    let mut rest = template;

    while let Some(start) = rest.find("{{") {
        rendered.push_str(&rest[..start]);
        let after_start = &rest[start + 2..];
        let Some(end) = after_start.find("}}") else {
            rendered.push_str(&rest[start..]);
            return rendered;
        };

        let key = after_start[..end].trim();
        if let Some(value) = context_path(context, key) {
            rendered.push_str(&view_value_to_string(value));
        }
        rest = &after_start[end + 2..];
    }

    rendered.push_str(rest);
    rendered
}

fn context_path<'a>(context: &'a Value, path: &str) -> Option<&'a Value> {
    if path.is_empty() {
        return None;
    }

    let mut value = context;
    for segment in path.split('.') {
        value = value.get(segment)?;
    }
    Some(value)
}

fn view_value_to_string(value: &Value) -> String {
    match value {
        Value::Null => String::new(),
        Value::Bool(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        Value::String(value) => value.clone(),
        Value::Array(_) | Value::Object(_) => value.to_string(),
    }
}
