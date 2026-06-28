"""High-level Cachebox caching APIs."""

from __future__ import annotations

import asyncio
import inspect
import random
import time
from collections.abc import Awaitable, Callable, Sequence
from dataclasses import dataclass
from functools import wraps
from typing import Any, Protocol, TypeVar

from . import protocol
from .async_client import AsyncClient, AsyncClientPool
from .client import Client, ClientPool
from .keys import build_custom_key, build_function_key, build_template_key, make_metadata
from .serde import BytesSerializer, Serializer

F = TypeVar("F", bound=Callable[..., Any])


class CacheBackend(Protocol):
    def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        ...

    def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        ...

    def delete(self, namespace: str, key: bytes) -> bool:
        ...

    def invalidate_tag(self, namespace: str, tag: str) -> int:
        ...

    def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        ...

    def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        ...


class AsyncCacheBackend(Protocol):
    async def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        ...

    async def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        ...

    async def delete(self, namespace: str, key: bytes) -> bool:
        ...

    async def invalidate_tag(self, namespace: str, tag: str) -> int:
        ...

    async def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        ...

    async def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        ...


@dataclass(frozen=True)
class DogpilePolicy:
    """Lease-backed stampede protection policy."""

    enabled: bool = True
    lease_ttl_ms: int = 30_000
    allow_stale_ms: int | None = None
    return_stale: bool = True
    wait_timeout_ms: int = 5_000
    retry_interval_ms: int = 25
    retry_jitter_ms: int = 25
    return_stale_on_error: bool = True


class DogpileTimeoutError(Exception):
    """A cache fill was still leased by another caller after waiting."""


