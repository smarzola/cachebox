//! Minimal native socket client.

use std::fmt;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::net::{TcpStream, ToSocketAddrs};

use crate::protocol::{
    BatchItem, Command, DecodeError, ErrorCode, HEADER_LEN, Metadata, RequestFrame, RequestPayload,
    ResponsePayload, decode_response_frame, encode_request_frame_into,
};

#[derive(Debug)]
pub enum ClientError {
    Io(std::io::Error),
    Decode(DecodeError),
    Server { code: ErrorCode, message: String },
    UnexpectedResponse(&'static str),
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error: {error}"),
            Self::Decode(error) => write!(f, "decode error: {error:?}"),
            Self::Server { code, message } => write!(f, "server error {code:?}: {message}"),
            Self::UnexpectedResponse(message) => write!(f, "unexpected response: {message}"),
        }
    }
}

impl std::error::Error for ClientError {}

impl From<std::io::Error> for ClientError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<DecodeError> for ClientError {
    fn from(error: DecodeError) -> Self {
        Self::Decode(error)
    }
}

pub struct NativeClient {
    stream: NativeStream,
    next_request_id: u64,
    max_payload_len: usize,
    request_buffer: Vec<u8>,
    response_buffer: Vec<u8>,
}

enum NativeStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl NativeClient {
    pub async fn connect_tcp(addr: impl ToSocketAddrs) -> Result<Self, ClientError> {
        Ok(Self {
            stream: NativeStream::Tcp(TcpStream::connect(addr).await?),
            next_request_id: 1,
            max_payload_len: usize::MAX,
            request_buffer: Vec::new(),
            response_buffer: Vec::new(),
        })
    }

    #[cfg(unix)]
    pub async fn connect_unix(path: impl AsRef<std::path::Path>) -> Result<Self, ClientError> {
        Ok(Self {
            stream: NativeStream::Unix(UnixStream::connect(path).await?),
            next_request_id: 1,
            max_payload_len: usize::MAX,
            request_buffer: Vec::new(),
            response_buffer: Vec::new(),
        })
    }

    pub fn set_max_payload_len(&mut self, max_payload_len: usize) {
        self.max_payload_len = max_payload_len;
    }

    pub async fn get(
        &mut self,
        namespace: impl Into<String>,
        key: impl Into<Vec<u8>>,
    ) -> Result<ResponsePayload, ClientError> {
        self.request(
            Command::Get,
            RequestPayload::Get {
                namespace: namespace.into(),
                key: key.into(),
            },
        )
        .await
    }

    pub async fn put(
        &mut self,
        namespace: impl Into<String>,
        key: impl Into<Vec<u8>>,
        metadata: Metadata,
        value: impl Into<Vec<u8>>,
    ) -> Result<u32, ClientError> {
        match self
            .request(
                Command::Put,
                RequestPayload::Put {
                    namespace: namespace.into(),
                    key: key.into(),
                    metadata,
                    value: value.into(),
                },
            )
            .await?
        {
            ResponsePayload::Stored { evicted } => Ok(evicted),
            _ => Err(ClientError::UnexpectedResponse("put did not return stored")),
        }
    }

    pub async fn delete(
        &mut self,
        namespace: impl Into<String>,
        key: impl Into<Vec<u8>>,
    ) -> Result<bool, ClientError> {
        match self
            .request(
                Command::Delete,
                RequestPayload::Delete {
                    namespace: namespace.into(),
                    key: key.into(),
                },
            )
            .await?
        {
            ResponsePayload::Deleted { removed } => Ok(removed),
            _ => Err(ClientError::UnexpectedResponse(
                "delete did not return deleted",
            )),
        }
    }

    pub async fn batch_get(
        &mut self,
        namespace: impl Into<String>,
        keys: Vec<Vec<u8>>,
    ) -> Result<Vec<BatchItem>, ClientError> {
        match self
            .request(
                Command::BatchGet,
                RequestPayload::BatchGet {
                    namespace: namespace.into(),
                    keys,
                },
            )
            .await?
        {
            ResponsePayload::BatchGet { items } => Ok(items),
            _ => Err(ClientError::UnexpectedResponse(
                "batch get did not return batch",
            )),
        }
    }

