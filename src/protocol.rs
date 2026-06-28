//! Native socket protocol frame codec.
//!
//! This module is transport-independent: it turns byte buffers into native
//! cache requests and native responses into byte buffers. TCP and Unix socket
//! listeners are intentionally outside this module.

use crate::api::{ContentType, Ttl};

pub const MAGIC: [u8; 4] = *b"CBX1";
pub const VERSION: u8 = 1;
pub const HEADER_LEN: usize = 24;

const KIND_REQUEST: u8 = 0x00;
const KIND_RESPONSE: u8 = 0x01;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Get,
    Put,
    Delete,
    BatchGet,
    TagInvalidate,
    LeaseStart,
    LeaseComplete,
}

impl Command {
    pub fn id(self) -> u8 {
        match self {
            Self::Get => 0x01,
            Self::Put => 0x02,
            Self::Delete => 0x03,
            Self::BatchGet => 0x04,
            Self::TagInvalidate => 0x05,
            Self::LeaseStart => 0x06,
            Self::LeaseComplete => 0x07,
        }
    }

    pub fn from_id(id: u8) -> Result<Self, DecodeError> {
        match id {
            0x01 => Ok(Self::Get),
            0x02 => Ok(Self::Put),
            0x03 => Ok(Self::Delete),
            0x04 => Ok(Self::BatchGet),
            0x05 => Ok(Self::TagInvalidate),
            0x06 => Ok(Self::LeaseStart),
            0x07 => Ok(Self::LeaseComplete),
            _ => Err(DecodeError::UnknownCommand(id)),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestFrame {
    pub request_id: u64,
    pub command: Command,
    pub payload: RequestPayload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RequestFrameView<'a> {
    pub request_id: u64,
    pub command: Command,
    pub payload: RequestPayloadView<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RequestPayload {
    Get {
        namespace: String,
        key: Vec<u8>,
    },
    Put {
        namespace: String,
        key: Vec<u8>,
        metadata: Metadata,
        value: Vec<u8>,
    },
    Delete {
        namespace: String,
        key: Vec<u8>,
    },
    BatchGet {
        namespace: String,
        keys: Vec<Vec<u8>>,
    },
    TagInvalidate {
        namespace: String,
        tag: String,
    },
    LeaseStart {
        namespace: String,
        key: Vec<u8>,
        lease_ttl_ms: u64,
        allow_stale_ms: Option<u64>,
    },
    LeaseComplete {
        namespace: String,
        key: Vec<u8>,
        lease_token: String,
        metadata: Metadata,
        value: Vec<u8>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestPayloadView<'a> {
    Get {
        namespace: &'a str,
        key: &'a [u8],
    },
    Delete {
        namespace: &'a str,
        key: &'a [u8],
    },
    TagInvalidate {
        namespace: &'a str,
        tag: &'a str,
    },
    LeaseStart {
        namespace: &'a str,
        key: &'a [u8],
        lease_ttl_ms: u64,
        allow_stale_ms: Option<u64>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Metadata {
    pub ttl: Option<Ttl>,
    pub stale_ttl: Option<Ttl>,
    pub cost: Option<u64>,
    pub tags: Vec<String>,
    pub content_type: ContentType,
}

impl Default for Metadata {
    fn default() -> Self {
        Self {
            ttl: None,
            stale_ttl: None,
            cost: None,
            tags: Vec::new(),
            content_type: ContentType::OctetStream,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResponseFrame {
    pub request_id: u64,
    pub command: Command,
    pub payload: ResponsePayload,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ResponsePayload {
    Ok,
    Hit(Vec<u8>),
    Stale(Vec<u8>),
    Miss,
    Stored {
        evicted: u32,
    },
    Deleted {
        removed: bool,
    },
    Invalidated {
        removed: u32,
    },
    LeaseGranted {
        lease_token: String,
        stale_value: Option<Vec<u8>>,
    },
    LeaseDenied,
    Error {
        code: ErrorCode,
        message: String,
    },
    BatchGet {
        items: Vec<BatchItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BatchItem {
    Hit(Vec<u8>),
    Stale(Vec<u8>),
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResponsePayloadView<'a> {
    Hit(&'a [u8]),
    Stale(&'a [u8]),
    Miss,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorCode {
    BadFrame,
    UnsupportedVersion,
    UnknownCommand,
    InvalidNamespace,
    InvalidTag,
    InvalidTtl,
    ValueTooLarge,
    EntryTooLarge,
    InsufficientMemory,
    InvalidLeaseToken,
    FrameTooLarge,
}

impl ErrorCode {
    fn id(self) -> u16 {
        match self {
            Self::BadFrame => 0x0001,
            Self::UnsupportedVersion => 0x0002,
            Self::UnknownCommand => 0x0003,
            Self::InvalidNamespace => 0x0004,
            Self::InvalidTag => 0x0005,
            Self::InvalidTtl => 0x0006,
            Self::ValueTooLarge => 0x0007,
            Self::EntryTooLarge => 0x0008,
            Self::InsufficientMemory => 0x0009,
            Self::InvalidLeaseToken => 0x000a,
            Self::FrameTooLarge => 0x000b,
        }
    }

    fn from_id(id: u16) -> Result<Self, DecodeError> {
        match id {
            0x0001 => Ok(Self::BadFrame),
            0x0002 => Ok(Self::UnsupportedVersion),
            0x0003 => Ok(Self::UnknownCommand),
            0x0004 => Ok(Self::InvalidNamespace),
            0x0005 => Ok(Self::InvalidTag),
            0x0006 => Ok(Self::InvalidTtl),
            0x0007 => Ok(Self::ValueTooLarge),
            0x0008 => Ok(Self::EntryTooLarge),
            0x0009 => Ok(Self::InsufficientMemory),
            0x000a => Ok(Self::InvalidLeaseToken),
            0x000b => Ok(Self::FrameTooLarge),
            _ => Err(DecodeError::InvalidPayload("unknown error code")),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    IncompleteHeader,
    BadMagic,
    UnsupportedVersion(u8),
    InvalidKind(u8),
    UnknownCommand(u8),
    NonzeroFlags(u8),
    NonzeroReserved(u32),
    FrameTooLarge { payload_len: usize, max: usize },
    TruncatedPayload { expected: usize, actual: usize },
    InvalidPayload(&'static str),
    InvalidUtf8,
    InvalidNamespace,
    InvalidTag,
    InvalidBool(u8),
    TrailingBytes(usize),
}

pub fn decode_request_frame(
    input: &[u8],
    max_payload_len: usize,
) -> Result<RequestFrame, DecodeError> {
    let header = decode_header(input, max_payload_len, KIND_REQUEST)?;
    let payload_start = HEADER_LEN;
    let payload_end = payload_start + header.payload_len;
    if input.len() < payload_end {
        return Err(DecodeError::TruncatedPayload {
            expected: payload_end,
            actual: input.len(),
        });
    }

    let command = Command::from_id(header.command)?;
    let mut cursor = Cursor::new(&input[payload_start..payload_end]);
    let payload = decode_request_payload(command, &mut cursor)?;
    cursor.expect_empty()?;
    Ok(RequestFrame {
        request_id: header.request_id,
        command,
        payload,
    })
}

pub fn decode_borrowed_request_frame(
    input: &[u8],
    max_payload_len: usize,
) -> Result<Option<RequestFrameView<'_>>, DecodeError> {
    let header = decode_header(input, max_payload_len, KIND_REQUEST)?;
    let payload_start = HEADER_LEN;
    let payload_end = payload_start + header.payload_len;
    if input.len() < payload_end {
        return Err(DecodeError::TruncatedPayload {
            expected: payload_end,
            actual: input.len(),
        });
    }

    let command = Command::from_id(header.command)?;
    let mut cursor = Cursor::new(&input[payload_start..payload_end]);
    let payload = match command {
        Command::Get => Some(RequestPayloadView::Get {
            namespace: read_borrowed_namespace(&mut cursor)?,
            key: cursor.read_borrowed_bytes()?,
        }),
        Command::Delete => Some(RequestPayloadView::Delete {
            namespace: read_borrowed_namespace(&mut cursor)?,
            key: cursor.read_borrowed_bytes()?,
        }),
        Command::TagInvalidate => Some(RequestPayloadView::TagInvalidate {
            namespace: read_borrowed_namespace(&mut cursor)?,
            tag: read_borrowed_tag(&mut cursor)?,
        }),
        Command::LeaseStart => {
            let namespace = read_borrowed_namespace(&mut cursor)?;
            let key = cursor.read_borrowed_bytes()?;
            let lease_ttl_ms = cursor.read_u64()?;
            if lease_ttl_ms == 0 {
                return Err(DecodeError::InvalidPayload(
                    "lease ttl must be greater than zero",
                ));
            }
            let allow_stale_ms = match cursor.read_u64()? {
                0 => None,
                value => Some(value),
            };
            Some(RequestPayloadView::LeaseStart {
                namespace,
                key,
                lease_ttl_ms,
                allow_stale_ms,
            })
        }
        Command::Put | Command::BatchGet | Command::LeaseComplete => None,
    };
    if let Some(payload) = payload {
        cursor.expect_empty()?;
        Ok(Some(RequestFrameView {
            request_id: header.request_id,
            command,
            payload,
        }))
    } else {
        Ok(None)
    }
}

pub fn encode_request_frame(frame: &RequestFrame) -> Vec<u8> {
    let mut out = Vec::new();
    encode_request_frame_into(frame, &mut out);
    out
}

pub fn encode_request_frame_into(frame: &RequestFrame, out: &mut Vec<u8>) {
    encode_frame_into(
        KIND_REQUEST,
        frame.command,
        frame.request_id,
        &frame.payload,
        encode_request_payload,
        out,
    );
}

pub fn decode_response_frame(
    input: &[u8],
    max_payload_len: usize,
) -> Result<ResponseFrame, DecodeError> {
    let header = decode_header(input, max_payload_len, KIND_RESPONSE)?;
    let payload_start = HEADER_LEN;
    let payload_end = payload_start + header.payload_len;
    if input.len() < payload_end {
        return Err(DecodeError::TruncatedPayload {
            expected: payload_end,
            actual: input.len(),
        });
    }

    let command = Command::from_id(header.command)?;
    let mut cursor = Cursor::new(&input[payload_start..payload_end]);
    let payload = decode_response_payload(command, &mut cursor)?;
    cursor.expect_empty()?;
    Ok(ResponseFrame {
        request_id: header.request_id,
        command,
        payload,
    })
}

pub fn encode_response_frame(frame: &ResponseFrame) -> Vec<u8> {
    let mut out = Vec::new();
    encode_response_frame_into(frame, &mut out);
    out
}

pub fn encode_response_frame_into(frame: &ResponseFrame, out: &mut Vec<u8>) {
    encode_frame_into(
        KIND_RESPONSE,
        frame.command,
        frame.request_id,
        &frame.payload,
        encode_response_payload,
        out,
    );
}

pub fn encode_response_payload_view_frame_into(
    request_id: u64,
    command: Command,
    payload: ResponsePayloadView<'_>,
    out: &mut Vec<u8>,
) {
    encode_frame_into(
        KIND_RESPONSE,
        command,
        request_id,
        &payload,
        encode_response_payload_view,
        out,
    );
}

fn decode_response_payload(
    command: Command,
    cursor: &mut Cursor<'_>,
) -> Result<ResponsePayload, DecodeError> {
    let status = cursor.read_u8()?;
    match status {
        0x00 if command == Command::BatchGet => {
            let item_count = cursor.read_u32()? as usize;
            let mut items = Vec::with_capacity(item_count);
            for _ in 0..item_count {
                items.push(match cursor.read_u8()? {
                    0x01 => BatchItem::Hit(cursor.read_bytes()?),
                    0x02 => BatchItem::Stale(cursor.read_bytes()?),
                    0x03 => BatchItem::Miss,
                    _ => return Err(DecodeError::InvalidPayload("invalid batch item status")),
                });
            }
            Ok(ResponsePayload::BatchGet { items })
        }
        0x00 => Ok(ResponsePayload::Ok),
        0x01 => Ok(ResponsePayload::Hit(cursor.read_bytes()?)),
        0x02 => Ok(ResponsePayload::Stale(cursor.read_bytes()?)),
        0x03 => Ok(ResponsePayload::Miss),
        0x04 => Ok(ResponsePayload::Stored {
            evicted: cursor.read_u32()?,
        }),
        0x05 => Ok(ResponsePayload::Deleted {
            removed: cursor.read_bool()?,
        }),
        0x06 => Ok(ResponsePayload::Invalidated {
            removed: cursor.read_u32()?,
        }),
        0x07 => {
            let lease_token = cursor.read_string()?;
            let stale_value = if cursor.read_bool()? {
                Some(cursor.read_bytes()?)
            } else {
                None
            };
            Ok(ResponsePayload::LeaseGranted {
                lease_token,
                stale_value,
            })
        }
        0x08 => Ok(ResponsePayload::LeaseDenied),
        0xff => Ok(ResponsePayload::Error {
            code: ErrorCode::from_id(cursor.read_u16()?)?,
            message: cursor.read_string()?,
        }),
        _ => Err(DecodeError::InvalidPayload("unknown response status")),
    }
}

fn encode_frame_into<T>(
    kind: u8,
    command: Command,
    request_id: u64,
    payload: &T,
    encode_payload: fn(&T, &mut Vec<u8>),
    out: &mut Vec<u8>,
) {
    out.clear();
    out.extend_from_slice(&MAGIC);
    out.push(VERSION);
    out.push(kind);
    out.push(command.id());
    out.push(0);
    out.extend_from_slice(&request_id.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    out.extend_from_slice(&0u32.to_be_bytes());
    let payload_start = out.len();
    encode_payload(payload, out);
    let payload_len = out.len() - payload_start;
    assert!(
        u32::try_from(payload_len).is_ok(),
        "native payload too large"
    );
    out[16..20].copy_from_slice(&(payload_len as u32).to_be_bytes());
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Header {
    command: u8,
    request_id: u64,
    payload_len: usize,
}

fn decode_header(
    input: &[u8],
    max_payload_len: usize,
    expected_kind: u8,
) -> Result<Header, DecodeError> {
    if input.len() < HEADER_LEN {
        return Err(DecodeError::IncompleteHeader);
    }
    if input[0..4] != MAGIC {
        return Err(DecodeError::BadMagic);
    }
    let version = input[4];
    if version != VERSION {
        return Err(DecodeError::UnsupportedVersion(version));
    }
    let kind = input[5];
    if kind != expected_kind {
        return Err(DecodeError::InvalidKind(kind));
    }
    let command = input[6];
    let flags = input[7];
    if flags != 0 {
        return Err(DecodeError::NonzeroFlags(flags));
    }
    let request_id = u64::from_be_bytes(input[8..16].try_into().expect("slice length"));
    let payload_len = u32::from_be_bytes(input[16..20].try_into().expect("slice length")) as usize;
    if payload_len > max_payload_len {
        return Err(DecodeError::FrameTooLarge {
            payload_len,
            max: max_payload_len,
        });
    }
    let reserved = u32::from_be_bytes(input[20..24].try_into().expect("slice length"));
    if reserved != 0 {
        return Err(DecodeError::NonzeroReserved(reserved));
    }
    Ok(Header {
        command,
        request_id,
        payload_len,
    })
}

fn decode_request_payload(
    command: Command,
    cursor: &mut Cursor<'_>,
) -> Result<RequestPayload, DecodeError> {
    match command {
        Command::Get => Ok(RequestPayload::Get {
            namespace: read_namespace(cursor)?,
            key: cursor.read_bytes()?,
        }),
        Command::Put => Ok(RequestPayload::Put {
            namespace: read_namespace(cursor)?,
            key: cursor.read_bytes()?,
            metadata: read_metadata(cursor)?,
            value: cursor.read_bytes()?,
        }),
        Command::Delete => Ok(RequestPayload::Delete {
            namespace: read_namespace(cursor)?,
            key: cursor.read_bytes()?,
        }),
        Command::BatchGet => {
            let namespace = read_namespace(cursor)?;
            let key_count = cursor.read_u32()? as usize;
            if key_count == 0 {
                return Err(DecodeError::InvalidPayload(
                    "batch get requires at least one key",
                ));
            }
            let mut keys = Vec::with_capacity(key_count);
            for _ in 0..key_count {
                keys.push(cursor.read_bytes()?);
            }
            Ok(RequestPayload::BatchGet { namespace, keys })
        }
        Command::TagInvalidate => Ok(RequestPayload::TagInvalidate {
            namespace: read_namespace(cursor)?,
            tag: read_tag(cursor)?,
        }),
        Command::LeaseStart => {
            let namespace = read_namespace(cursor)?;
            let key = cursor.read_bytes()?;
            let lease_ttl_ms = cursor.read_u64()?;
            if lease_ttl_ms == 0 {
                return Err(DecodeError::InvalidPayload(
                    "lease ttl must be greater than zero",
                ));
            }
            let allow_stale_ms = match cursor.read_u64()? {
                0 => None,
                value => Some(value),
            };
            Ok(RequestPayload::LeaseStart {
                namespace,
                key,
                lease_ttl_ms,
                allow_stale_ms,
            })
        }
        Command::LeaseComplete => {
            let namespace = read_namespace(cursor)?;
            let key = cursor.read_bytes()?;
            let lease_token = cursor.read_string()?;
            if lease_token.is_empty() {
                return Err(DecodeError::InvalidPayload("lease token must be non-empty"));
            }
            Ok(RequestPayload::LeaseComplete {
                namespace,
                key,
                lease_token,
                metadata: read_metadata(cursor)?,
                value: cursor.read_bytes()?,
            })
        }
    }
}

fn encode_request_payload(payload: &RequestPayload, out: &mut Vec<u8>) {
    match payload {
        RequestPayload::Get { namespace, key } | RequestPayload::Delete { namespace, key } => {
            write_string(out, namespace);
            write_bytes(out, key);
        }
        RequestPayload::Put {
            namespace,
            key,
            metadata,
            value,
        } => {
            write_string(out, namespace);
            write_bytes(out, key);
            write_metadata(out, metadata);
            write_bytes(out, value);
        }
        RequestPayload::BatchGet { namespace, keys } => {
            write_string(out, namespace);
            write_u32(out, keys.len() as u32);
            for key in keys {
                write_bytes(out, key);
            }
        }
        RequestPayload::TagInvalidate { namespace, tag } => {
            write_string(out, namespace);
            write_string(out, tag);
        }
        RequestPayload::LeaseStart {
            namespace,
            key,
            lease_ttl_ms,
            allow_stale_ms,
        } => {
            write_string(out, namespace);
            write_bytes(out, key);
            write_u64(out, *lease_ttl_ms);
            write_u64(out, allow_stale_ms.unwrap_or(0));
        }
        RequestPayload::LeaseComplete {
            namespace,
            key,
            lease_token,
            metadata,
            value,
        } => {
            write_string(out, namespace);
            write_bytes(out, key);
            write_string(out, lease_token);
            write_metadata(out, metadata);
            write_bytes(out, value);
        }
    }
}

fn encode_response_payload(payload: &ResponsePayload, out: &mut Vec<u8>) {
    match payload {
        ResponsePayload::Ok => out.push(0x00),
        ResponsePayload::Hit(value) => {
            out.push(0x01);
            write_bytes(out, value);
        }
        ResponsePayload::Stale(value) => {
            out.push(0x02);
            write_bytes(out, value);
        }
        ResponsePayload::Miss => out.push(0x03),
        ResponsePayload::Stored { evicted } => {
            out.push(0x04);
            write_u32(out, *evicted);
        }
        ResponsePayload::Deleted { removed } => {
            out.push(0x05);
            write_bool(out, *removed);
        }
        ResponsePayload::Invalidated { removed } => {
            out.push(0x06);
            write_u32(out, *removed);
        }
        ResponsePayload::LeaseGranted {
            lease_token,
            stale_value,
        } => {
            out.push(0x07);
            write_string(out, lease_token);
            write_bool(out, stale_value.is_some());
            if let Some(value) = stale_value {
                write_bytes(out, value);
            }
        }
        ResponsePayload::LeaseDenied => out.push(0x08),
        ResponsePayload::Error { code, message } => {
            out.push(0xff);
            write_u16(out, code.id());
            write_string(out, message);
        }
        ResponsePayload::BatchGet { items } => {
            out.push(0x00);
            write_u32(out, items.len() as u32);
            for item in items {
                match item {
                    BatchItem::Hit(value) => {
                        out.push(0x01);
                        write_bytes(out, value);
                    }
                    BatchItem::Stale(value) => {
                        out.push(0x02);
                        write_bytes(out, value);
                    }
                    BatchItem::Miss => out.push(0x03),
                }
            }
        }
    }
}

fn encode_response_payload_view(payload: &ResponsePayloadView<'_>, out: &mut Vec<u8>) {
    match payload {
        ResponsePayloadView::Hit(value) => {
            out.push(0x01);
            write_bytes(out, value);
        }
        ResponsePayloadView::Stale(value) => {
            out.push(0x02);
            write_bytes(out, value);
        }
        ResponsePayloadView::Miss => out.push(0x03),
    }
}

fn read_metadata(cursor: &mut Cursor<'_>) -> Result<Metadata, DecodeError> {
    let ttl = match cursor.read_u64()? {
        0 => None,
        milliseconds => Some(Ttl { milliseconds }),
    };
    let stale_ttl = match cursor.read_u64()? {
        0 => None,
        milliseconds => Some(Ttl { milliseconds }),
    };
    let cost = if cursor.read_bool()? {
        Some(cursor.read_u64()?)
    } else {
        let _ignored_cost = cursor.read_u64()?;
        None
    };
    let tag_count = cursor.read_u16()? as usize;
    let mut tags = Vec::with_capacity(tag_count);
    for _ in 0..tag_count {
        tags.push(read_tag(cursor)?);
    }
    let content_type = match cursor.read_u8()? {
        0 => ContentType::OctetStream,
        1 => ContentType::Other,
        _ => return Err(DecodeError::InvalidPayload("invalid content type")),
    };
    Ok(Metadata {
        ttl,
        stale_ttl,
        cost,
        tags,
        content_type,
    })
}

fn write_metadata(out: &mut Vec<u8>, metadata: &Metadata) {
    write_u64(out, metadata.ttl.map_or(0, |ttl| ttl.milliseconds));
    write_u64(out, metadata.stale_ttl.map_or(0, |ttl| ttl.milliseconds));
    write_bool(out, metadata.cost.is_some());
    write_u64(out, metadata.cost.unwrap_or(0));
    write_u16(out, metadata.tags.len() as u16);
    for tag in &metadata.tags {
        write_string(out, tag);
    }
    out.push(match metadata.content_type {
        ContentType::OctetStream => 0,
        ContentType::Other => 1,
    });
}

fn read_namespace(cursor: &mut Cursor<'_>) -> Result<String, DecodeError> {
    let namespace = cursor.read_string()?;
    if !is_valid_namespace(&namespace) {
        return Err(DecodeError::InvalidNamespace);
    }
    Ok(namespace)
}

fn read_tag(cursor: &mut Cursor<'_>) -> Result<String, DecodeError> {
    let tag = cursor.read_string()?;
    if !is_valid_tag(&tag) {
        return Err(DecodeError::InvalidTag);
    }
    Ok(tag)
}

fn read_borrowed_namespace<'a>(cursor: &mut Cursor<'a>) -> Result<&'a str, DecodeError> {
    let namespace = cursor.read_borrowed_string()?;
    if !is_valid_namespace(namespace) {
        return Err(DecodeError::InvalidNamespace);
    }
    Ok(namespace)
}

fn read_borrowed_tag<'a>(cursor: &mut Cursor<'a>) -> Result<&'a str, DecodeError> {
    let tag = cursor.read_borrowed_string()?;
    if !is_valid_tag(tag) {
        return Err(DecodeError::InvalidTag);
    }
    Ok(tag)
}

fn is_valid_namespace(namespace: &str) -> bool {
    !namespace.is_empty()
        && namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

fn is_valid_tag(tag: &str) -> bool {
    !tag.is_empty()
        && tag.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.' | b'/')
        })
}

struct Cursor<'a> {
    input: &'a [u8],
    offset: usize,
}

impl<'a> Cursor<'a> {
    fn new(input: &'a [u8]) -> Self {
        Self { input, offset: 0 }
    }

    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let byte = *self
            .input
            .get(self.offset)
            .ok_or(DecodeError::InvalidPayload("truncated u8"))?;
        self.offset += 1;
        Ok(byte)
    }

    fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let bytes = self.read_exact(2)?;
        Ok(u16::from_be_bytes(bytes.try_into().expect("slice length")))
    }

    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let bytes = self.read_exact(4)?;
        Ok(u32::from_be_bytes(bytes.try_into().expect("slice length")))
    }

    fn read_u64(&mut self) -> Result<u64, DecodeError> {
        let bytes = self.read_exact(8)?;
        Ok(u64::from_be_bytes(bytes.try_into().expect("slice length")))
    }

    fn read_bool(&mut self) -> Result<bool, DecodeError> {
        match self.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            value => Err(DecodeError::InvalidBool(value)),
        }
    }

    fn read_bytes(&mut self) -> Result<Vec<u8>, DecodeError> {
        let len = self.read_u32()? as usize;
        Ok(self.read_exact(len)?.to_vec())
    }

    fn read_string(&mut self) -> Result<String, DecodeError> {
        String::from_utf8(self.read_bytes()?).map_err(|_| DecodeError::InvalidUtf8)
    }

    fn read_borrowed_bytes(&mut self) -> Result<&'a [u8], DecodeError> {
        let len = self.read_u32()? as usize;
        self.read_exact(len)
    }

    fn read_borrowed_string(&mut self) -> Result<&'a str, DecodeError> {
        std::str::from_utf8(self.read_borrowed_bytes()?).map_err(|_| DecodeError::InvalidUtf8)
    }

    fn read_exact(&mut self, len: usize) -> Result<&'a [u8], DecodeError> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or(DecodeError::InvalidPayload("length overflow"))?;
        if end > self.input.len() {
            return Err(DecodeError::InvalidPayload("truncated field"));
        }
        let bytes = &self.input[self.offset..end];
        self.offset = end;
        Ok(bytes)
    }

    fn expect_empty(&self) -> Result<(), DecodeError> {
        if self.offset == self.input.len() {
            Ok(())
        } else {
            Err(DecodeError::TrailingBytes(self.input.len() - self.offset))
        }
    }
}

fn write_u16(out: &mut Vec<u8>, value: u16) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_u32(out: &mut Vec<u8>, value: u32) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_u64(out: &mut Vec<u8>, value: u64) {
    out.extend_from_slice(&value.to_be_bytes());
}

fn write_bool(out: &mut Vec<u8>, value: bool) {
    out.push(u8::from(value));
}

fn write_bytes(out: &mut Vec<u8>, value: &[u8]) {
    write_u32(out, value.len() as u32);
    out.extend_from_slice(value);
}

fn write_string(out: &mut Vec<u8>, value: &str) {
    write_bytes(out, value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    const MAX_PAYLOAD: usize = 1024;

    fn round_trip(payload: RequestPayload) -> RequestFrame {
        let command = command_for_payload(&payload);
        let frame = RequestFrame {
            request_id: 42,
            command,
            payload,
        };
        let encoded = encode_request_frame(&frame);
        decode_request_frame(&encoded, MAX_PAYLOAD).expect("frame should decode")
    }

    fn command_for_payload(payload: &RequestPayload) -> Command {
        match payload {
            RequestPayload::Get { .. } => Command::Get,
            RequestPayload::Put { .. } => Command::Put,
            RequestPayload::Delete { .. } => Command::Delete,
            RequestPayload::BatchGet { .. } => Command::BatchGet,
            RequestPayload::TagInvalidate { .. } => Command::TagInvalidate,
            RequestPayload::LeaseStart { .. } => Command::LeaseStart,
            RequestPayload::LeaseComplete { .. } => Command::LeaseComplete,
        }
    }

    fn sample_metadata() -> Metadata {
        Metadata {
            ttl: Some(Ttl { milliseconds: 1000 }),
            stale_ttl: Some(Ttl { milliseconds: 5000 }),
            cost: Some(99),
            tags: vec!["model:gpt".to_string(), "tenant/a".to_string()],
            content_type: ContentType::Other,
        }
    }

    #[test]
    fn decodes_get_request() {
        let decoded = round_trip(RequestPayload::Get {
            namespace: "default".to_string(),
            key: b"user:1".to_vec(),
        });

        assert_eq!(decoded.request_id, 42);
        assert_eq!(decoded.command, Command::Get);
        assert_eq!(
            decoded.payload,
            RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"user:1".to_vec()
            }
        );
    }

    #[test]
    fn decodes_hot_request_views_without_owning_payload_fields() {
        let frame = encode_request_frame(&RequestFrame {
            request_id: 7,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"user:1".to_vec(),
            },
        });
        let view = decode_borrowed_request_frame(&frame, MAX_PAYLOAD)
            .expect("borrowed decode")
            .expect("hot request view");

        assert_eq!(view.request_id, 7);
        assert_eq!(view.command, Command::Get);
        let RequestPayloadView::Get { namespace, key } = view.payload else {
            panic!("expected get view");
        };
        assert_eq!(namespace, "default");
        assert_eq!(key, b"user:1");

        let frame_start = frame.as_ptr() as usize;
        let frame_end = frame_start + frame.len();
        let namespace_ptr = namespace.as_ptr() as usize;
        let key_ptr = key.as_ptr() as usize;
        assert!((frame_start..frame_end).contains(&namespace_ptr));
        assert!((frame_start..frame_end).contains(&key_ptr));
    }

    #[test]
    fn borrowed_decode_skips_owned_only_request_shapes() {
        let frame = encode_request_frame(&RequestFrame {
            request_id: 8,
            command: Command::Put,
            payload: RequestPayload::Put {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                metadata: Metadata::default(),
                value: b"value".to_vec(),
            },
        });

        assert_eq!(
            decode_borrowed_request_frame(&frame, MAX_PAYLOAD).expect("borrowed decode"),
            None
        );
    }

    #[test]
    fn borrowed_decode_validates_hot_request_fields() {
        let invalid_namespace = encode_request_frame(&RequestFrame {
            request_id: 9,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "bad namespace".to_string(),
                key: b"k".to_vec(),
            },
        });

        assert_eq!(
            decode_borrowed_request_frame(&invalid_namespace, MAX_PAYLOAD),
            Err(DecodeError::InvalidNamespace)
        );

        let invalid_tag = encode_request_frame(&RequestFrame {
            request_id: 10,
            command: Command::TagInvalidate,
            payload: RequestPayload::TagInvalidate {
                namespace: "default".to_string(),
                tag: "bad tag".to_string(),
            },
        });

        assert_eq!(
            decode_borrowed_request_frame(&invalid_tag, MAX_PAYLOAD),
            Err(DecodeError::InvalidTag)
        );
    }

    #[test]
    fn decodes_all_request_payload_shapes() {
        let requests = [
            RequestPayload::Put {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                metadata: sample_metadata(),
                value: b"value".to_vec(),
            },
            RequestPayload::Delete {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
            },
            RequestPayload::BatchGet {
                namespace: "default".to_string(),
                keys: vec![b"a".to_vec(), b"b".to_vec()],
            },
            RequestPayload::TagInvalidate {
                namespace: "default".to_string(),
                tag: "tenant/a".to_string(),
            },
            RequestPayload::LeaseStart {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_ttl_ms: 1000,
                allow_stale_ms: Some(5000),
            },
            RequestPayload::LeaseComplete {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_token: "lease-1".to_string(),
                metadata: sample_metadata(),
                value: b"value".to_vec(),
            },
        ];

        for request in requests {
            assert_eq!(round_trip(request.clone()).payload, request);
        }
    }

    #[test]
    fn encodes_response_frame() {
        let encoded = encode_response_frame(&ResponseFrame {
            request_id: 7,
            command: Command::Get,
            payload: ResponsePayload::Hit(b"bytes".to_vec()),
        });

        assert_eq!(&encoded[0..4], b"CBX1");
        assert_eq!(encoded[4], VERSION);
        assert_eq!(encoded[5], KIND_RESPONSE);
        assert_eq!(encoded[6], Command::Get.id());
        assert_eq!(u64::from_be_bytes(encoded[8..16].try_into().unwrap()), 7);
        assert_eq!(u32::from_be_bytes(encoded[16..20].try_into().unwrap()), 10);
        assert_eq!(encoded[24], 0x01);
        assert_eq!(u32::from_be_bytes(encoded[25..29].try_into().unwrap()), 5);
        assert_eq!(&encoded[29..34], b"bytes");
    }

    #[test]
    fn encodes_borrowed_response_payload_view() {
        let mut buffer = Vec::new();
        encode_response_payload_view_frame_into(
            7,
            Command::Get,
            ResponsePayloadView::Hit(b"bytes"),
            &mut buffer,
        );

        assert_eq!(
            decode_response_frame(&buffer, MAX_PAYLOAD).expect("response frame"),
            ResponseFrame {
                request_id: 7,
                command: Command::Get,
                payload: ResponsePayload::Hit(b"bytes".to_vec()),
            }
        );
    }

    #[test]
    fn encode_into_reuses_and_truncates_existing_buffers() {
        let request = RequestFrame {
            request_id: 11,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
            },
        };
        let mut buffer = vec![0xff; 256];
        let capacity = buffer.capacity();

        encode_request_frame_into(&request, &mut buffer);

        assert!(buffer.len() < 256);
        assert_eq!(buffer.capacity(), capacity);
        assert_eq!(
            decode_request_frame(&buffer, MAX_PAYLOAD).expect("request frame"),
            request
        );

        let response = ResponseFrame {
            request_id: 11,
            command: Command::Get,
            payload: ResponsePayload::Miss,
        };
        encode_response_frame_into(&response, &mut buffer);

        assert!(buffer.len() < 256);
        assert_eq!(buffer.capacity(), capacity);
        assert_eq!(
            decode_response_frame(&buffer, MAX_PAYLOAD).expect("response frame"),
            response
        );
    }

    #[test]
    fn rejects_bad_header_fields() {
        let frame = encode_request_frame(&RequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
            },
        });

        let mut bad_magic = frame.clone();
        bad_magic[0] = b'X';
        assert_eq!(
            decode_request_frame(&bad_magic, MAX_PAYLOAD),
            Err(DecodeError::BadMagic)
        );

        let mut bad_version = frame.clone();
        bad_version[4] = 2;
        assert_eq!(
            decode_request_frame(&bad_version, MAX_PAYLOAD),
            Err(DecodeError::UnsupportedVersion(2))
        );

        let mut response_kind = frame.clone();
        response_kind[5] = KIND_RESPONSE;
        assert_eq!(
            decode_request_frame(&response_kind, MAX_PAYLOAD),
            Err(DecodeError::InvalidKind(KIND_RESPONSE))
        );

        let mut bad_flags = frame.clone();
        bad_flags[7] = 1;
        assert_eq!(
            decode_request_frame(&bad_flags, MAX_PAYLOAD),
            Err(DecodeError::NonzeroFlags(1))
        );
    }

    #[test]
    fn rejects_unknown_command_and_oversized_frame() {
        let mut frame = encode_request_frame(&RequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
            },
        });
        frame[6] = 0x7f;
        assert_eq!(
            decode_request_frame(&frame, MAX_PAYLOAD),
            Err(DecodeError::UnknownCommand(0x7f))
        );

        let mut oversized = frame;
        oversized[6] = Command::Get.id();
        oversized[16..20].copy_from_slice(&2048u32.to_be_bytes());
        assert_eq!(
            decode_request_frame(&oversized, MAX_PAYLOAD),
            Err(DecodeError::FrameTooLarge {
                payload_len: 2048,
                max: MAX_PAYLOAD
            })
        );
    }

