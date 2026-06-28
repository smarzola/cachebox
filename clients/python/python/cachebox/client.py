"""Synchronous Cachebox native protocol client."""

from __future__ import annotations

import socket
import threading
from contextlib import contextmanager
from queue import LifoQueue
from types import TracebackType
from typing import Iterator, Self

from . import protocol


class ClientError(Exception):
    """Base class for Cachebox client errors."""


class ConnectionClosed(ClientError):
    """The server closed the connection before a complete response arrived."""


class UnexpectedResponse(ClientError):
    """The response did not match the request or expected payload type."""


class ServerError(ClientError):
    """A structured error returned by the Cachebox server."""

    def __init__(self, code: protocol.ErrorCode, message: str) -> None:
        self.code = code
        self.message = message
        super().__init__(f"{code.name}: {message}")


class Client:
    """Synchronous native protocol client.

    A `Client` owns one socket connection. Operations are guarded by a lock so
    callers do not interleave request/response pairs accidentally. Use
    `ClientPool` when multiple threads or greenlets should perform independent
    operations concurrently.
    """

    def __init__(self, sock: socket.socket, max_payload_len: int = 2**32 - 1) -> None:
        self._socket = sock
        self._max_payload_len = max_payload_len
        self._next_request_id = 1
        self._lock = threading.Lock()
        self._closed = False

    @classmethod
    def connect_tcp(
        cls,
        addr: str | tuple[str, int],
        *,
        timeout: float | None = None,
        max_payload_len: int = 2**32 - 1,
    ) -> Self:
        host, port = _tcp_addr(addr)
        sock = socket.create_connection((host, port), timeout=timeout)
        return cls(sock, max_payload_len=max_payload_len)

    @classmethod
    def connect_unix(
        cls,
        path: str,
        *,
        timeout: float | None = None,
        max_payload_len: int = 2**32 - 1,
    ) -> Self:
        sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
        try:
            if timeout is not None:
                sock.settimeout(timeout)
            sock.connect(path)
        except BaseException:
            sock.close()
            raise
        return cls(sock, max_payload_len=max_payload_len)

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        self._socket.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()

    def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        payload = self._request(
            protocol.Command.GET,
            protocol.Get(namespace, key),
        )
        if isinstance(payload, protocol.Hit | protocol.Stale | protocol.Miss):
            return payload
        raise UnexpectedResponse(f"get returned {payload!r}")

    def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        payload = self._request(
            protocol.Command.PUT,
            protocol.Put(namespace, key, metadata or protocol.Metadata(), value),
        )
        if isinstance(payload, protocol.Stored):
            return payload.evicted
        raise UnexpectedResponse(f"put returned {payload!r}")

    def delete(self, namespace: str, key: bytes) -> bool:
        payload = self._request(
            protocol.Command.DELETE,
            protocol.Delete(namespace, key),
        )
        if isinstance(payload, protocol.Deleted):
            return payload.removed
        raise UnexpectedResponse(f"delete returned {payload!r}")

    def batch_get(self, namespace: str, keys: list[bytes] | tuple[bytes, ...]) -> tuple[protocol.BatchItem, ...]:
        payload = self._request(
            protocol.Command.BATCH_GET,
            protocol.BatchGet(namespace, tuple(keys)),
        )
        if isinstance(payload, protocol.BatchGetResult):
            return payload.items
        raise UnexpectedResponse(f"batch_get returned {payload!r}")

    def invalidate_tag(self, namespace: str, tag: str) -> int:
        payload = self._request(
            protocol.Command.TAG_INVALIDATE,
            protocol.TagInvalidate(namespace, tag),
        )
        if isinstance(payload, protocol.Invalidated):
            return payload.removed
        raise UnexpectedResponse(f"invalidate_tag returned {payload!r}")

    def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        payload = self._request(
            protocol.Command.LEASE_START,
            protocol.LeaseStart(namespace, key, lease_ttl_ms, allow_stale_ms),
        )
        if isinstance(
            payload,
            protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied,
        ):
            return payload
        raise UnexpectedResponse(f"start_lease returned {payload!r}")

    def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        payload = self._request(
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

    def _request(
        self,
        command: protocol.Command,
        payload: protocol.RequestPayload,
    ) -> protocol.ResponsePayload:
        with self._lock:
            request_id = self._next_id()
            request = protocol.RequestFrame(request_id, command, payload)
            self._socket.sendall(protocol.encode_request_frame(request))
            response = self._read_response()
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

    def _read_response(self) -> protocol.ResponseFrame:
        header = self._read_exact(protocol.HEADER_LEN)
        payload_len = int.from_bytes(header[16:20], "big")
        payload = self._read_exact(payload_len)
        return protocol.decode_response_frame(
            header + payload,
            max_payload_len=self._max_payload_len,
        )

    def _read_exact(self, size: int) -> bytes:
        chunks = bytearray()
        while len(chunks) < size:
            chunk = self._socket.recv(size - len(chunks))
            if not chunk:
                raise ConnectionClosed("connection closed while reading response")
            chunks.extend(chunk)
        return bytes(chunks)


class ClientPool:
    """Optional synchronous connection pool for concurrent callers."""

    def __init__(
        self,
        clients: list[Client],
        *,
        acquire_timeout: float | None = None,
    ) -> None:
        if not clients:
            raise ValueError("ClientPool requires at least one client")
        self._clients: tuple[Client, ...] = tuple(clients)
        self._available: LifoQueue[Client] = LifoQueue()
        for client in clients:
            self._available.put(client)
        self._acquire_timeout = acquire_timeout
        self._closed = False

    @classmethod
    def connect_tcp(
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
            Client.connect_tcp(
                addr,
                timeout=timeout,
                max_payload_len=max_payload_len,
            )
            for _ in range(pool_size)
        ]
        return cls(clients, acquire_timeout=acquire_timeout)

    @contextmanager
    def acquire(self) -> Iterator[Client]:
        if self._closed:
            raise ClientError("client pool is closed")
        client = self._available.get(timeout=self._acquire_timeout)
        try:
            yield client
        finally:
            self._available.put(client)

    def close(self) -> None:
        if self._closed:
            return
        self._closed = True
        for client in self._clients:
            client.close()

    def __enter__(self) -> Self:
        return self

    def __exit__(
        self,
        exc_type: type[BaseException] | None,
        exc: BaseException | None,
        traceback: TracebackType | None,
    ) -> None:
        self.close()

    def get(self, namespace: str, key: bytes) -> protocol.Hit | protocol.Stale | protocol.Miss:
        with self.acquire() as client:
            return client.get(namespace, key)

    def put(
        self,
        namespace: str,
        key: bytes,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        with self.acquire() as client:
            return client.put(namespace, key, value, metadata)

    def delete(self, namespace: str, key: bytes) -> bool:
        with self.acquire() as client:
            return client.delete(namespace, key)

    def batch_get(self, namespace: str, keys: list[bytes] | tuple[bytes, ...]) -> tuple[protocol.BatchItem, ...]:
        with self.acquire() as client:
            return client.batch_get(namespace, keys)

    def invalidate_tag(self, namespace: str, tag: str) -> int:
        with self.acquire() as client:
            return client.invalidate_tag(namespace, tag)

    def start_lease(
        self,
        namespace: str,
        key: bytes,
        lease_ttl_ms: int,
        allow_stale_ms: int | None = None,
    ) -> protocol.Hit | protocol.Stale | protocol.LeaseGranted | protocol.LeaseDenied:
        with self.acquire() as client:
            return client.start_lease(namespace, key, lease_ttl_ms, allow_stale_ms)

    def complete_lease(
        self,
        namespace: str,
        key: bytes,
        lease_token: str,
        value: bytes,
        metadata: protocol.Metadata | None = None,
    ) -> int:
        with self.acquire() as client:
            return client.complete_lease(namespace, key, lease_token, value, metadata)


def _tcp_addr(addr: str | tuple[str, int]) -> tuple[str, int]:
    if isinstance(addr, tuple):
        return addr
    host, port = addr.rsplit(":", 1)
    return host, int(port)