    pub async fn invalidate_tag(
        &mut self,
        namespace: impl Into<String>,
        tag: impl Into<String>,
    ) -> Result<u32, ClientError> {
        match self
            .request(
                Command::TagInvalidate,
                RequestPayload::TagInvalidate {
                    namespace: namespace.into(),
                    tag: tag.into(),
                },
            )
            .await?
        {
            ResponsePayload::Invalidated { removed } => Ok(removed),
            _ => Err(ClientError::UnexpectedResponse(
                "tag invalidation did not return invalidated",
            )),
        }
    }

    pub async fn start_lease(
        &mut self,
        namespace: impl Into<String>,
        key: impl Into<Vec<u8>>,
        lease_ttl_ms: u64,
        allow_stale_ms: Option<u64>,
    ) -> Result<ResponsePayload, ClientError> {
        self.request(
            Command::LeaseStart,
            RequestPayload::LeaseStart {
                namespace: namespace.into(),
                key: key.into(),
                lease_ttl_ms,
                allow_stale_ms,
            },
        )
        .await
    }

    pub async fn complete_lease(
        &mut self,
        namespace: impl Into<String>,
        key: impl Into<Vec<u8>>,
        lease_token: impl Into<String>,
        metadata: Metadata,
        value: impl Into<Vec<u8>>,
    ) -> Result<u32, ClientError> {
        match self
            .request(
                Command::LeaseComplete,
                RequestPayload::LeaseComplete {
                    namespace: namespace.into(),
                    key: key.into(),
                    lease_token: lease_token.into(),
                    metadata,
                    value: value.into(),
                },
            )
            .await?
        {
            ResponsePayload::Stored { evicted } => Ok(evicted),
            _ => Err(ClientError::UnexpectedResponse(
                "lease completion did not return stored",
            )),
        }
    }

    pub async fn request(
        &mut self,
        command: Command,
        payload: RequestPayload,
    ) -> Result<ResponsePayload, ClientError> {
        let request_id = self.next_request_id;
        self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
        let request = RequestFrame {
            request_id,
            command,
            payload,
        };
        encode_request_frame_into(&request, &mut self.request_buffer);
        self.write_all_request_buffer().await?;

        let mut header = [0u8; HEADER_LEN];
        self.read_exact(&mut header).await?;
        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("header payload length")) as usize;
        self.response_buffer.clear();
        self.response_buffer.extend_from_slice(&header);
        let start = self.response_buffer.len();
        self.response_buffer.resize(HEADER_LEN + payload_len, 0);
        self.read_response_payload(start).await?;

        let response = decode_response_frame(&self.response_buffer, self.max_payload_len)?;
        if response.request_id != request_id || response.command != command {
            return Err(ClientError::UnexpectedResponse("response header mismatch"));
        }
        match response.payload {
            ResponsePayload::Error { code, message } => Err(ClientError::Server { code, message }),
            payload => Ok(payload),
        }
    }

    async fn write_all_request_buffer(&mut self) -> std::io::Result<()> {
        match &mut self.stream {
            NativeStream::Tcp(stream) => stream.write_all(&self.request_buffer).await,
            #[cfg(unix)]
            NativeStream::Unix(stream) => stream.write_all(&self.request_buffer).await,
        }
    }

    async fn read_exact(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        match &mut self.stream {
            NativeStream::Tcp(stream) => stream.read_exact(bytes).await,
            #[cfg(unix)]
            NativeStream::Unix(stream) => stream.read_exact(bytes).await,
        }
    }

    async fn read_response_payload(&mut self, start: usize) -> std::io::Result<usize> {
        match &mut self.stream {
            NativeStream::Tcp(stream) => {
                stream.read_exact(&mut self.response_buffer[start..]).await
            }
            #[cfg(unix)]
            NativeStream::Unix(stream) => {
                stream.read_exact(&mut self.response_buffer[start..]).await
            }
        }
    }
}
