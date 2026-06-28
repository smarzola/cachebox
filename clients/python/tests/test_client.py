import http.client
import socket
import subprocess
import time
from pathlib import Path

from cachebox import Client, GetResult, Metadata, ServerError


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


def test_python_client_core_workflow():
    process, native_addr = spawn_server()
    try:
        client = Client.connect_tcp(native_addr)
        metadata = Metadata(
            ttl_ms=60_000,
            stale_ttl_ms=60_000,
            cost=7,
            tags=["group", "blob"],
        )

        assert client.put("default", b"blob", b"cached bytes", metadata) == 0
        assert client.get("default", b"blob") == GetResult.hit(b"cached bytes")
        assert client.batch_get("default", [b"blob", b"missing"]) == [
            GetResult.hit(b"cached bytes"),
            GetResult.miss(),
        ]

        lease = client.start_lease("default", b"leased", 10_000)
        assert lease.state == "lease_granted"
        assert lease.lease_token is not None
        assert (
            client.complete_lease(
                "default",
                b"leased",
                lease.lease_token,
                b"leased-value",
            )
            == 0
        )
        assert client.get("default", b"leased") == GetResult.hit(b"leased-value")

        assert client.invalidate_tag("default", "group") == 1
        assert client.get("default", b"blob") == GetResult.miss()
        assert client.delete("default", b"leased") is True

        try:
            client.get("bad namespace!", b"k")
        except ServerError as error:
            assert "InvalidNamespace" in str(error)
        else:
            raise AssertionError("invalid namespace should raise ServerError")
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_lease_denial_is_typed():
    process, native_addr = spawn_server()
    try:
        first = Client.connect_tcp(native_addr)
        second = Client.connect_tcp(native_addr)

        granted = first.start_lease("default", b"hot-key", 10_000)
        denied = second.start_lease("default", b"hot-key", 10_000)

        assert granted.state == "lease_granted"
        assert denied.state == "lease_denied"
    finally:
        process.terminate()
        process.wait(timeout=5)
