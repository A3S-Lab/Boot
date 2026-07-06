use crate::{
    BootApplication, BootError, BootRequest, BootResponse, HttpAdapter, HttpMethod, Result,
    RouteDefinition,
};
use axum::body::{to_bytes, Body};
use axum::extract::Request;
use axum::http::{HeaderName, HeaderValue, Method, StatusCode};
use axum::response::Response;
use axum::routing::{delete, get, head, options, patch, post, put, MethodRouter};
use axum::Router;
use std::collections::BTreeMap;
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
        let mut router = Router::new();
        for route in app.routes().iter().cloned() {
            let path = route.path().to_string();
            router = router.route(&path, route_to_method_router(route, self.body_limit));
        }
        Ok(router)
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

fn route_to_method_router(route: RouteDefinition, body_limit: usize) -> MethodRouter {
    let method = route.method();
    let dispatch = move |request: Request| {
        let route = route.clone();
        async move { dispatch_route(route, request, body_limit).await }
    };

    match method {
        HttpMethod::Get => get(dispatch),
        HttpMethod::Post => post(dispatch),
        HttpMethod::Put => put(dispatch),
        HttpMethod::Patch => patch(dispatch),
        HttpMethod::Delete => delete(dispatch),
        HttpMethod::Options => options(dispatch),
        HttpMethod::Head => head(dispatch),
    }
}

async fn dispatch_route(route: RouteDefinition, request: Request, body_limit: usize) -> Response {
    let boot_request = match to_boot_request(route.method(), request, body_limit).await {
        Ok(request) => request,
        Err(err) => return error_response(StatusCode::BAD_REQUEST, err.to_string()),
    };

    match route.handler().call(boot_request).await {
        Ok(response) => to_axum_response(response),
        Err(err) => error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()),
    }
}

async fn to_boot_request(
    method: HttpMethod,
    request: Request,
    body_limit: usize,
) -> Result<BootRequest> {
    let path = request.uri().path().to_string();
    let headers = request
        .headers()
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect::<BTreeMap<_, _>>();
    let body = to_bytes(request.into_body(), body_limit)
        .await
        .map_err(|err| BootError::Adapter(err.to_string()))?
        .to_vec();

    Ok(BootRequest {
        method,
        path,
        headers,
        body,
    })
}

fn to_axum_response(response: BootResponse) -> Response {
    let status = StatusCode::from_u16(response.status).unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
    let mut builder = Response::builder().status(status);
    for (name, value) in response.headers {
        if let (Ok(name), Ok(value)) = (
            HeaderName::try_from(name.as_str()),
            HeaderValue::try_from(value.as_str()),
        ) {
            builder = builder.header(name, value);
        }
    }

    builder
        .body(Body::from(response.body))
        .unwrap_or_else(|err| error_response(StatusCode::INTERNAL_SERVER_ERROR, err.to_string()))
}

fn error_response(status: StatusCode, message: String) -> Response {
    Response::builder()
        .status(status)
        .header("content-type", "text/plain; charset=utf-8")
        .body(Body::from(message))
        .expect("valid static error response")
}

impl From<Method> for HttpMethod {
    fn from(method: Method) -> Self {
        match method {
            Method::POST => Self::Post,
            Method::PUT => Self::Put,
            Method::PATCH => Self::Patch,
            Method::DELETE => Self::Delete,
            Method::OPTIONS => Self::Options,
            Method::HEAD => Self::Head,
            _ => Self::Get,
        }
    }
}
