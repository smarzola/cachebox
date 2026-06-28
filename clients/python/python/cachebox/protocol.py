"""Cachebox native protocol v1 codec.

This module is intentionally transport-independent. It turns Python request and
response objects into native protocol frames and decodes native protocol frames
back into typed Python objects.
"""

from __future__ import annotations

from dataclasses import dataclass
from enum import IntEnum
from typing import Final

MAGIC: Final = b"CBX1"
VERSION: Final = 1
HEADER_LEN: Final = 24
KIND_REQUEST: Final = 0x00
KIND_RESPONSE: Final = 0x01
NAMESPACE_CHARS: Final = frozenset("-_")
TAG_CHARS: Final = frozenset("-_:/.")


class Command(IntEnum):
    GET = 0x01
    PUT = 0x02
    DELETE = 0x03
    BATCH_GET = 0x04
    TAG_INVALIDATE = 0x05
    LEASE_START = 0x06
    LEASE_COMPLETE = 0x07


class ContentType(IntEnum):
    OCTET_STREAM = 0x00
    OTHER = 0x01


class ErrorCode(IntEnum):
    BAD_FRAME = 0x0001
    UNSUPPORTED_VERSION = 0x0002
    UNKNOWN_COMMAND = 0x0003
    INVALID_NAMESPACE = 0x0004
    INVALID_TAG = 0x0005
    INVALID_TTL = 0x0006
    VALUE_TOO_LARGE = 0x0007
    ENTRY_TOO_LARGE = 0x0008
    INSUFFICIENT_MEMORY = 0x0009
    INVALID_LEASE_TOKEN = 0x000A
    FRAME_TOO_LARGE = 0x000B


class DecodeError(Exception):
    """Base class for native protocol decode errors."""


class IncompleteHeader(DecodeError):
    pass


class BadMagic(DecodeError):
    pass


class UnsupportedVersion(DecodeError):
    pass


class InvalidKind(DecodeError):
    pass


class UnknownCommand(DecodeError):
    pass


class NonzeroFlags(DecodeError):
    pass


class NonzeroReserved(DecodeError):
    pass


class FrameTooLarge(DecodeError):
    pass


class TruncatedPayload(DecodeError):
    pass


class InvalidPayload(DecodeError):
    pass


class InvalidUtf8(DecodeError):
    pass


class InvalidNamespace(DecodeError):
    pass


class InvalidTag(DecodeError):
    pass


class InvalidBool(DecodeError):
    pass


class TrailingBytes(DecodeError):
    pass


@dataclass(frozen=True)
class Ttl:
    milliseconds: int


@dataclass(frozen=True)
class Metadata:
    ttl: Ttl | None = None
    stale_ttl: Ttl | None = None
    cost: int | None = None
    tags: tuple[str, ...] = ()
    content_type: ContentType = ContentType.OCTET_STREAM


@dataclass(frozen=True)
class Get:
    namespace: str
    key: bytes


@dataclass(frozen=True)
class Put:
    namespace: str
    key: bytes
    metadata: Metadata
    value: bytes


@dataclass(frozen=True)
class Delete:
    namespace: str
    key: bytes


@dataclass(frozen=True)
class BatchGet:
    namespace: str
    keys: tuple[bytes, ...]


@dataclass(frozen=True)
class TagInvalidate:
    namespace: str
    tag: str


@dataclass(frozen=True)
class LeaseStart:
    namespace: str
    key: bytes
    lease_ttl_ms: int
    allow_stale_ms: int | None = None


@dataclass(frozen=True)
class LeaseComplete:
    namespace: str
    key: bytes
    lease_token: str
    metadata: Metadata
    value: bytes


RequestPayload = (
    Get | Put | Delete | BatchGet | TagInvalidate | LeaseStart | LeaseComplete
)


@dataclass(frozen=True)
class RequestFrame:
    request_id: int
    command: Command
    payload: RequestPayload


@dataclass(frozen=True)
class Ok:
    pass


@dataclass(frozen=True)
class Hit:
    value: bytes


@dataclass(frozen=True)
class Stale:
    value: bytes


@dataclass(frozen=True)
class Miss:
    pass


@dataclass(frozen=True)
class Stored:
    evicted: int


@dataclass(frozen=True)
class Deleted:
    removed: bool


@dataclass(frozen=True)
class Invalidated:
    removed: int


@dataclass(frozen=True)
class LeaseGranted:
    lease_token: str
    stale_value: bytes | None = None


@dataclass(frozen=True)
class LeaseDenied:
    pass


@dataclass(frozen=True)
class Error:
    code: ErrorCode
    message: str


@dataclass(frozen=True)
class BatchItem:
    state: str
    value: bytes | None = None


