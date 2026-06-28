"""Asyncio Cachebox native protocol client."""

from __future__ import annotations

import asyncio
from contextlib import asynccontextmanager
from types import TracebackType
from typing import AsyncIterator, Self

from . import protocol
from .client import ClientError, ConnectionClosed, ServerError, UnexpectedResponse


class AsyncClient:
    """Asyncio native protocol client.

    An `AsyncClient` owns one stream connection. Operations are guarded by an
    async lock so callers do not interleave request/response pairs accidentally.
    Use `AsyncClientPool` when multiple coroutines should perform independent
    operations concurrently.
    """

    def __init__(
        self,
        reader: asyncio.StreamReader,
        writer: asyncio.StreamWriter,
        max_payload_len: int = 2**32 - 1,
    ) -> None:
        self._reader = reader
        self._writer = writer
        self._max_payload_len = max_payload_len
        self._next_request_id = 1
        self._lock = asyncio.Lock()
        self._closed = False

    @classmethod
    async def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        timeout: float | None = None,
        max_payload_len: int = 2**32 - 1,
    ) -> Self:
        host, port = _tcp_addr(addr)
        if timeout is None:
            reader, writer = await asyncio.open_connection(host, port)
        else:
            reader, writer = await asyncio.wait_for(
                asyncio.open_connection(host, port),
                timeout=timeout,
            )
        return cls(reader, writer, max_payload_len=max_payload_len)

    @classmethod
    async def connect_unix(
        cls,
        path: str,
        *,
        timeout: float | None = None,
        max_payload_len: int = 2**32 - 1,
    ) -> Self:
        if timeout is None:
            reader, writer = await asyncio.open_unix_connection(path)
        else:
            reader, writer = await asyncio.wait_for(
                asyncio.open_unix_connection(path),
                timeout=timeout,
            )
        return cls(reader, writer, max_payload_len=max_payload_len)

    @property
    def closed(self) -> bool:
        return self._closed

    async def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        self._writer.close()
        try:
            await self._writer.wait_closed()
        except OSError:
            pass

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        await self.close()

    async def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        payload = await self._request(
            protocol.Command.GET,
            protocol.Get(namespace, key),
        )
        if isinstance(payload, protocol.Hit | protocol.Stale | protocol.Miss):
            return payload
        raise UnexpectedResponse(f"get returned {payload!r}")

    async def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        payload = await self._request(
            protocol.Command.PUT,
            protocol.Put(namespace, key, metadata or protocol.Metadata(), value),
        )
        if isinstance(payload, protocol.Stored):
            return payload.evicted
        raise UnexpectedResponse(f"put returned {payload!r}")

    async def delete(self, namespace: str, key: bytes) -> bool:
        payload = await self._request(
            protocol.Command.DELETE,
            protocol.Delete(namespace, key),
        )
        if isinstance(payload, protocol.Deleted):
            return payload.removed
        raise UnexpectedResponse(f"delete returned {payload!r}")

    async def batch_get(
        self,
        namespace: str,
        keys: list[bytes] | tuple[bytes, ...],
    ) -> tuple[protocol.BatchItem, ...]:
        payload = await self._request(
            protocol.Command.BATCH_GET,
            protocol.BatchGet(namespace, tuple(keys)),
        )
        if isinstance(payload, protocol.BatchGetResult):
            return payload.items
        raise UnexpectedResponse(f"batch_get returned {payload!r}")

    async def invalidate_tag(self, namespace: str, tag: str) -> int:
        payload = await self._request(
            protocol.Command.TAG_INVALIDATE,
            protocol.TagInvalidate(namespace, tag),
        )
        if isinstance(payload, protocol.Invalidated):
            return payload.removed
        raise UnexpectedResponse(f"invalidate_tag returned {payload!r}")

    async def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        payload = await self._request(
            protocol.Command.LEASE_START,
            protocol.LeaseStart(namespace, key, lease_ttl_ms, allow_stale_ms),
        )
        if isinstance(
            payload,
            protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied,
        ):
            return payload
        raise UnexpectedResponse(f"start_lease returned {payload!r}")

    async def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        payload = await self._request(
            protocol.Command.LEASE_COMPLETE,
            protocol.LeaseComplete(
                namespace,
                key,
                lease_token,
                metadata or protocol.Metadata(),
                value,
            ),
        )
        if isinstance(payload, protocol.Stored):
            return payload.evicted
        raise UnexpectedResponse(f"complete_lease returned {payload!r}")

    async def _request(
        self,
        command: protocol.Command,
        payload: protocol.RequestPayload,
    ) -> protocol.ResponsePayload:
        if self._closed:
            raise ConnectionClosed("connection is closed")
        async with self._lock:
            request_id = self._next_id()
            request = protocol.RequestFrame(request_id, command, payload)
            try:
                self._writer.write(protocol.encode_request_frame(request))
                await self._writer.drain()
                response = await self._read_response()
            except asyncio.CancelledError:
                await self.close()
                raise
            except (OSError, ConnectionError, asyncio.IncompleteReadError) as error:
                await self.close()
                raise ConnectionClosed("connection closed during request") from error

            if response.request_id != request_id or response.command != command:
                raise UnexpectedResponse(
                    "response request_id or command did not match request"
                )
            if isinstance(response.payload, protocol.Error):
                raise ServerError(response.payload.code, response.payload.message)
            return response.payload

    def _next_id(self) -> int:
        request_id = self._next_request_id
        self._next_request_id += 1
        if self._next_request_id > 2**64 - 1:
            self._next_request_id = 1
        return request_id

    async def _read_response(self) -> protocol.ResponseFrame:
        header = await self._reader.readexactly(protocol.HEADER_LEN)
        payload_len = int.from_bytes(header[16:20], "big")
        payload = await self._reader.readexactly(payload_len)
        return protocol.decode_response_frame(
            header + payload,
            max_payload_len=self._max_payload_len,
        )


