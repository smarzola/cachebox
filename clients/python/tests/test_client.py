import asyncio
import http.client
import socket
import subprocess
import time
from pathlib import Path

from cachebox import AsyncClient, AsyncClientPool, Client, ClientError, ClientPool, ServerError, protocol


ROOT = Path(__file__).resolve().parents[3]


def unused_addr() -> str:
    with socket.socket(socket.AF_INET, socket.SOCK_STREAM) as sock:
        sock.bind(("127.0.0.1", 0))
        host, port = sock.getsockname()
    return f"{host}:{port}"


def wait_for_health(addr: str) -> None:
    deadline = time.monotonic() + 5
    host, port = addr.rsplit(":", 1)
    last_error = None
    while time.monotonic() < deadline:
        try:
            conn = http.client.HTTPConnection(host, int(port), timeout=1)
            conn.request("GET", "/healthz")
            response = conn.getresponse()
            response.read()
            if response.status == 200:
                return
        except OSError as error:
            last_error = error
        finally:
            try:
                conn.close()
            except UnboundLocalError:
                pass
        time.sleep(0.05)
    raise RuntimeError(f"server did not become healthy: {last_error}")


def spawn_server():
    subprocess.run(["cargo", "build", "--bin", "cachebox"], cwd=ROOT, check=True)
    admin_addr = unused_addr()
    native_addr = unused_addr()
    process = subprocess.Popen(
        [
            str(ROOT / "target" / "debug" / "cachebox"),
            "--bind",
            admin_addr,
            "--native-bind",
            native_addr,
            "--max-memory-bytes",
            "1048576",
            "--max-value-bytes",
            "1048576",
        ],
        cwd=ROOT,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
    )
    try:
        wait_for_health(admin_addr)
    except Exception:
        process.terminate()
        process.wait(timeout=5)
        raise
    return process, native_addr


def test_sync_client_core_workflow():
    process, native_addr = spawn_server()
    try:
        with Client.connect_tcp(native_addr, timeout=2) as client:
            metadata = protocol.Metadata(
                ttl=protocol.Ttl(60_000),
                stale_ttl=protocol.Ttl(60_000),
                cost=7,
                tags=("group", "blob"),
            )

            assert client.put("default", b"blob", b"cached bytes", metadata) == 0
            assert client.get("default", b"blob") == protocol.Hit(b"cached bytes")
            assert client.batch_get("default", (b"blob", b"missing")) == (
                protocol.BatchItem("hit", b"cached bytes"),
                protocol.BatchItem("miss"),
            )

            lease = client.start_lease("default", b"leased", 10_000)
            assert isinstance(lease, protocol.LeaseGranted)
            assert client.complete_lease(
                "default",
                b"leased",
                lease.lease_token,
                b"leased-value",
            ) == 0
            assert client.get("default", b"leased") == protocol.Hit(b"leased-value")

            assert client.invalidate_tag("default", "group") == 1
            assert client.get("default", b"blob") == protocol.Miss()
            assert client.delete("default", b"leased") is True

            try:
                client.get("bad namespace!", b"k")
            except ServerError as error:
                assert error.code == protocol.ErrorCode.INVALID_NAMESPACE
            else:
                raise AssertionError("invalid namespace should raise ServerError")
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_sync_client_lease_denial_is_typed():
    process, native_addr = spawn_server()
    try:
        with Client.connect_tcp(native_addr, timeout=2) as first:
            with Client.connect_tcp(native_addr, timeout=2) as second:
                granted = first.start_lease("default", b"hot-key", 10_000)
                denied = second.start_lease("default", b"hot-key", 10_000)

                assert isinstance(granted, protocol.LeaseGranted)
                assert denied == protocol.LeaseDenied()
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_sync_client_pool_reuses_multiple_connections():
    process, native_addr = spawn_server()
    try:
        with ClientPool.connect_tcp(native_addr, pool_size=2, timeout=2) as pool:
            assert pool.put("default", b"a", b"1") == 0
            assert pool.put("default", b"b", b"2") == 0
            assert pool.get("default", b"a") == protocol.Hit(b"1")
            assert pool.batch_get("default", (b"a", b"b")) == (
                protocol.BatchItem("hit", b"1"),
                protocol.BatchItem("hit", b"2"),
            )
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_async_client_core_workflow():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncClient.connect_tcp(native_addr, timeout=2) as client:
                metadata = protocol.Metadata(
                    ttl=protocol.Ttl(60_000),
                    stale_ttl=protocol.Ttl(60_000),
                    cost=7,
                    tags=("async-group", "async-blob"),
                )

                assert await client.put("default", b"async-blob", b"cached bytes", metadata) == 0
                assert await client.get("default", b"async-blob") == protocol.Hit(b"cached bytes")
                assert await client.batch_get("default", (b"async-blob", b"missing")) == (
                    protocol.BatchItem("hit", b"cached bytes"),
                    protocol.BatchItem("miss"),
                )

                lease = await client.start_lease("default", b"async-leased", 10_000)
                assert isinstance(lease, protocol.LeaseGranted)
                assert await client.complete_lease(
                    "default",
                    b"async-leased",
                    lease.lease_token,
                    b"leased-value",
                ) == 0
                assert await client.get("default", b"async-leased") == protocol.Hit(b"leased-value")

                assert await client.invalidate_tag("default", "async-group") == 1
                assert await client.get("default", b"async-blob") == protocol.Miss()
                assert await client.delete("default", b"async-leased") is True

                try:
                    await client.get("bad namespace!", b"k")
                except ServerError as error:
                    assert error.code == protocol.ErrorCode.INVALID_NAMESPACE
                else:
                    raise AssertionError("invalid namespace should raise ServerError")
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())


