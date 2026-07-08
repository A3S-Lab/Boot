use crate::{
    BootApplication, BootError, BootRequest, BootResponse, HttpAdapter, HttpMethod, Result,
    RouteDefinition, WebSocketGatewayDefinition, WebSocketMessage,
};
use axum::body::{to_bytes, Body, HttpBody};
use axum::extract::ws::{Message as AxumWebSocketMessage, WebSocket, WebSocketUpgrade};
use axum::extract::Request;
use axum::http::{
    header::{ALLOW, CONTENT_TYPE},
    response::Builder as ResponseBuilder,
    HeaderMap, HeaderName, HeaderValue, Method, StatusCode, Uri,
};
use axum::response::Response;
use axum::routing::{on, MethodFilter, MethodRouter};
use axum::Router;
use futures_util::StreamExt;
use std::error::Error;
use std::net::SocketAddr;

/// Axum-backed HTTP adapter.
///
/// Axum is the default adapter, not the Boot kernel. Applications can replace
/// it by implementing [`HttpAdapter`] for another backend.
#[derive(Debug, Clone)]
pub struct AxumAdapter {
    body_limit: usize,
}

impl AxumAdapter {
    pub fn new() -> Self {
        Self {
            body_limit: 1024 * 1024,
        }
    }

    pub fn with_body_limit(mut self, body_limit: usize) -> Self {
        self.body_limit = body_limit;
        self
    }
}

impl Default for AxumAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl HttpAdapter for AxumAdapter {
    type Output = Router;

    fn build(&self, app: BootApplication) -> Result<Self::Output> {
        let fallback_routing = app.api_versioning().is_some()
            || app
                .routes()
                .iter()
                .any(|route| route.host().is_some() || route.method().is_wildcard());
        let mut router = if fallback_routing {
            let body_limit = self.body_limit;
            let app = app.clone();
            Router::new().fallback(move |request: Request| {
                let app = app.clone();
                async move { dispatch_application(app, request, body_limit).await }
            })
        } else {
            Router::new().fallback(not_found_fallback)
        };

        if !fallback_routing {
            for route in app.routes().iter().cloned() {
                let path = axum_route_path(route.path());
                router = router.route(
                    &path,
                    route_to_method_router(route, app.clone(), self.body_limit),
                );
            }
        }

        for gateway in app.gateways().iter().cloned() {
            let path = axum_route_path(gateway.path());
            router = router.route(&path, gateway_to_method_router(gateway));
        }
        let body_limit = self.body_limit;
        let app = app.clone();
        Ok(router.method_not_allowed_fallback(move |request: Request| {
            let app = app.clone();
            async move { dispatch_application(app, request, body_limit).await }
        }))
    }

    fn serve(
        &self,
        app: BootApplication,
        addr: SocketAddr,
    ) -> crate::BoxFuture<'static, Result<()>> {
        let adapter = self.clone();
        Box::pin(async move {
            let router = adapter.build(app)?;
            let listener = tokio::net::TcpListener::bind(addr).await?;
            axum::serve(listener, router).await?;
            Ok(())
        })
    }
}

fn gateway_to_method_router(gateway: WebSocketGatewayDefinition) -> MethodRouter {
    axum::routing::get(move |ws: WebSocketUpgrade, uri: Uri, headers: HeaderMap| {
        let gateway = gateway.clone();
        async move {
            ws.on_upgrade(move |socket| async move {
                handle_websocket(socket, gateway, uri, headers).await;
            })
        }
    })
}

fn route_to_method_router(
    route: RouteDefinition,
    app: BootApplication,
    body_limit: usize,
) -> MethodRouter {
    let method = route.method();
    let dispatch = move |request: Request| {
        let route = route.clone();
        let app = app.clone();
        async move { dispatch_route(route, app, request, body_limit).await }
    };

    on(method_filter(method), dispatch)
}

fn method_filter(method: HttpMethod) -> MethodFilter {
    match method {
        HttpMethod::All => MethodFilter::GET
            .or(MethodFilter::POST)
            .or(MethodFilter::PUT)
            .or(MethodFilter::PATCH)
            .or(MethodFilter::DELETE)
            .or(MethodFilter::OPTIONS)
            .or(MethodFilter::HEAD),
        HttpMethod::Get => MethodFilter::GET,
        HttpMethod::Post => MethodFilter::POST,
        HttpMethod::Put => MethodFilter::PUT,
        HttpMethod::Patch => MethodFilter::PATCH,
        HttpMethod::Delete => MethodFilter::DELETE,
        HttpMethod::Options => MethodFilter::OPTIONS,
        HttpMethod::Head => MethodFilter::HEAD,
    }
}

fn axum_route_path(path: &str) -> String {
    let path = path.strip_prefix('/').unwrap_or(path);
    if path.is_empty() {
        return "/".to_string();
    }

    let segments = path
        .split('/')
        .enumerate()
        .map(|(index, segment)| {
            if segment.starts_with('{') && segment.ends_with('}') {
                format!("{{p{index}}}")
            } else {
                segment.to_string()
            }
        })
        .collect::<Vec<_>>();

    format!("/{}", segments.join("/"))
}

