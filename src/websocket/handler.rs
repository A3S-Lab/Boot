use super::message::{IntoWebSocketReply, WebSocketMessage};
use crate::{BoxFuture, Result};
use std::future::Future;

pub(crate) type WebSocketHandlerFuture = BoxFuture<'static, Result<Option<WebSocketMessage>>>;

pub(crate) trait WebSocketMessageHandler: Send + Sync + 'static {
    fn call(&self, message: WebSocketMessage) -> WebSocketHandlerFuture;
}

pub(crate) struct WebSocketHandlerAdapter<H> {
    pub(crate) handler: H,
}

impl<H, Fut, R> WebSocketMessageHandler for WebSocketHandlerAdapter<H>
where
    H: Fn(WebSocketMessage) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = Result<R>> + Send + 'static,
    R: IntoWebSocketReply + Send + 'static,
{
    fn call(&self, message: WebSocketMessage) -> WebSocketHandlerFuture {
        let future = (self.handler)(message);
        Box::pin(async move { Ok(future.await?.into_websocket_reply()) })
    }
}
