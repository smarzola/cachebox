"""Serialization helpers for Cachebox high-level APIs."""

from __future__ import annotations

import json
import pickle
from typing import Any, Protocol, runtime_checkable


class SerializationError(Exception):
    """Base class for high-level serialization errors."""


class EncodeError(SerializationError):
    """A value could not be serialized for storage."""


class DecodeError(SerializationError):
    """A cached value could not be deserialized."""


@runtime_checkable
class Serializer(Protocol):
    """Serializer contract for high-level cache values."""

    content_type: str

    def encode(self, value: Any) -> bytes:
        """Serialize a Python value into bytes."""

    def decode(self, payload: bytes) -> Any:
        """Deserialize bytes into a Python value."""


class BytesSerializer:
    """Serializer for byte-oriented values.

    Accepts `bytes`, `bytearray`, and `memoryview`. Decoding always returns
    immutable `bytes`.
    """

    content_type = "application/octet-stream"

    def encode(self, value: Any) -> bytes:
        try:
            if isinstance(value, bytes):
                return value
            if isinstance(value, bytearray | memoryview):
                return bytes(value)
        except TypeError as error:
            raise EncodeError("value cannot be converted to bytes") from error
        raise EncodeError("BytesSerializer requires bytes-like values")

    def decode(self, payload: bytes) -> bytes:
        return bytes(payload)


class JsonSerializer:
    """UTF-8 JSON serializer with deterministic output."""

    content_type = "application/json"

    def __init__(self, *, sort_keys: bool = True) -> None:
        self._sort_keys = sort_keys

    def encode(self, value: Any) -> bytes:
        try:
            return json.dumps(
                value,
                allow_nan=False,
                ensure_ascii=False,
                separators=(",", ":"),
                sort_keys=self._sort_keys,
            ).encode("utf-8")
        except (TypeError, ValueError) as error:
            raise EncodeError("value is not JSON serializable") from error

    def decode(self, payload: bytes) -> Any:
        try:
            return json.loads(payload.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as error:
            raise DecodeError("payload is not valid UTF-8 JSON") from error


class PickleSerializer:
    """Python pickle serializer.

    Pickle can execute code while decoding untrusted payloads. Keep its use
    explicit at call sites and prefer `JsonSerializer` for portable data.
    """

    content_type = "application/x-python-pickle"

    def __init__(self, *, protocol: int = pickle.HIGHEST_PROTOCOL) -> None:
        self._protocol = protocol

    def encode(self, value: Any) -> bytes:
        try:
            return pickle.dumps(value, protocol=self._protocol)
        except (pickle.PickleError, TypeError, ValueError) as error:
            raise EncodeError("value is not pickle serializable") from error

    def decode(self, payload: bytes) -> Any:
        try:
            return pickle.loads(payload)
        except (pickle.PickleError, EOFError, AttributeError, ImportError, IndexError, ValueError) as error:
            raise DecodeError("payload is not valid pickle data") from error
