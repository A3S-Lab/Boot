use super::{transport_error_from_status, MessageTransport, TransportMessage, TransportReply};
use crate::{BootApplication, BootError, BoxFuture, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::net::SocketAddr;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader, WriteHalf};
use tokio::net::{TcpListener, TcpStream};

/// Options for the newline-delimited JSON TCP message transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpTransportOptions {
    max_frame_len: usize,
}

impl TcpTransportOptions {
    pub const DEFAULT_MAX_FRAME_LEN: usize = 1024 * 1024;

    pub fn new() -> Self {
        Self::default()
    }

    pub fn max_frame_len(&self) -> usize {
        self.max_frame_len
    }

    pub fn with_max_frame_len(mut self, max_frame_len: usize) -> Self {
        self.max_frame_len = max_frame_len.max(1);
        self
    }
}

impl Default for TcpTransportOptions {
    fn default() -> Self {
        Self {
            max_frame_len: Self::DEFAULT_MAX_FRAME_LEN,
        }
    }
}

/// Production TCP message transport using newline-delimited JSON frames.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpTransport {
    addr: SocketAddr,
    options: TcpTransportOptions,
}

impl TcpTransport {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            options: TcpTransportOptions::default(),
        }
    }

    pub fn with_options(addr: SocketAddr, options: TcpTransportOptions) -> Self {
        Self { addr, options }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn options(&self) -> TcpTransportOptions {
        self.options
    }

    pub fn with_max_frame_len(mut self, max_frame_len: usize) -> Self {
        self.options = self.options.with_max_frame_len(max_frame_len);
        self
    }
}

impl MessageTransport for TcpTransport {
    type Output = TcpTransportClient;

    fn build(&self, _app: BootApplication) -> Result<Self::Output> {
        Ok(TcpTransportClient {
            addr: self.addr,
            options: self.options,
        })
    }

    fn serve(&self, app: BootApplication) -> BoxFuture<'static, Result<()>> {
        let addr = self.addr;
        let options = self.options;
        Box::pin(async move {
            let listener = TcpListener::bind(addr).await?;
            loop {
                let (stream, _) = listener.accept().await?;
                let app = app.clone();
                tokio::spawn(async move {
                    let _ = serve_connection(app, stream, options).await;
                });
            }
        })
    }
}

/// Client for the Boot TCP message transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpTransportClient {
    addr: SocketAddr,
    options: TcpTransportOptions,
}

impl TcpTransportClient {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            options: TcpTransportOptions::default(),
        }
    }

    pub fn with_options(addr: SocketAddr, options: TcpTransportOptions) -> Self {
        Self { addr, options }
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    pub fn options(&self) -> TcpTransportOptions {
        self.options
    }

    pub fn with_max_frame_len(mut self, max_frame_len: usize) -> Self {
        self.options = self.options.with_max_frame_len(max_frame_len);
        self
    }

    pub async fn send(&self, message: TransportMessage) -> Result<Option<TransportReply>> {
        let mut stream = TcpStream::connect(self.addr).await?;
        write_json_line(&mut stream, &message, self.options.max_frame_len).await?;

        let mut reader = BufReader::new(stream);
        let Some(frame) = read_frame(&mut reader, self.options.max_frame_len).await? else {
            return Err(BootError::Adapter(
                "tcp transport closed before replying".to_string(),
            ));
        };

        decode_response(&frame)?.into_result()
    }

    pub async fn emit(&self, message: TransportMessage) -> Result<()> {
        self.send(message).await.map(|_| ())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum TcpTransportResponse {
    Reply { data: Value },
    NoReply,
    Error { status: u16, message: String },
}

impl TcpTransportResponse {
    fn from_result(result: Result<Option<TransportReply>>) -> Self {
        match result {
            Ok(Some(reply)) => Self::Reply { data: reply.data },
            Ok(None) => Self::NoReply,
            Err(error) => Self::from_error(error),
        }
    }

    fn from_error(error: BootError) -> Self {
        Self::Error {
            status: error.http_status_code(),
            message: error.http_response_message(),
        }
    }

    fn into_result(self) -> Result<Option<TransportReply>> {
        match self {
            Self::Reply { data } => Ok(Some(TransportReply::new(data))),
            Self::NoReply => Ok(None),
            Self::Error { status, message } => Err(transport_error_from_status(status, message)),
        }
    }
}

async fn serve_connection(
    app: BootApplication,
    stream: TcpStream,
    options: TcpTransportOptions,
) -> Result<()> {
    let (reader, mut writer) = tokio::io::split(stream);
    let mut reader = BufReader::new(reader);

    loop {
        let frame = match read_frame(&mut reader, options.max_frame_len).await {
            Ok(Some(frame)) => frame,
            Ok(None) => return Ok(()),
            Err(error) => {
                write_response(
                    &mut writer,
                    TcpTransportResponse::from_error(error),
                    options.max_frame_len,
                )
                .await?;
                return Ok(());
            }
        };

        let response = match decode_message(&frame) {
            Ok(message) => TcpTransportResponse::from_result(app.dispatch_message(message).await),
            Err(error) => TcpTransportResponse::from_error(error),
        };

        write_response(&mut writer, response, options.max_frame_len).await?;
    }
}

async fn read_frame<R>(reader: &mut R, max_frame_len: usize) -> Result<Option<Vec<u8>>>
where
    R: AsyncBufRead + Unpin,
{
    let mut frame = Vec::new();
    let bytes = reader.read_until(b'\n', &mut frame).await?;
    if bytes == 0 {
        return Ok(None);
    }
    if frame.len() > max_frame_len {
        return Err(BootError::PayloadTooLarge(format!(
            "tcp transport frame exceeds {max_frame_len} bytes"
        )));
    }
    if !frame.ends_with(b"\n") {
        return Err(BootError::BadRequest(
            "tcp transport frame is missing newline delimiter".to_string(),
        ));
    }
    frame.pop();
    if frame.ends_with(b"\r") {
        frame.pop();
    }
    if frame.is_empty() {
        return Err(BootError::BadRequest(
            "tcp transport frame cannot be empty".to_string(),
        ));
    }
    Ok(Some(frame))
}

async fn write_response(
    writer: &mut WriteHalf<TcpStream>,
    response: TcpTransportResponse,
    max_frame_len: usize,
) -> Result<()> {
    write_json_line(writer, &response, max_frame_len).await
}

async fn write_json_line<W, T>(writer: &mut W, value: &T, max_frame_len: usize) -> Result<()>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let mut frame =
        serde_json::to_vec(value).map_err(|err| BootError::Internal(err.to_string()))?;
    if frame.len() > max_frame_len {
        return Err(BootError::PayloadTooLarge(format!(
            "tcp transport frame exceeds {max_frame_len} bytes"
        )));
    }
    frame.push(b'\n');
    writer.write_all(&frame).await?;
    writer.flush().await?;
    Ok(())
}

fn decode_message(frame: &[u8]) -> Result<TransportMessage> {
    serde_json::from_slice(frame).map_err(|err| BootError::BadRequest(err.to_string()))
}

fn decode_response(frame: &[u8]) -> Result<TcpTransportResponse> {
    serde_json::from_slice(frame).map_err(|err| BootError::Adapter(err.to_string()))
}
