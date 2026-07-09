use super::{transport_error_from_status, MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use prost::Message;
use serde::Serialize;
use serde_json::Value;
use std::convert::Infallible;
use std::fmt;
use std::net::SocketAddr;
use std::task::{Context, Poll};
use std::time::Duration;
use tonic::body::Body;
use tonic::codegen::{http, BoxFuture as TonicBoxFuture, Service};
use tonic::transport::{Channel, Endpoint, Server};
use tonic::{Request, Response, Status};
use tonic_prost::ProstCodec;

const SERVICE_NAME: &str = "a3s.boot.transport.MessageTransport";
const SEND_PATH: &str = "/a3s.boot.transport.MessageTransport/Send";
const EMIT_PATH: &str = "/a3s.boot.transport.MessageTransport/Emit";

/// Options for the gRPC message transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrpcTransportOptions {
    request_timeout: Duration,
    connect_timeout: Duration,
    max_decoding_message_size: usize,
    max_encoding_message_size: usize,
}

impl GrpcTransportOptions {
    pub const DEFAULT_MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn request_timeout(&self) -> Duration {
        self.request_timeout
    }

    pub fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    pub fn max_decoding_message_size(&self) -> usize {
        self.max_decoding_message_size
    }

    pub fn max_encoding_message_size(&self) -> usize {
        self.max_encoding_message_size
    }

    pub fn with_request_timeout(mut self, timeout: Duration) -> Self {
        self.request_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    pub fn with_connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout.max(Duration::from_millis(1));
        self
    }

    pub fn with_max_decoding_message_size(mut self, max_message_size: usize) -> Self {
        self.max_decoding_message_size = max_message_size.max(1);
        self
    }

    pub fn with_max_encoding_message_size(mut self, max_message_size: usize) -> Self {
        self.max_encoding_message_size = max_message_size.max(1);
        self
    }

    pub fn with_max_message_size(mut self, max_message_size: usize) -> Self {
        let max_message_size = max_message_size.max(1);
        self.max_decoding_message_size = max_message_size;
        self.max_encoding_message_size = max_message_size;
        self
    }
}

impl Default for GrpcTransportOptions {
    fn default() -> Self {
        Self {
            request_timeout: Duration::from_secs(5),
            connect_timeout: Duration::from_secs(5),
            max_decoding_message_size: Self::DEFAULT_MAX_MESSAGE_SIZE,
            max_encoding_message_size: Self::DEFAULT_MAX_MESSAGE_SIZE,
        }
    }
}

/// gRPC transport for Boot message patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GrpcTransport {
    addr: SocketAddr,
    options: GrpcTransportOptions,
}

impl GrpcTransport {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            options: GrpcTransportOptions::default(),
        }
    }

    pub fn with_options(addr: SocketAddr, options: GrpcTransportOptions) -> Self {
        Self { addr, options }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn endpoint(&self) -> String {
        endpoint_from_addr(self.addr)
    }

    pub fn options(&self) -> GrpcTransportOptions {
        self.options
    }
}

impl MessageTransport for GrpcTransport {
    type Output = GrpcTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(GrpcTransportClient {
            endpoint: self.endpoint(),
            options: self.options,
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let addr = self.addr;
        let options = self.options;
        Box::pin(async move {
            let service = GrpcTransportService::new(app, options);
            Server::builder()
                .serve(addr, service)
                .await
                .map_err(grpc_error)
        })
    }
}

/// gRPC client for Boot message patterns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrpcTransportClient {
    endpoint: String,
    options: GrpcTransportOptions,
}

impl GrpcTransportClient {
    pub fn new(endpoint: impl Into<String>) -> Self {
        Self {
            endpoint: endpoint.into(),
            options: GrpcTransportOptions::default(),
        }
    }

    pub fn for_addr(addr: SocketAddr) -> Self {
        Self::new(endpoint_from_addr(addr))
    }

    pub fn with_options(endpoint: impl Into<String>, options: GrpcTransportOptions) -> Self {
        Self {
            endpoint: endpoint.into(),
            options,
        }
    }

    pub fn with_addr_options(addr: SocketAddr, options: GrpcTransportOptions) -> Self {
        Self::with_options(endpoint_from_addr(addr), options)
    }

    pub fn endpoint(&self) -> &str {
        &self.endpoint
    }

