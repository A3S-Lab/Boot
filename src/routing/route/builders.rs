use super::definition::RouteDefinition;
use crate::routing::handler::RouteHandler;
use crate::{BootRequest, BootResponse, HttpMethod, OpenApiResponse, Result, SseEvent, Validate};
use futures_core::Stream;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::future::Future;

impl RouteDefinition {
    pub fn get<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Get, path, handler)
    }

    pub fn get_json<H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::get_json_with_status(path, 200, handler)
    }

    pub fn get_json_with_status<H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_response_with_status(HttpMethod::Get, path, status, handler)
    }

    pub fn sse<H, Fut, S>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<S>> + Send + 'static,
        S: Stream<Item = Result<SseEvent>> + Send + 'static,
    {
        Self::new(HttpMethod::Get, path, move |request: BootRequest| {
            let future = request
                .require_accepts_event_stream()
                .map(|()| handler(request));
            async move {
                let stream = future?.await?;
                Ok(BootResponse::sse(stream))
            }
        })
    }

    pub fn post<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Post, path, handler)
    }

    pub fn post_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::post_json_with_status(path, 200, handler)
    }

    pub fn post_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_with_status(HttpMethod::Post, path, status, handler)
    }

    pub fn post_validated_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::post_validated_json_with_status(path, 200, handler)
    }

    pub fn post_validated_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::validated_json_with_status(HttpMethod::Post, path, status, handler)
    }

    pub fn put<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Put, path, handler)
    }

    pub fn put_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::put_json_with_status(path, 200, handler)
    }

    pub fn put_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_with_status(HttpMethod::Put, path, status, handler)
    }

    pub fn put_validated_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::put_validated_json_with_status(path, 200, handler)
    }

    pub fn put_validated_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::validated_json_with_status(HttpMethod::Put, path, status, handler)
    }

    pub fn patch<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Patch, path, handler)
    }

    pub fn patch_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::patch_json_with_status(path, 200, handler)
    }

    pub fn patch_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_with_status(HttpMethod::Patch, path, status, handler)
    }

    pub fn patch_validated_json<T, H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::patch_validated_json_with_status(path, 200, handler)
    }

    pub fn patch_validated_json_with_status<T, H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::validated_json_with_status(HttpMethod::Patch, path, status, handler)
    }

    pub fn delete<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Delete, path, handler)
    }

    pub fn delete_json<H, Fut, R>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::delete_json_with_status(path, 200, handler)
    }

    pub fn delete_json_with_status<H, Fut, R>(
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_response_with_status(HttpMethod::Delete, path, status, handler)
    }

    pub fn options<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Options, path, handler)
    }

    pub fn head<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Head, path, handler)
    }

    fn json_with_status<T, H, Fut, R>(
        method: HttpMethod,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::new(method, path, move |request: BootRequest| {
            let future = request
                .require_json_content_type()
                .and_then(|()| request.require_accepts_json())
                .and_then(|()| request.json::<T>())
                .map(&handler);
            async move {
                let body = future?.await?;
                BootResponse::json_with_status(status, &body)
            }
        })
        .map(|route| route.with_response(status, OpenApiResponse::description("Success")))
    }

    fn json_response_with_status<H, Fut, R>(
        method: HttpMethod,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        H: Fn(BootRequest) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::new(method, path, move |request: BootRequest| {
            let future = request.require_accepts_json().map(|()| handler(request));
            async move {
                let body = future?.await?;
                BootResponse::json_with_status(status, &body)
            }
        })
        .map(|route| route.with_response(status, OpenApiResponse::description("Success")))
    }

    fn validated_json_with_status<T, H, Fut, R>(
        method: HttpMethod,
        path: impl Into<String>,
        status: u16,
        handler: H,
    ) -> Result<Self>
    where
        T: DeserializeOwned + Validate + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::json_with_status(method, path, status, handler)
            .map(|route| route.with_body_validation::<T>().with_validation())
    }
}
