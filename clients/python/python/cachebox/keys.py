"""Deterministic key and metadata helpers for high-level Cachebox APIs."""

from __future__ import annotations

import base64
import inspect
import json
from collections.abc import Callable, Mapping, Sequence
from dataclasses import dataclass
from typing import Any

from . import protocol


class KeyBuildError(Exception):
    """A high-level cache key could not be built deterministically."""


@dataclass(frozen=True)
class KeyOptions:
    """Options shared by high-level key builders."""

    prefix: str | None = None
    version: str | int | None = None


def build_function_key(
    function: Callable[..., Any],
    *args: Any,
    prefix: str | None = None,
    version: str | int | None = None,
    key_func: Callable[..., Any] | None = None,
    **kwargs: Any,
) -> bytes:
    """Build a deterministic key for a function call.

    Positional and keyword calls that bind to the same signature produce the
    same key. Default argument values are included after `Signature.bind` so a
    caller omitting a default and a caller passing the same value explicitly
    share a key.
    """

    if key_func is not None:
        return build_custom_key(
            key_func(*args, **kwargs),
            prefix=prefix,
            version=version,
        )

    try:
        signature = inspect.signature(function)
        bound = signature.bind(*args, **kwargs)
    except (TypeError, ValueError) as error:
        raise KeyBuildError(str(error)) from error
    bound.apply_defaults()

    payload = {
        "function": f"{function.__module__}.{function.__qualname__}",
        "arguments": [
            [name, _normalize_for_key(value)]
            for name, value in bound.arguments.items()
        ],
    }
    return _join_key_parts(
        prefix,
        version,
        _json_bytes(payload),
    )


def build_template_key(
    template: str,
    /,
    *,
    prefix: str | None = None,
    version: str | int | None = None,
    **values: Any,
) -> bytes:
    """Build a key from an explicit format template.

    Template values are normalized with the same deterministic rules as
    function arguments before being converted to strings.
    """

    try:
        rendered = template.format(
            **{
                name: _stringify_template_value(_normalize_for_key(value))
                for name, value in values.items()
            }
        )
    except (KeyError, IndexError, ValueError) as error:
        raise KeyBuildError(f"could not render key template {template!r}") from error
    return _join_key_parts(prefix, version, rendered.encode("utf-8"))


def build_custom_key(
    value: bytes | str | Any,
    *,
    prefix: str | None = None,
    version: str | int | None = None,
) -> bytes:
    """Normalize a custom key function result to bytes."""

    if isinstance(value, bytes):
        key = value
    elif isinstance(value, str):
        key = value.encode("utf-8")
    else:
        key = _json_bytes(_normalize_for_key(value))
    return _join_key_parts(prefix, version, key)


def make_metadata(
    *,
    ttl_ms: int | None = None,
    stale_ttl_ms: int | None = None,
    tags: Sequence[str] = (),
    cost: int | None = None,
    content_type: str | None = None,
) -> protocol.Metadata:
    """Build protocol metadata from high-level cache options."""

    return protocol.Metadata(
        ttl=protocol.Ttl(ttl_ms) if ttl_ms is not None else None,
        stale_ttl=protocol.Ttl(stale_ttl_ms) if stale_ttl_ms is not None else None,
        cost=cost,
        tags=tuple(tags),
        content_type=(
            protocol.ContentType.OTHER
            if content_type and content_type != "application/octet-stream"
            else protocol.ContentType.OCTET_STREAM
        ),
    )


def _join_key_parts(
    prefix: str | None,
    version: str | int | None,
    key: bytes,
) -> bytes:
    parts: list[bytes] = []
    if prefix is not None:
        parts.append(_safe_label(prefix, "prefix"))
    if version is not None:
        parts.append(_safe_label(str(version), "version"))
    parts.append(key)
    return b":".join(parts)


def _safe_label(value: str, label: str) -> bytes:
    if value == "":
        raise KeyBuildError(f"{label} cannot be empty")
    try:
        encoded = value.encode("ascii")
    except UnicodeEncodeError as error:
        raise KeyBuildError(f"{label} must be ASCII") from error
    if b":" in encoded:
        raise KeyBuildError(f"{label} cannot contain ':'")
    return encoded


def _json_bytes(value: Any) -> bytes:
    try:
        return json.dumps(
            value,
            allow_nan=False,
            ensure_ascii=False,
            separators=(",", ":"),
            sort_keys=True,
        ).encode("utf-8")
    except (TypeError, ValueError) as error:
        raise KeyBuildError("key contains values that cannot be JSON encoded") from error


def _normalize_for_key(value: Any) -> Any:
    if value is None or isinstance(value, bool | int | float | str):
        return value
    if isinstance(value, bytes | bytearray | memoryview):
        return {
            "__cachebox_type__": "bytes",
            "base64": base64.b64encode(bytes(value)).decode("ascii"),
        }
    if isinstance(value, tuple):
        return {
            "__cachebox_type__": "tuple",
            "items": [_normalize_for_key(item) for item in value],
        }
    if isinstance(value, list):
        return [_normalize_for_key(item) for item in value]
    if isinstance(value, Mapping):
        return _normalize_mapping(value)
    raise KeyBuildError(
        f"unsupported key value {value!r}; pass key_func or explicit key template"
    )


def _normalize_mapping(value: Mapping[Any, Any]) -> list[list[Any]]:
    normalized: list[list[Any]] = []
    for key, item in value.items():
        if not isinstance(key, str):
            raise KeyBuildError("mapping keys used in cache keys must be strings")
        normalized.append([key, _normalize_for_key(item)])
    normalized.sort(key=lambda pair: pair[0])
    return normalized


def _stringify_template_value(value: Any) -> str:
    if isinstance(value, str):
        return value
    if isinstance(value, bool):
        return "true" if value else "false"
    if value is None:
        return "null"
    if isinstance(value, int | float):
        return str(value)
    return _json_bytes(value).decode("utf-8")
