use super::definition::RouteDefinition;
use crate::routing::handler::RouteHandler;
use crate::{BootRequest, BootResponse, HttpMethod, Result};
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
        Self::json(HttpMethod::Post, path, handler)
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
        Self::json(HttpMethod::Put, path, handler)
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
        Self::json(HttpMethod::Patch, path, handler)
    }

    pub fn delete<H>(path: impl Into<String>, handler: H) -> Result<Self>
    where
        H: RouteHandler,
    {
        Self::new(HttpMethod::Delete, path, handler)
    }

    fn json<T, H, Fut, R>(method: HttpMethod, path: impl Into<String>, handler: H) -> Result<Self>
    where
        T: DeserializeOwned + Send + 'static,
        H: Fn(T) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<R>> + Send + 'static,
        R: Serialize + Send + 'static,
    {
        Self::new(method, path, move |request: BootRequest| {
            let future = request.json::<T>().map(&handler);
            async move {
                let body = future?.await?;
                BootResponse::json(&body)
            }
        })
    }
}