async fn dispatch_route(
    route: RouteDefinition,
    app: BootApplication,
    request: Request,
    body_limit: usize,
) -> Response {
    let path = request.uri().path().to_string();
    let is_head = request.method() == Method::HEAD;
    let boot_request = match to_boot_request(request, body_limit).await {
        Ok(request) => request,
        Err(err) => return finalize_response(&app, &path, boot_error_response(err), is_head),
    };

    let response = match route.call(boot_request).await {
        Ok(response) => to_axum_response(response),
        Err(err) => boot_error_response(err),
    };
    finalize_response(&app, &path, response, is_head)
}

async fn dispatch_application(
    app: BootApplication,
    request: Request,
    body_limit: usize,
) -> Response {
    let path = request.uri().path().to_string();
    let is_head = request.method() == Method::HEAD;
    let boot_request = match to_boot_request(request, body_limit).await {
        Ok(request) => request,
        Err(err) => return finalize_response(&app, &path, boot_error_response(err), is_head),
    };

    let response = to_axum_response(app.handle(boot_request).await);
    finalize_response(&app, &path, response, is_head)
}

async fn not_found_fallback(request: Request) -> Response {
    let is_head = request.method() == Method::HEAD;
    let response = boot_error_response(BootError::NotFound(format!(
        "{} {}",
        request.method(),
        request.uri().path()
    )));
    strip_head_body(is_head, response)
}

async fn to_boot_request(axum_request: Request, body_limit: usize) -> Result<BootRequest> {
    let path = axum_request.uri().path().to_string();
    let method =
        HttpMethod::try_from(axum_request.method().clone()).map_err(|error| match error {
            BootError::MethodNotAllowed(method) => {
                BootError::MethodNotAllowed(format!("{method} {path}"))
            }
            error => error,
        })?;
    let query_string = axum_request.uri().query().map(str::to_string);
    let mut headers = Vec::new();
    for (name, value) in axum_request.headers() {
        let value = value.to_str().map_err(|err| {
            BootError::BadRequest(format!("invalid request header value for {name}: {err}"))
        })?;
        headers.push((name.as_str().to_string(), value.to_string()));
    }
    let mut boot_request = BootRequest::new(method, path);
    if let Some(query_string) = query_string {
        boot_request = boot_request.with_query_string(query_string);
    }
    for (name, value) in headers {
        boot_request = if boot_request.header(&name).is_some() {
            boot_request.append_header(name, value)
        } else {
            boot_request.with_header(name, value)
        };
    }

    boot_request.validate_headers()?;
    boot_request.validate_body_limit(body_limit)?;

    let body = axum_request.into_body();
    if body.size_hint().lower() > body_limit as u64 {
        return Err(BootError::PayloadTooLarge(format!(
            "request body exceeds {body_limit} bytes"
        )));
    }
    let body = to_bytes(body, body_limit)
        .await
        .map_err(|err| map_body_error(err, body_limit))?
        .to_vec();

    let boot_request = boot_request.with_body(body);
    boot_request.validate_with_body_limit(body_limit)?;
    Ok(boot_request)
}

fn websocket_boot_request(uri: Uri, headers: HeaderMap) -> Result<BootRequest> {
    let mut request = BootRequest::new(HttpMethod::Get, uri.path().to_string());
    if let Some(query) = uri.query() {
        request = request.with_query_string(query.to_string());
    }
    for (name, value) in &headers {
        let value = value.to_str().map_err(|err| {
            BootError::BadRequest(format!("invalid request header value for {name}: {err}"))
        })?;
        request = if request.header(name.as_str()).is_some() {
            request.append_header(name.as_str(), value)
        } else {
            request.with_header(name.as_str(), value)
        };
    }
    request.validate_headers()?;
    Ok(request)
}

async fn handle_websocket(
    mut socket: WebSocket,
    gateway: WebSocketGatewayDefinition,
    uri: Uri,
    headers: HeaderMap,
) {
    let request = match websocket_boot_request(uri, headers) {
        Ok(request) => request,
        Err(error) => {
            let _ = send_websocket_error(&mut socket, error).await;
            return;
        }
    };
    let connection = match gateway.connect(request) {
        Ok(connection) => connection,
        Err(error) => {
            let _ = send_websocket_error(&mut socket, error).await;
            return;
        }
    };

    while let Some(message) = socket.next().await {
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                let _ =
                    send_websocket_error(&mut socket, BootError::Adapter(error.to_string())).await;
                return;
            }
        };

        let Some(message) = decode_websocket_message(message) else {
            continue;
        };
        let message = match message {
            Ok(message) => message,
            Err(error) => {
                if send_websocket_error(&mut socket, error).await.is_err() {
                    return;
                }
                continue;
            }
        };

        match connection.dispatch(message).await {
            Ok(Some(reply)) => {
                if send_websocket_message(&mut socket, reply).await.is_err() {
                    return;
                }
            }
            Ok(None) => {}
            Err(error) => {
                if send_websocket_error(&mut socket, error).await.is_err() {
                    return;
                }
            }
        }
    }
}