class AsyncClientPool:
    """Optional asyncio connection pool for concurrent coroutine callers."""

    def __init__(
        self,
        clients: list[AsyncClient],
        *,
        acquire_timeout: float | None = None,
    ) -> None:
        if not clients:
            raise ValueError("AsyncClientPool requires at least one client")
        self._clients: tuple[AsyncClient, ...] = tuple(clients)
        self._available: asyncio.LifoQueue[AsyncClient] = asyncio.LifoQueue()
        for client in clients:
            self._available.put_nowait(client)
        self._acquire_timeout = acquire_timeout
        self._closed = False

    @classmethod
    async def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        pool_size: int,
        timeout: float | None = None,
        acquire_timeout: float | None = None,
        max_payload_len: int = 2**32 - 1,
    ) -> Self:
        if pool_size <= 0:
            raise ValueError("pool_size must be greater than zero")
        clients = [
            await AsyncClient.connect_tcp(
                addr,
                timeout=timeout,
                max_payload_len=max_payload_len,
            )
            for _ in range(pool_size)
        ]
        return cls(clients, acquire_timeout=acquire_timeout)

    @asynccontextmanager
    async def acquire(self) -> AsyncIterator[AsyncClient]:
        if self._closed:
            raise ClientError("async client pool is closed")
        client = await self._acquire_client()
        try:
            yield client
        finally:
            if not self._closed and not client.closed:
                self._available.put_nowait(client)

    async def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        await asyncio.gather(
            *(client.close() for client in self._clients),
            return_exceptions=True,
        )

    async def __aenter__(self) -> Self:
        return self

    async def __aexit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        await self.close()

    async def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        async with self.acquire() as client:
            return await client.get(namespace, key)

    async def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        async with self.acquire() as client:
            return await client.put(namespace, key, value, metadata)

    async def delete(self, namespace: str, key: bytes) -> bool:
        async with self.acquire() as client:
            return await client.delete(namespace, key)

    async def batch_get(
        self,
        namespace: str,
        keys: list[bytes] | tuple[bytes, ...],
    ) -> tuple[protocol.BatchItem, ...]:
        async with self.acquire() as client:
            return await client.batch_get(namespace, keys)

    async def invalidate_tag(self, namespace: str, tag: str) -> int:
        async with self.acquire() as client:
            return await client.invalidate_tag(namespace, tag)

    async def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        async with self.acquire() as client:
            return await client.start_lease(namespace, key, lease_ttl_ms, allow_stale_ms)

    async def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        async with self.acquire() as client:
            return await client.complete_lease(namespace, key, lease_token, value, metadata)

    async def _acquire_client(self) -> AsyncClient:
        while True:
            if self._closed:
                raise ClientError("async client pool is closed")
            try:
                if self._acquire_timeout is None:
                    client = await self._available.get()
                else:
                    client = await asyncio.wait_for(
                        self._available.get(),
                        timeout=self._acquire_timeout,
                    )
            except TimeoutError as error:
                raise ClientError("timed out acquiring async client") from error

            if not client.closed:
                return client


def _tcp_addr(addr: str | tuple[str, int]) -> tuple[str, int]:
    if isinstance(addr, tuple):
        return addr
    host, port = addr.rsplit(":", 1)
    return host, int(port)