@dataclass(frozen=True)
class BatchGetResult:
    items: tuple[BatchItem, ...]


ResponsePayload = (
    Ok
    | Hit
    | Stale
    | Miss
    | Stored
    | Deleted
    | Invalidated
    | LeaseGranted
    | LeaseDenied
    | Error
    | BatchGetResult
)


@dataclass(frozen=True)
class ResponseFrame:
    request_id: int
    command: Command
    payload: ResponsePayload


def encode_request_frame(frame: RequestFrame) -> bytes:
    return _encode_frame(KIND_REQUEST, frame.command, frame.request_id, _encode_request_payload(frame.payload))


def encode_response_frame(frame: ResponseFrame) -> bytes:
    return _encode_frame(KIND_RESPONSE, frame.command, frame.request_id, _encode_response_payload(frame.command, frame.payload))


def decode_request_frame(frame: bytes, max_payload_len: int = 2**32 - 1) -> RequestFrame:
    header = _decode_header(frame, KIND_REQUEST, max_payload_len)
    payload_end = HEADER_LEN + header.payload_len
    if len(frame) < payload_end:
        raise TruncatedPayload(f"expected {payload_end} bytes, got {len(frame)}")
    cursor = _Cursor(frame[HEADER_LEN:payload_end])
    payload = _decode_request_payload(header.command, cursor)
    cursor.expect_empty()
    return RequestFrame(header.request_id, header.command, payload)


def decode_response_frame(frame: bytes, max_payload_len: int = 2**32 - 1) -> ResponseFrame:
    header = _decode_header(frame, KIND_RESPONSE, max_payload_len)
    payload_end = HEADER_LEN + header.payload_len
    if len(frame) < payload_end:
        raise TruncatedPayload(f"expected {payload_end} bytes, got {len(frame)}")
    cursor = _Cursor(frame[HEADER_LEN:payload_end])
    payload = _decode_response_payload(header.command, cursor)
    cursor.expect_empty()
    return ResponseFrame(header.request_id, header.command, payload)


@dataclass(frozen=True)
class _Header:
    command: Command
    request_id: int
    payload_len: int


def _decode_header(frame: bytes, expected_kind: int, max_payload_len: int) -> _Header:
    if len(frame) < HEADER_LEN:
        raise IncompleteHeader("frame header is incomplete")
    if frame[0:4] != MAGIC:
        raise BadMagic("bad magic")
    if frame[4] != VERSION:
        raise UnsupportedVersion(f"unsupported version {frame[4]}")
    if frame[5] != expected_kind:
        raise InvalidKind(f"invalid kind {frame[5]}")
    try:
        command = Command(frame[6])
    except ValueError as error:
        raise UnknownCommand(f"unknown command {frame[6]}") from error
    if frame[7] != 0:
        raise NonzeroFlags(f"nonzero flags {frame[7]}")
    request_id = int.from_bytes(frame[8:16], "big")
    payload_len = int.from_bytes(frame[16:20], "big")
    if payload_len > max_payload_len:
        raise FrameTooLarge(f"payload {payload_len} exceeds max {max_payload_len}")
    reserved = int.from_bytes(frame[20:24], "big")
    if reserved != 0:
        raise NonzeroReserved(f"nonzero reserved {reserved}")
    return _Header(command, request_id, payload_len)


def _encode_frame(kind: int, command: Command, request_id: int, payload: bytes) -> bytes:
    if not 0 <= request_id <= 2**64 - 1:
        raise ValueError("request_id must fit in u64")
    if len(payload) > 2**32 - 1:
        raise ValueError("payload too large")
    return b"".join(
        [
            MAGIC,
            _u8(VERSION),
            _u8(kind),
            _u8(int(command)),
            _u8(0),
            _u64(request_id),
            _u32(len(payload)),
            _u32(0),
            payload,
        ]
    )