def test_async_client_lease_denial_is_typed():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncClient.connect_tcp(native_addr, timeout=2) as first:
                async with await AsyncClient.connect_tcp(native_addr, timeout=2) as second:
                    granted = await first.start_lease("default", b"async-hot-key", 10_000)
                    denied = await second.start_lease("default", b"async-hot-key", 10_000)

                    assert isinstance(granted, protocol.LeaseGranted)
                    assert denied == protocol.LeaseDenied()
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())


def test_async_client_pool_supports_concurrent_operations():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncClientPool.connect_tcp(
                native_addr,
                pool_size=2,
                timeout=2,
                acquire_timeout=1,
            ) as pool:
                await asyncio.gather(
                    pool.put("default", b"async-a", b"1"),
                    pool.put("default", b"async-b", b"2"),
                )
                assert await pool.get("default", b"async-a") == protocol.Hit(b"1")
                assert await pool.batch_get("default", (b"async-a", b"async-b")) == (
                    protocol.BatchItem("hit", b"1"),
                    protocol.BatchItem("hit", b"2"),
                )
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())


def test_async_client_pool_acquire_timeout_is_typed():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncClientPool.connect_tcp(
                native_addr,
                pool_size=1,
                timeout=2,
                acquire_timeout=0.01,
            ) as pool:
                async with pool.acquire():
                    try:
                        async with pool.acquire():
                            pass
                    except ClientError as error:
                        assert "timed out acquiring async client" in str(error)
                    else:
                        raise AssertionError("pool acquisition should time out")
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())


def test_async_client_cancellation_closes_in_flight_connection():
    async def run() -> None:
        request_started = asyncio.Event()

        async def handle_client(
            reader: asyncio.StreamReader,
            writer: asyncio.StreamWriter,
        ) -> None:
            await reader.readexactly(protocol.HEADER_LEN)
            request_started.set()
            try:
                await asyncio.sleep(10)
            finally:
                writer.close()
                await writer.wait_closed()

        server = await asyncio.start_server(handle_client, "127.0.0.1", 0)
        sockets = server.sockets
        assert sockets is not None
        host, port = sockets[0].getsockname()

        async with server:
            client = await AsyncClient.connect_tcp((host, port), timeout=2)
            task = asyncio.create_task(client.get("default", b"cancelled"))
            await asyncio.wait_for(request_started.wait(), timeout=2)
            task.cancel()
            try:
                await task
            except asyncio.CancelledError:
                pass
            else:
                raise AssertionError("request task should be cancelled")

            assert client.closed

    asyncio.run(run())