class Cachebox:
    """Synchronous high-level Cachebox cache API."""

    def __init__(
        self,
        client: CacheBackend,
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
        dogpile: DogpilePolicy | None = None,
    ) -> None:
        self._client = client
        self.namespace = namespace
        self.serializer = serializer or BytesSerializer()
        self.key_prefix = key_prefix
        self.key_version = key_version
        self.dogpile = dogpile or DogpilePolicy()

    @classmethod
    def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
        dogpile: DogpilePolicy | None = None,
        pool_size: int | None = None,
        timeout: float | None = None,
        acquire_timeout: float | None = None,
    ) -> "Cachebox":
        client: Client | ClientPool
        if pool_size is None:
            client = Client.connect_tcp(addr, timeout=timeout)
        else:
            client = ClientPool.connect_tcp(
                addr,
                pool_size=pool_size,
                timeout=timeout,
                acquire_timeout=acquire_timeout,
            )
        return cls(
            client,
            namespace=namespace,
            serializer=serializer,
            key_prefix=key_prefix,
            key_version=key_version,
            dogpile=dogpile,
        )

    def close(self) -> None:
        close = getattr(self._client, "close", None)
        if close is not None:
            close()

    def __enter__(self) -> "Cachebox":
        return self

    def __exit__(self, *args: object) -> None:
        self.close()

    def get(
        self,
        key: bytes | str | Any,
        *,
        default: Any = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> Any:
        return self._get_keyed(
            self.key(key),
            default=default,
            serializer=serializer,
            namespace=namespace,
        )

    def set(
        self,
        key: bytes | str | Any,
        value: Any,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> int:
        selected = serializer or self.serializer
        return self._set_keyed(
            self.key(key),
            value,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=selected,
            namespace=namespace,
        )

    def delete(self, key: bytes | str | Any, *, namespace: str | None = None) -> bool:
        return self._client.delete(namespace or self.namespace, self.key(key))

    def invalidate_tag(self, tag: str, *, namespace: str | None = None) -> int:
        return self._client.invalidate_tag(namespace or self.namespace, tag)

    def get_or_set(
        self,
        key: bytes | str | Any,
        factory: Callable[[], Any],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Any:
        return self._get_or_set_keyed(
            self.key(key),
            factory,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=serializer,
            namespace=namespace,
            dogpile=dogpile,
        )

    def _get_or_set_keyed(
        self,
        key: bytes,
        factory: Callable[[], Any],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Any:
        policy = _resolve_dogpile(self.dogpile, dogpile)
        if policy.enabled:
            return self._get_or_set_keyed_with_lease(
                key,
                factory,
                ttl_ms=ttl_ms,
                stale_ttl_ms=stale_ttl_ms,
                tags=tags,
                cost=cost,
                serializer=serializer,
                namespace=namespace,
                policy=policy,
            )

        cached = self._get_keyed(
            key,
            default=_MISSING,
            serializer=serializer,
            namespace=namespace,
        )
        if cached is not _MISSING:
            return cached
        value = factory()
        self._set_keyed(
            key,
            value,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=serializer,
            namespace=namespace,
        )
        return value

    def _get_or_set_keyed_with_lease(
        self,
        key: bytes,
        factory: Callable[[], Any],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        policy: DogpilePolicy,
    ) -> Any:
        selected = serializer or self.serializer
        resolved_namespace = namespace or self.namespace
        deadline = _deadline(policy.wait_timeout_ms)

        while True:
            outcome = self._client.start_lease(
                resolved_namespace,
                key,
                policy.lease_ttl_ms,
                policy.allow_stale_ms,
            )
            if isinstance(outcome, protocol.Hit):
                return selected.decode(outcome.value)
            if isinstance(outcome, protocol.Stale):
                if policy.return_stale:
                    return selected.decode(outcome.value)
                if _timed_out(deadline):
                    raise DogpileTimeoutError("timed out waiting for fresh cache value")
                _sleep_retry(policy)
                continue
            if isinstance(outcome, protocol.LeaseGranted):
                try:
                    value = factory()
                except Exception:
                    if outcome.stale_value is not None and policy.return_stale_on_error:
                        return selected.decode(outcome.stale_value)
                    raise
                encoded = selected.encode(value)
                self._client.complete_lease(
                    resolved_namespace,
                    key,
                    outcome.lease_token,
                    encoded,
                    make_metadata(
                        ttl_ms=ttl_ms,
                        stale_ttl_ms=stale_ttl_ms,
                        tags=tags,
                        cost=cost,
                        content_type=selected.content_type,
                    ),
                )
                return value
            if _timed_out(deadline):
                raise DogpileTimeoutError("timed out waiting for cache lease")
            _sleep_retry(policy)

    def _get_keyed(
        self,
        key: bytes,
        *,
        default: Any = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> Any:
        result = self._client.get(namespace or self.namespace, key)
        if isinstance(result, protocol.Miss):
            return default
        return (serializer or self.serializer).decode(result.value)

    def _set_keyed(
        self,
        key: bytes,
        value: Any,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> int:
        selected = serializer or self.serializer
        return self._client.put(
            namespace or self.namespace,
            key,
            selected.encode(value),
            make_metadata(
                ttl_ms=ttl_ms,
                stale_ttl_ms=stale_ttl_ms,
                tags=tags,
                cost=cost,
                content_type=selected.content_type,
            ),
        )

    def memoize(
        self,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        prefix: str | None = None,
        version: str | int | None = None,
        key_func: Callable[..., Any] | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Callable[[F], F]:
        def decorator(function: F) -> F:
            @wraps(function)
            def wrapper(*args: Any, **kwargs: Any) -> Any:
                key = build_function_key(
                    function,
                    *args,
                    prefix=prefix if prefix is not None else self.key_prefix,
                    version=version if version is not None else self.key_version,
                    key_func=key_func,
                    **kwargs,
                )
                return self._get_or_set_keyed(
                    key,
                    lambda: function(*args, **kwargs),
                    ttl_ms=ttl_ms,
                    stale_ttl_ms=stale_ttl_ms,
                    tags=tags,
                    cost=cost,
                    serializer=serializer,
                    namespace=namespace,
                    dogpile=dogpile,
                )

            return wrapper  # type: ignore[return-value]

        return decorator

    def cached(
        self,
        key: bytes | str | Callable[..., Any],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        prefix: str | None = None,
        version: str | int | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Callable[[F], F]:
        def decorator(function: F) -> F:
            @wraps(function)
            def wrapper(*args: Any, **kwargs: Any) -> Any:
                cache_key = _explicit_key(
                    key,
                    function,
                    args,
                    kwargs,
                    prefix=prefix if prefix is not None else self.key_prefix,
                    version=version if version is not None else self.key_version,
                )
                return self._get_or_set_keyed(
                    cache_key,
                    lambda: function(*args, **kwargs),
                    ttl_ms=ttl_ms,
                    stale_ttl_ms=stale_ttl_ms,
                    tags=tags,
                    cost=cost,
                    serializer=serializer,
                    namespace=namespace,
                    dogpile=dogpile,
                )

            return wrapper  # type: ignore[return-value]

        return decorator

    def key(self, value: bytes | str | Any) -> bytes:
        return build_custom_key(
            value,
            prefix=self.key_prefix,
            version=self.key_version,
        )


class AsyncCachebox:
    """Asyncio high-level Cachebox cache API."""

    def __init__(
        self,
        client: AsyncCacheBackend,
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
        dogpile: DogpilePolicy | None = None,
    ) -> None:
        self._client = client
        self.namespace = namespace
        self.serializer = serializer or BytesSerializer()
        self.key_prefix = key_prefix
        self.key_version = key_version
        self.dogpile = dogpile or DogpilePolicy()

    @classmethod
    async def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
        dogpile: DogpilePolicy | None = None,
        pool_size: int | None = None,
        timeout: float | None = None,
        acquire_timeout: float | None = None,
    ) -> "AsyncCachebox":
        client: AsyncClient | AsyncClientPool
        if pool_size is None:
            client = await AsyncClient.connect_tcp(addr, timeout=timeout)
        else:
            client = await AsyncClientPool.connect_tcp(
                addr,
                pool_size=pool_size,
                timeout=timeout,
                acquire_timeout=acquire_timeout,
            )
        return cls(
            client,
            namespace=namespace,
            serializer=serializer,
            key_prefix=key_prefix,
            key_version=key_version,
            dogpile=dogpile,
        )

    async def close(self) -> None:
        close = getattr(self._client, "close", None)
        if close is not None:
            result = close()
            if inspect.isawaitable(result):
                await result

    async def __aenter__(self) -> "AsyncCachebox":
        return self

    async def __aexit__(self, *args: object) -> None:
        await self.close()

    async def get(
        self,
        key: bytes | str | Any,
        *,
        default: Any = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> Any:
        return await self._get_keyed(
            self.key(key),
            default=default,
            serializer=serializer,
            namespace=namespace,
        )

    async def set(
        self,
        key: bytes | str | Any,
        value: Any,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> int:
        selected = serializer or self.serializer
        return await self._set_keyed(
            self.key(key),
            value,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=selected,
            namespace=namespace,
        )

    async def delete(self, key: bytes | str | Any, *, namespace: str | None = None) -> bool:
        return await self._client.delete(namespace or self.namespace, self.key(key))

    async def invalidate_tag(self, tag: str, *, namespace: str | None = None) -> int:
        return await self._client.invalidate_tag(namespace or self.namespace, tag)

    async def get_or_set(
        self,
        key: bytes | str | Any,
        factory: Callable[[], Any] | Callable[[], Awaitable[Any]],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Any:
        return await self._get_or_set_keyed(
            self.key(key),
            factory,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=serializer,
            namespace=namespace,
            dogpile=dogpile,
        )

    async def _get_or_set_keyed(
        self,
        key: bytes,
        factory: Callable[[], Any] | Callable[[], Awaitable[Any]],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Any:
        policy = _resolve_dogpile(self.dogpile, dogpile)
        if policy.enabled:
            return await self._get_or_set_keyed_with_lease(
                key,
                factory,
                ttl_ms=ttl_ms,
                stale_ttl_ms=stale_ttl_ms,
                tags=tags,
                cost=cost,
                serializer=serializer,
                namespace=namespace,
                policy=policy,
            )

        cached = await self._get_keyed(
            key,
            default=_MISSING,
            serializer=serializer,
            namespace=namespace,
        )
        if cached is not _MISSING:
            return cached
        value = factory()
        if inspect.isawaitable(value):
            value = await value
        await self._set_keyed(
            key,
            value,
            ttl_ms=ttl_ms,
            stale_ttl_ms=stale_ttl_ms,
            tags=tags,
            cost=cost,
            serializer=serializer,
            namespace=namespace,
        )
        return value

    async def _get_or_set_keyed_with_lease(
        self,
        key: bytes,
        factory: Callable[[], Any] | Callable[[], Awaitable[Any]],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        policy: DogpilePolicy,
    ) -> Any:
        selected = serializer or self.serializer
        resolved_namespace = namespace or self.namespace
        deadline = _deadline(policy.wait_timeout_ms)

        while True:
            outcome = await self._client.start_lease(
                resolved_namespace,
                key,
                policy.lease_ttl_ms,
                policy.allow_stale_ms,
            )
            if isinstance(outcome, protocol.Hit):
                return selected.decode(outcome.value)
            if isinstance(outcome, protocol.Stale):
                if policy.return_stale:
                    return selected.decode(outcome.value)
                if _timed_out(deadline):
                    raise DogpileTimeoutError("timed out waiting for fresh cache value")
                await _async_sleep_retry(policy)
                continue
            if isinstance(outcome, protocol.LeaseGranted):
                try:
                    value = factory()
                    if inspect.isawaitable(value):
                        value = await value
                except Exception:
                    if outcome.stale_value is not None and policy.return_stale_on_error:
                        return selected.decode(outcome.stale_value)
                    raise
                encoded = selected.encode(value)
                await self._client.complete_lease(
                    resolved_namespace,
                    key,
                    outcome.lease_token,
                    encoded,
                    make_metadata(
                        ttl_ms=ttl_ms,
                        stale_ttl_ms=stale_ttl_ms,
                        tags=tags,
                        cost=cost,
                        content_type=selected.content_type,
                    ),
                )
                return value
            if _timed_out(deadline):
                raise DogpileTimeoutError("timed out waiting for cache lease")
            await _async_sleep_retry(policy)

    async def _get_keyed(
        self,
        key: bytes,
        *,
        default: Any = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> Any:
        result = await self._client.get(namespace or self.namespace, key)
        if isinstance(result, protocol.Miss):
            return default
        return (serializer or self.serializer).decode(result.value)

    async def _set_keyed(
        self,
        key: bytes,
        value: Any,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
    ) -> int:
        selected = serializer or self.serializer
        return await self._client.put(
            namespace or self.namespace,
            key,
            selected.encode(value),
            make_metadata(
                ttl_ms=ttl_ms,
                stale_ttl_ms=stale_ttl_ms,
                tags=tags,
                cost=cost,
                content_type=selected.content_type,
            ),
        )

    def memoize(
        self,
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        prefix: str | None = None,
        version: str | int | None = None,
        key_func: Callable[..., Any] | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Callable[[F], F]:
        def decorator(function: F) -> F:
            @wraps(function)
            async def wrapper(*args: Any, **kwargs: Any) -> Any:
                key = build_function_key(
                    function,
                    *args,
                    prefix=prefix if prefix is not None else self.key_prefix,
                    version=version if version is not None else self.key_version,
                    key_func=key_func,
                    **kwargs,
                )
                return await self._get_or_set_keyed(
                    key,
                    lambda: function(*args, **kwargs),
                    ttl_ms=ttl_ms,
                    stale_ttl_ms=stale_ttl_ms,
                    tags=tags,
                    cost=cost,
                    serializer=serializer,
                    namespace=namespace,
                    dogpile=dogpile,
                )

            return wrapper  # type: ignore[return-value]

        return decorator

    def cached(
        self,
        key: bytes | str | Callable[..., Any],
        *,
        ttl_ms: int | None = None,
        stale_ttl_ms: int | None = None,
        tags: Sequence[str] = (),
        cost: int | None = None,
        serializer: Serializer | None = None,
        namespace: str | None = None,
        prefix: str | None = None,
        version: str | int | None = None,
        dogpile: DogpilePolicy | bool | None = None,
    ) -> Callable[[F], F]:
        def decorator(function: F) -> F:
            @wraps(function)
            async def wrapper(*args: Any, **kwargs: Any) -> Any:
                cache_key = _explicit_key(
                    key,
                    function,
                    args,
                    kwargs,
                    prefix=prefix if prefix is not None else self.key_prefix,
                    version=version if version is not None else self.key_version,
                )
                return await self._get_or_set_keyed(
                    cache_key,
                    lambda: function(*args, **kwargs),
                    ttl_ms=ttl_ms,
                    stale_ttl_ms=stale_ttl_ms,
                    tags=tags,
                    cost=cost,
                    serializer=serializer,
                    namespace=namespace,
                    dogpile=dogpile,
                )

            return wrapper  # type: ignore[return-value]

        return decorator

    def key(self, value: bytes | str | Any) -> bytes:
        return build_custom_key(
            value,
            prefix=self.key_prefix,
            version=self.key_version,
        )


def _explicit_key(
    key: bytes | str | Callable[..., Any],
    function: Callable[..., Any],
    args: tuple[Any, ...],
    kwargs: dict[str, Any],
    *,
    prefix: str | None,
    version: str | int | None,
) -> bytes:
    if callable(key):
        return build_custom_key(key(*args, **kwargs), prefix=prefix, version=version)
    if isinstance(key, str) and "{" in key:
        signature = inspect.signature(function)
        bound = signature.bind(*args, **kwargs)
        bound.apply_defaults()
        return build_template_key(
            key,
            prefix=prefix,
            version=version,
            **bound.arguments,
        )
    return build_custom_key(key, prefix=prefix, version=version)


def _resolve_dogpile(
    default: DogpilePolicy,
    override: DogpilePolicy | bool | None,
) -> DogpilePolicy:
    if override is None:
        return default
    if isinstance(override, bool):
        if override:
            return default
        return DogpilePolicy(enabled=False)
    return override


def _deadline(timeout_ms: int) -> float:
    return time.monotonic() + (timeout_ms / 1000)


def _timed_out(deadline: float) -> bool:
    return time.monotonic() >= deadline


def _retry_delay_seconds(policy: DogpilePolicy) -> float:
    jitter_ms = (
        random.uniform(0, policy.retry_jitter_ms)
        if policy.retry_jitter_ms > 0
        else 0
    )
    return max(0, policy.retry_interval_ms + jitter_ms) / 1000


def _sleep_retry(policy: DogpilePolicy) -> None:
    time.sleep(_retry_delay_seconds(policy))


async def _async_sleep_retry(policy: DogpilePolicy) -> None:
    await asyncio.sleep(_retry_delay_seconds(policy))


class _Missing:
    pass


_MISSING = _Missing()