    pub fn options(&self) -> GrpcTransportOptions {
        self.options
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let channel = self.channel().await?;
        let mut client = tonic::client::Grpc::new(channel)
            .max_decoding_message_size(self.options.max_decoding_message_size)
            .max_encoding_message_size(self.options.max_encoding_message_size);
        client.ready().await.map_err(grpc_error)?;

        let request = GrpcTransportMessage {
            data: encode(&message)?,
        };
        let path = http::uri::PathAndQuery::from_static(SEND_PATH);
        let codec = ProstCodec::<GrpcTransportMessage, GrpcTransportReply>::default();
        let response = tokio::time::timeout(
            self.options.request_timeout,
            client.unary(Request::new(request), path, codec),
        )
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "grpc transport response timed out after {:?}",
                self.options.request_timeout
            ))
        })?
        .map_err(error_from_status)?
        .into_inner();

        response.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        let channel = self.channel().await?;
        let mut client = tonic::client::Grpc::new(channel)
            .max_decoding_message_size(self.options.max_decoding_message_size)
            .max_encoding_message_size(self.options.max_encoding_message_size);
        client.ready().await.map_err(grpc_error)?;

        let request = GrpcTransportMessage {
            data: encode(&message)?,
        };
        let path = http::uri::PathAndQuery::from_static(EMIT_PATH);
        let codec = ProstCodec::<GrpcTransportMessage, GrpcTransportAck>::default();
        tokio::time::timeout(
            self.options.request_timeout,
            client.unary(Request::new(request), path, codec),
        )
        .await
        .map_err(|_| {
            BootError::Adapter(format!(
                "grpc transport emit timed out after {:?}",
                self.options.request_timeout
            ))
        })?
        .map_err(error_from_status)?;
        Ok(())
    }

    async fn channel(&self) -> Result<Channel> {
        Endpoint::from_shared(self.endpoint.clone())
            .map_err(grpc_error)?
            .connect_timeout(self.options.connect_timeout)
            .connect()
            .await
            .map_err(grpc_error)
    }
}

#[derive(Clone)]
struct GrpcTransportService {
    app: BootApplication,
    options: GrpcTransportOptions,
}

impl GrpcTransportService {
    fn new(app: BootApplication, options: GrpcTransportOptions) -> Self {
        Self { app, options }
    }
}

impl Service<http::Request<Body>> for GrpcTransportService {
    type Response = http::Response<Body>;
    type Error = Infallible;
    type Future = TonicBoxFuture<Self::Response, Self::Error>;

    fn poll_ready(&mut self, _cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, req: http::Request<Body>) -> Self::Future {
        match req.uri().path() {
            SEND_PATH => {
                let method = GrpcSendSvc {
                    app: self.app.clone(),
                };
                let codec = ProstCodec::<GrpcTransportReply, GrpcTransportMessage>::default();
                let mut grpc = tonic::server::Grpc::new(codec)
                    .max_decoding_message_size(self.options.max_decoding_message_size)
                    .max_encoding_message_size(self.options.max_encoding_message_size);
                Box::pin(async move { Ok(grpc.unary(method, req).await) })
            }
            EMIT_PATH => {
                let method = GrpcEmitSvc {
                    app: self.app.clone(),
                };
                let codec = ProstCodec::<GrpcTransportAck, GrpcTransportMessage>::default();
                let mut grpc = tonic::server::Grpc::new(codec)
                    .max_decoding_message_size(self.options.max_decoding_message_size)
                    .max_encoding_message_size(self.options.max_encoding_message_size);
                Box::pin(async move { Ok(grpc.unary(method, req).await) })
            }
            _ => Box::pin(async move { Ok(unimplemented_grpc_response()) }),
        }
    }
}

impl tonic::server::NamedService for GrpcTransportService {
    const NAME: &'static str = SERVICE_NAME;
}

#[derive(Clone)]
struct GrpcSendSvc {
    app: BootApplication,
}

impl tonic::server::UnaryService<GrpcTransportMessage> for GrpcSendSvc {
    type Response = GrpcTransportReply;
    type Future = BoxFuture<'static, std::result::Result<Response<Self::Response>, Status>>;

    fn call(&mut self, request: Request<GrpcTransportMessage>) -> Self::Future {
        let app = self.app.clone();
        Box::pin(async move {
            let response = match decode_message(request.into_inner().data.as_slice()) {
                Ok(message) => GrpcTransportReply::from_result(app.dispatch_message(message).await),
                Err(error) => GrpcTransportReply::from_error(error),
            };
            Ok(Response::new(response))
        })
    }
}

#[derive(Clone)]
struct GrpcEmitSvc {
    app: BootApplication,
}

impl tonic::server::UnaryService<GrpcTransportMessage> for GrpcEmitSvc {
    type Response = GrpcTransportAck;
    type Future = BoxFuture<'static, std::result::Result<Response<Self::Response>, Status>>;

    fn call(&mut self, request: Request<GrpcTransportMessage>) -> Self::Future {
        let app = self.app.clone();
        Box::pin(async move {
            let message =
                decode_message(request.into_inner().data.as_slice()).map_err(status_from_error)?;
            app.emit_message(message).await.map_err(status_from_error)?;
            Ok(Response::new(GrpcTransportAck {}))
        })
    }
}