    #[test]
    fn rejects_truncated_and_trailing_payloads() {
        let frame = encode_request_frame(&RequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
            },
        });
        let truncated = &frame[..frame.len() - 1];
        assert!(matches!(
            decode_request_frame(truncated, MAX_PAYLOAD),
            Err(DecodeError::TruncatedPayload { .. })
        ));

        let mut trailing = frame;
        let payload_len = u32::from_be_bytes(trailing[16..20].try_into().unwrap()) + 1;
        trailing[16..20].copy_from_slice(&payload_len.to_be_bytes());
        trailing.push(0);
        assert_eq!(
            decode_request_frame(&trailing, MAX_PAYLOAD),
            Err(DecodeError::TrailingBytes(1))
        );
    }

    #[test]
    fn rejects_invalid_payload_values() {
        assert_eq!(
            round_trip_err(RequestPayload::BatchGet {
                namespace: "default".to_string(),
                keys: Vec::new(),
            }),
            DecodeError::InvalidPayload("batch get requires at least one key")
        );
        assert_eq!(
            round_trip_err(RequestPayload::LeaseStart {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_ttl_ms: 0,
                allow_stale_ms: None,
            }),
            DecodeError::InvalidPayload("lease ttl must be greater than zero")
        );
        assert_eq!(
            round_trip_err(RequestPayload::LeaseComplete {
                namespace: "default".to_string(),
                key: b"k".to_vec(),
                lease_token: String::new(),
                metadata: Metadata::default(),
                value: b"value".to_vec(),
            }),
            DecodeError::InvalidPayload("lease token must be non-empty")
        );
    }

    fn round_trip_err(payload: RequestPayload) -> DecodeError {
        let command = command_for_payload(&payload);
        let frame = RequestFrame {
            request_id: 1,
            command,
            payload,
        };
        let encoded = encode_request_frame(&frame);
        decode_request_frame(&encoded, MAX_PAYLOAD).expect_err("frame should fail")
    }

    #[test]
    fn rejects_invalid_namespace_and_tag() {
        assert_eq!(
            round_trip_err(RequestPayload::Get {
                namespace: "bad namespace".to_string(),
                key: b"k".to_vec(),
            }),
            DecodeError::InvalidNamespace
        );
        assert_eq!(
            round_trip_err(RequestPayload::TagInvalidate {
                namespace: "default".to_string(),
                tag: "bad tag!".to_string(),
            }),
            DecodeError::InvalidTag
        );
    }
}
