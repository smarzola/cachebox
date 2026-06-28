"""High-level Cachebox caching APIs."""

from __future__ import annotations

import inspect
from collections.abc import Awaitable, Callable, Sequence
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
    ) -> None:
        self._client = client
        self.namespace = namespace
        self.serializer = serializer or BytesSerializer()
        self.key_prefix = key_prefix
        self.key_version = key_version

    @classmethod
    def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
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
    ) -> Any:
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
    ) -> None:
        self._client = client
        self.namespace = namespace
        self.serializer = serializer or BytesSerializer()
        self.key_prefix = key_prefix
        self.key_version = key_version

    @classmethod
    async def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        namespace: str = "default",
        serializer: Serializer | None = None,
        key_prefix: str | None = None,
        key_version: str | int | None = None,
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
    ) -> Any:
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


class _Missing:
    pass


_MISSING = _Missing()