#[derive(Clone, PartialEq, Message)]
struct GrpcTransportMessage {
    #[prost(bytes = "vec", tag = "1")]
    data: Vec<u8>,
}

#[derive(Clone, PartialEq, Message)]
struct GrpcTransportReply {
    #[prost(enumeration = "GrpcReplyKind", tag = "1")]
    kind: i32,
    #[prost(bytes = "vec", tag = "2")]
    data: Vec<u8>,
    #[prost(uint32, tag = "3")]
    status: u32,
    #[prost(string, tag = "4")]
    message: String,
}

impl GrpcTransportReply {
    fn from_result(result: Result<Option<TransportReply>>) -> Self {
        match result {
            Ok(Some(reply)) => match encode(&reply.data) {
                Ok(data) => Self {
                    kind: GrpcReplyKind::Reply as i32,
                    data,
                    status: 0,
                    message: String::new(),
                },
                Err(error) => Self::from_error(error),
            },
            Ok(None) => Self {
                kind: GrpcReplyKind::NoReply as i32,
                data: Vec::new(),
                status: 0,
                message: String::new(),
            },
            Err(error) => Self::from_error(error),
        }
    }

    fn from_error(error: BootError) -> Self {
        Self {
            kind: GrpcReplyKind::Error as i32,
            data: Vec::new(),
            status: error.http_status_code().into(),
            message: error.http_response_message(),
        }
    }

    fn into_result(self) -> Result<Option<TransportReply>> {
        match GrpcReplyKind::try_from(self.kind).unwrap_or(GrpcReplyKind::Error) {
            GrpcReplyKind::Reply => {
                let data = serde_json::from_slice::<Value>(&self.data)
                    .map_err(|err| BootError::Adapter(err.to_string()))?;
                Ok(Some(TransportReply::new(data)))
            }
            GrpcReplyKind::NoReply => Ok(None),
            GrpcReplyKind::Error => Err(transport_error_from_status(
                self.status.try_into().unwrap_or(500),
                self.message,
            )),
        }
    }
}

#[derive(Clone, PartialEq, Message)]
struct GrpcTransportAck {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, prost::Enumeration)]
#[repr(i32)]
enum GrpcReplyKind {
    Reply = 0,
    NoReply = 1,
    Error = 2,
}

fn encode<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))
}

fn decode_message(payload: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(payload).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn endpoint_from_addr(addr: SocketAddr) -> String {
    format!("http://{addr}")
}

fn unimplemented_grpc_response() -> http::Response<Body> {
    http::Response::builder()
        .status(200)
        .header("grpc-status", "12")
        .header("content-type", "application/grpc")
        .body(Body::empty())
        .unwrap_or_else(|_| http::Response::new(Body::empty()))
}

fn status_from_error(error: BootError) -> Status {
    if let BootError::Io(error) = error {
        return Status::unavailable(error.to_string());
    }

    let status = error.http_status_code();
    let message = error.http_response_message();
    match status {
        400 | 415 | 422 => Status::invalid_argument(message),
        401 => Status::unauthenticated(message),
        403 => Status::permission_denied(message),
        404 => Status::not_found(message),
        406 | 412 => Status::failed_precondition(message),
        408 | 504 => Status::deadline_exceeded(message),
        409 => Status::already_exists(message),
        413 => Status::out_of_range(message),
        429 => Status::resource_exhausted(message),
        501 => Status::unimplemented(message),
        502 | 503 => Status::unavailable(message),
        _ => Status::internal(message),
    }
}

fn error_from_status(status: Status) -> BootError {
    let message = status.message().to_string();
    match status.code() {
        tonic::Code::InvalidArgument => BootError::bad_request(message),
        tonic::Code::Unauthenticated => BootError::unauthorized(message),
        tonic::Code::PermissionDenied => BootError::forbidden(message),
        tonic::Code::NotFound => BootError::not_found(message),
        tonic::Code::FailedPrecondition => BootError::precondition_failed(message),
        tonic::Code::DeadlineExceeded => BootError::gateway_timeout(message),
        tonic::Code::AlreadyExists | tonic::Code::Aborted => BootError::conflict(message),
        tonic::Code::OutOfRange => BootError::payload_too_large(message),
        tonic::Code::ResourceExhausted => BootError::too_many_requests(message),
        tonic::Code::Unimplemented => BootError::not_implemented(message),
        tonic::Code::Unavailable => BootError::service_unavailable(message),
        tonic::Code::Internal => BootError::internal_server_error(message),
        _ => BootError::Adapter(status.to_string()),
    }
}

fn grpc_error(error: impl fmt::Display) -> BootError {
    BootError::Adapter(error.to_string())
}