fn decode_websocket_message(message: AxumWebSocketMessage) -> Option<Result<WebSocketMessage>> {
    match message {
        AxumWebSocketMessage::Text(text) => Some(
            serde_json::from_str(&text).map_err(|error| BootError::BadRequest(error.to_string())),
        ),
        AxumWebSocketMessage::Binary(bytes) => Some(
            serde_json::from_slice(&bytes)
                .map_err(|error| BootError::BadRequest(error.to_string())),
        ),
        AxumWebSocketMessage::Close(_) => None,
        AxumWebSocketMessage::Ping(_) | AxumWebSocketMessage::Pong(_) => None,
    }
}

async fn send_websocket_message(
    socket: &mut WebSocket,
    message: WebSocketMessage,
) -> std::result::Result<(), axum::Error> {
    let text = serde_json::to_string(&message)
        .unwrap_or_else(|error| format!(r#"{{"event":"error","data":"{error}"}}"#));
    socket.send(AxumWebSocketMessage::Text(text.into())).await
}

async fn send_websocket_error(
    socket: &mut WebSocket,
    error: BootError,
) -> std::result::Result<(), axum::Error> {
    let message = WebSocketMessage::text("error", error.http_response_message());
    send_websocket_message(socket, message).await
}

fn map_body_error(error: axum::Error, body_limit: usize) -> BootError {
    if error
        .source()
        .is_some_and(|source| source.is::<http_body_util::LengthLimitError>())
    {
        BootError::PayloadTooLarge(format!("request body exceeds {body_limit} bytes"))
    } else {
        BootError::Adapter(error.to_string())
    }
}

fn to_axum_response(response: BootResponse) -> Response {
    if let Err(error) = response.validate() {
        return error_response(StatusCode::INTERNAL_SERVER_ERROR, internal_message(error));
    }
    let is_streaming = response.is_streaming();
    let status = match StatusCode::from_u16(response.status()) {
        Ok(status) => status,
        Err(err) => {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("invalid response status {}: {err}", response.status()),
            );
        }
    };

    let mut builder = Response::builder().status(status);
    builder = match with_response_headers(builder, response.header_entries()) {
        Ok(builder) => builder,
        Err(message) => return error_response(StatusCode::INTERNAL_SERVER_ERROR, message),
    };

    let body = if is_streaming {
        let Some(stream) = response.into_sse_stream() else {
            return error_response(
                StatusCode::INTERNAL_SERVER_ERROR,
                "streaming response body has already been consumed".to_string(),
            );
        };
        Body::from_stream(stream.map(|event| event.map(|event| event.encode())))
    } else {
        Body::from(response.into_body())
    };

    builder
        .body(body)
        .unwrap_or_else(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

fn with_response_headers<I, N, V>(
    mut builder: ResponseBuilder,
    headers: I,
) -> std::result::Result<ResponseBuilder, String>
where
    I: IntoIterator<Item = (N, V)>,
    N: AsRef<str>,
    V: AsRef<str>,
{
    for (name, value) in headers {
        let name = name.as_ref();
        let value = value.as_ref();
        let header_name = HeaderName::try_from(name)
            .map_err(|err| format!("invalid response header name {name:?}: {err}"))?;
        let header_value = HeaderValue::try_from(value)
            .map_err(|err| format!("invalid response header value for {name:?}: {err}"))?;
        builder = builder.header(header_name, header_value);
    }

    Ok(builder)
}

fn internal_message(error: BootError) -> String {
    match error {
        BootError::Internal(message) => message,
        error => error.to_string(),
    }
}

fn with_allow_header(app: &BootApplication, path: &str, mut response: Response) -> Response {
    if response.status() != StatusCode::METHOD_NOT_ALLOWED {
        return response;
    }

    let Some(allow) = app.allowed_methods_header(path) else {
        return response;
    };

    match HeaderValue::try_from(allow) {
        Ok(value) => {
            response.headers_mut().insert(ALLOW, value);
            response
        }
        Err(err) => error_response(
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("invalid allow header value: {err}"),
        ),
    }
}

fn finalize_response(
    app: &BootApplication,
    path: &str,
    response: Response,
    is_head: bool,
) -> Response {
    strip_head_body(is_head, with_allow_header(app, path, response))
}

fn strip_head_body(is_head: bool, response: Response) -> Response {
    if !is_head {
        return response;
    }

    let (parts, _) = response.into_parts();
    Response::from_parts(parts, Body::empty())
}

fn boot_error_response(error: BootError) -> Response {
    to_axum_response(BootResponse::from_error(&error))
}

fn error_response(status: StatusCode, message: String) -> Response {
    let mut response = Response::new(Body::from(message));
    *response.status_mut() = status;
    response.headers_mut().insert(
        CONTENT_TYPE,
        HeaderValue::from_static("text/plain; charset=utf-8"),
    );
    response
}

impl TryFrom<Method> for HttpMethod {
    type Error = BootError;

    fn try_from(method: Method) -> std::result::Result<Self, Self::Error> {
        method.as_str().parse()
    }
}
