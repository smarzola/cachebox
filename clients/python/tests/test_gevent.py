import pytest

gevent = pytest.importorskip("gevent")
monkey = pytest.importorskip("gevent.monkey")

monkey.patch_all()

from cachebox import Cachebox, DogpilePolicy, JsonSerializer
from test_client import spawn_server


def test_sync_cachebox_cooperates_with_gevent_monkey_patched_sockets():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=4,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=2_000,
                retry_interval_ms=5,
                retry_jitter_ms=0,
            ),
        ) as cache:
            calls = 0
            release_factory = gevent.event.Event()

            def factory():
                nonlocal calls
                calls += 1
                release_factory.wait(timeout=2)
                return {"value": "computed"}

            greenlets = [
                gevent.spawn(
                    cache.get_or_set,
                    "gevent-hot-key",
                    factory,
                    ttl_ms=60_000,
                )
                for _ in range(4)
            ]

            while calls == 0:
                gevent.sleep(0.01)
            release_factory.set()
            gevent.joinall(greenlets, timeout=3, raise_error=True)

            assert [greenlet.value for greenlet in greenlets] == [
                {"value": "computed"}
            ] * 4
            assert calls == 1
            assert cache.get("gevent-hot-key") == {"value": "computed"}
    finally:
        process.terminate()
        process.wait(timeout=5)