def _decode_request_payload(command: Command, cursor: "_Cursor") -> RequestPayload:
    if command == Command.GET:
        return Get(_read_namespace(cursor), cursor.read_bytes())
    if command == Command.PUT:
        return Put(
            _read_namespace(cursor),
            cursor.read_bytes(),
            _read_metadata(cursor),
            cursor.read_bytes(),
        )
    if command == Command.DELETE:
        return Delete(_read_namespace(cursor), cursor.read_bytes())
    if command == Command.BATCH_GET:
        namespace = _read_namespace(cursor)
        key_count = cursor.read_u32()
        if key_count == 0:
            raise InvalidPayload("batch get requires at least one key")
        return BatchGet(namespace, tuple(cursor.read_bytes() for _ in range(key_count)))
    if command == Command.TAG_INVALIDATE:
        return TagInvalidate(_read_namespace(cursor), _read_tag(cursor))
    if command == Command.LEASE_START:
        namespace = _read_namespace(cursor)
        key = cursor.read_bytes()
        lease_ttl_ms = cursor.read_u64()
        if lease_ttl_ms == 0:
            raise InvalidPayload("lease ttl must be greater than zero")
        allow_stale_ms = cursor.read_u64()
        return LeaseStart(namespace, key, lease_ttl_ms, allow_stale_ms or None)
    if command == Command.LEASE_COMPLETE:
        namespace = _read_namespace(cursor)
        key = cursor.read_bytes()
        lease_token = cursor.read_string()
        if not lease_token:
            raise InvalidPayload("lease token must be non-empty")
        return LeaseComplete(
            namespace,
            key,
            lease_token,
            _read_metadata(cursor),
            cursor.read_bytes(),
        )
    raise AssertionError(f"unhandled command {command}")


def _encode_request_payload(payload: RequestPayload) -> bytes:
    if isinstance(payload, Get | Delete):
        return _string(payload.namespace) + _bytes(payload.key)
    if isinstance(payload, Put):
        return (
            _string(payload.namespace)
            + _bytes(payload.key)
            + _metadata(payload.metadata)
            + _bytes(payload.value)
        )
    if isinstance(payload, BatchGet):
        return _string(payload.namespace) + _u32(len(payload.keys)) + b"".join(_bytes(key) for key in payload.keys)
    if isinstance(payload, TagInvalidate):
        return _string(payload.namespace) + _string(payload.tag)
    if isinstance(payload, LeaseStart):
        return (
            _string(payload.namespace)
            + _bytes(payload.key)
            + _u64(payload.lease_ttl_ms)
            + _u64(payload.allow_stale_ms or 0)
        )
    if isinstance(payload, LeaseComplete):
        return (
            _string(payload.namespace)
            + _bytes(payload.key)
            + _string(payload.lease_token)
            + _metadata(payload.metadata)
            + _bytes(payload.value)
        )
    raise TypeError(f"unknown request payload {payload!r}")


def _decode_response_payload(command: Command, cursor: "_Cursor") -> ResponsePayload:
    status = cursor.read_u8()
    if status == 0x00 and command == Command.BATCH_GET:
        items = []
        for _ in range(cursor.read_u32()):
            item_status = cursor.read_u8()
            if item_status == 0x01:
                items.append(BatchItem("hit", cursor.read_bytes()))
            elif item_status == 0x02:
                items.append(BatchItem("stale", cursor.read_bytes()))
            elif item_status == 0x03:
                items.append(BatchItem("miss"))
            else:
                raise InvalidPayload("invalid batch item status")
        return BatchGetResult(tuple(items))
    if status == 0x00:
        return Ok()
    if status == 0x01:
        return Hit(cursor.read_bytes())
    if status == 0x02:
        return Stale(cursor.read_bytes())
    if status == 0x03:
        return Miss()
    if status == 0x04:
        return Stored(cursor.read_u32())
    if status == 0x05:
        return Deleted(cursor.read_bool())
    if status == 0x06:
        return Invalidated(cursor.read_u32())
    if status == 0x07:
        lease_token = cursor.read_string()
        stale_value = cursor.read_bytes() if cursor.read_bool() else None
        return LeaseGranted(lease_token, stale_value)
    if status == 0x08:
        return LeaseDenied()
    if status == 0xFF:
        try:
            code = ErrorCode(cursor.read_u16())
        except ValueError as error:
            raise InvalidPayload("unknown error code") from error
        return Error(code, cursor.read_string())
    if status == 0x00:
        raise InvalidPayload("generic ok response is not used by the client")
    raise InvalidPayload("unknown response status")


def _encode_response_payload(command: Command, payload: ResponsePayload) -> bytes:
    if isinstance(payload, Hit):
        return _u8(0x01) + _bytes(payload.value)
    if isinstance(payload, Stale):
        return _u8(0x02) + _bytes(payload.value)
    if isinstance(payload, Miss):
        return _u8(0x03)
    if isinstance(payload, Stored):
        return _u8(0x04) + _u32(payload.evicted)
    if isinstance(payload, Deleted):
        return _u8(0x05) + _bool(payload.removed)
    if isinstance(payload, Invalidated):
        return _u8(0x06) + _u32(payload.removed)
    if isinstance(payload, LeaseGranted):
        return (
            _u8(0x07)
            + _string(payload.lease_token)
            + _bool(payload.stale_value is not None)
            + (_bytes(payload.stale_value) if payload.stale_value is not None else b"")
        )
    if isinstance(payload, LeaseDenied):
        return _u8(0x08)
    if isinstance(payload, Error):
        return _u8(0xFF) + _u16(int(payload.code)) + _string(payload.message)
    if isinstance(payload, BatchGetResult):
        if command != Command.BATCH_GET:
            raise ValueError("batch response payload requires BatchGet command")
        out = bytearray(_u8(0x00) + _u32(len(payload.items)))
        for item in payload.items:
            if item.state == "hit":
                if item.value is None:
                    raise ValueError("hit batch item requires value")
                out += _u8(0x01) + _bytes(item.value)
            elif item.state == "stale":
                if item.value is None:
                    raise ValueError("stale batch item requires value")
                out += _u8(0x02) + _bytes(item.value)
            elif item.state == "miss":
                out += _u8(0x03)
            else:
                raise ValueError(f"unknown batch item state {item.state!r}")
        return bytes(out)
    if isinstance(payload, Ok):
        return _u8(0x00)
    raise TypeError(f"unknown response payload {payload!r}")


def _read_metadata(cursor: "_Cursor") -> Metadata:
    ttl_ms = cursor.read_u64()
    stale_ttl_ms = cursor.read_u64()
    cost = cursor.read_u64() if cursor.read_bool() else _ignore_u64(cursor)
    tags = tuple(_read_tag(cursor) for _ in range(cursor.read_u16()))
    try:
        content_type = ContentType(cursor.read_u8())
    except ValueError as error:
        raise InvalidPayload("invalid content type") from error
    return Metadata(
        ttl=Ttl(ttl_ms) if ttl_ms else None,
        stale_ttl=Ttl(stale_ttl_ms) if stale_ttl_ms else None,
        cost=cost,
        tags=tags,
        content_type=content_type,
    )


def _metadata(metadata: Metadata) -> bytes:
    out = bytearray()
    out += _u64(metadata.ttl.milliseconds if metadata.ttl else 0)
    out += _u64(metadata.stale_ttl.milliseconds if metadata.stale_ttl else 0)
    out += _bool(metadata.cost is not None)
    out += _u64(metadata.cost or 0)
    out += _u16(len(metadata.tags))
    for tag in metadata.tags:
        out += _string(tag)
    out += _u8(int(metadata.content_type))
    return bytes(out)


def _read_namespace(cursor: "_Cursor") -> str:
    namespace = cursor.read_string()
    if not namespace or not all(
        char.isascii() and (char.isalnum() or char in NAMESPACE_CHARS)
        for char in namespace
    ):
        raise InvalidNamespace(namespace)
    return namespace


def _read_tag(cursor: "_Cursor") -> str:
    tag = cursor.read_string()
    if not tag or not all(
        char.isascii() and (char.isalnum() or char in TAG_CHARS) for char in tag
    ):
        raise InvalidTag(tag)
    return tag


def _ignore_u64(cursor: "_Cursor") -> None:
    cursor.read_u64()
    return None


class _Cursor:
    def __init__(self, data: bytes) -> None:
        self._data = data
        self._offset = 0

    def read_u8(self) -> int:
        return self._read_exact(1)[0]

    def read_u16(self) -> int:
        return int.from_bytes(self._read_exact(2), "big")

    def read_u32(self) -> int:
        return int.from_bytes(self._read_exact(4), "big")

    def read_u64(self) -> int:
        return int.from_bytes(self._read_exact(8), "big")

    def read_bool(self) -> bool:
        value = self.read_u8()
        if value == 0:
            return False
        if value == 1:
            return True
        raise InvalidBool(value)

    def read_bytes(self) -> bytes:
        return self._read_exact(self.read_u32())

    def read_string(self) -> str:
        try:
            return self.read_bytes().decode("utf-8")
        except UnicodeDecodeError as error:
            raise InvalidUtf8("invalid utf-8 string") from error

    def expect_empty(self) -> None:
        remaining = len(self._data) - self._offset
        if remaining:
            raise TrailingBytes(f"{remaining} trailing bytes")

    def _read_exact(self, size: int) -> bytes:
        end = self._offset + size
        if end > len(self._data):
            raise InvalidPayload("truncated field")
        value = self._data[self._offset:end]
        self._offset = end
        return value


def _u8(value: int) -> bytes:
    return value.to_bytes(1, "big")


def _u16(value: int) -> bytes:
    return value.to_bytes(2, "big")


def _u32(value: int) -> bytes:
    return value.to_bytes(4, "big")


def _u64(value: int) -> bytes:
    return value.to_bytes(8, "big")


def _bool(value: bool) -> bytes:
    return _u8(1 if value else 0)


def _bytes(value: bytes) -> bytes:
    return _u32(len(value)) + value


def _string(value: str) -> bytes:
    return _bytes(value.encode("utf-8"))
