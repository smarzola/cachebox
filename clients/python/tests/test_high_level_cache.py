import asyncio
import threading
import time
from concurrent.futures import ThreadPoolExecutor

import pytest

from cachebox import (
    AsyncCachebox,
    Cachebox,
    DogpilePolicy,
    DogpileTimeoutError,
    JsonSerializer,
)
from test_client import spawn_server


def test_cachebox_high_level_workflow_uses_serializer_keys_and_tags():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=2,
            serializer=JsonSerializer(),
            key_prefix="app",
            key_version=1,
        ) as cache:
            assert cache.get("user:1", default={"missing": True}) == {"missing": True}

            assert cache.set(
                "user:1",
                {"id": 1, "name": "Ada"},
                ttl_ms=60_000,
                stale_ttl_ms=30_000,
                tags=("users",),
                cost=2,
            ) == 0
            assert cache.get("user:1") == {"id": 1, "name": "Ada"}

            calls = 0

            def load_user():
                nonlocal calls
                calls += 1
                return {"id": 2, "name": "Grace"}

            assert cache.get_or_set("user:2", load_user, tags=("users",)) == {
                "id": 2,
                "name": "Grace",
            }
            assert cache.get_or_set("user:2", load_user) == {
                "id": 2,
                "name": "Grace",
            }
            assert calls == 1

            assert cache.invalidate_tag("users") == 2
            assert cache.get("user:1") is None
            assert cache.delete("user:2") is False
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_memoize_and_cached_decorators_cache_function_results():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            serializer=JsonSerializer(),
            key_prefix="decorators",
            key_version="v1",
        ) as cache:
            memoize_calls = 0

            @cache.memoize(ttl_ms=60_000)
            def load_profile(user_id: int, include_settings: bool = True):
                nonlocal memoize_calls
                memoize_calls += 1
                return {
                    "user_id": user_id,
                    "include_settings": include_settings,
                }

            assert load_profile(7) == {"user_id": 7, "include_settings": True}
            assert load_profile(user_id=7, include_settings=True) == {
                "user_id": 7,
                "include_settings": True,
            }
            assert memoize_calls == 1

            cached_calls = 0

            @cache.cached("tenant:{tenant_id}:settings", ttl_ms=60_000)
            def load_settings(tenant_id: str):
                nonlocal cached_calls
                cached_calls += 1
                return {"tenant": tenant_id}

            assert load_settings("acme") == {"tenant": "acme"}
            assert load_settings(tenant_id="acme") == {"tenant": "acme"}
            assert cached_calls == 1
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_async_cachebox_high_level_workflow_and_decorators():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncCachebox.connect_tcp(
                native_addr,
                timeout=2,
                pool_size=2,
                serializer=JsonSerializer(),
                key_prefix="async-app",
                key_version=1,
            ) as cache:
                assert await cache.set(
                    "user:1",
                    {"id": 1, "name": "Ada"},
                    ttl_ms=60_000,
                    tags=("async-users",),
                ) == 0
                assert await cache.get("user:1") == {"id": 1, "name": "Ada"}

                calls = 0

                async def load_user():
                    nonlocal calls
                    calls += 1
                    await asyncio.sleep(0)
                    return {"id": 2, "name": "Grace"}

                assert await cache.get_or_set("user:2", load_user, tags=("async-users",)) == {
                    "id": 2,
                    "name": "Grace",
                }
                assert await cache.get_or_set("user:2", load_user) == {
                    "id": 2,
                    "name": "Grace",
                }
                assert calls == 1

                memoize_calls = 0

                @cache.memoize(ttl_ms=60_000)
                async def load_profile(user_id: int, include_settings: bool = True):
                    nonlocal memoize_calls
                    memoize_calls += 1
                    await asyncio.sleep(0)
                    return {
                        "user_id": user_id,
                        "include_settings": include_settings,
                    }

                assert await load_profile(7) == {
                    "user_id": 7,
                    "include_settings": True,
                }
                assert await load_profile(user_id=7, include_settings=True) == {
                    "user_id": 7,
                    "include_settings": True,
                }
                assert memoize_calls == 1

                cached_calls = 0

                @cache.cached(lambda tenant_id: f"tenant:{tenant_id}:settings")
                async def load_settings(tenant_id: str):
                    nonlocal cached_calls
                    cached_calls += 1
                    await asyncio.sleep(0)
                    return {"tenant": tenant_id}

                assert await load_settings("acme") == {"tenant": "acme"}
                assert await load_settings("acme") == {"tenant": "acme"}
                assert cached_calls == 1

                assert await cache.invalidate_tag("async-users") == 2
                assert await cache.get("user:1") is None
                assert await cache.delete("user:2") is False
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())


def test_cachebox_get_or_set_uses_lease_to_prevent_sync_stampede():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=8,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=2_000,
                retry_interval_ms=5,
                retry_jitter_ms=0,
            ),
        ) as cache:
            calls = 0
            calls_lock = threading.Lock()
            release_factory = threading.Event()

            def factory():
                nonlocal calls
                with calls_lock:
                    calls += 1
                release_factory.wait(timeout=2)
                return {"value": "computed"}

            with ThreadPoolExecutor(max_workers=8) as executor:
                futures = [
                    executor.submit(
                        cache.get_or_set,
                        "hot-key",
                        factory,
                        ttl_ms=60_000,
                    )
                    for _ in range(8)
                ]
                while calls == 0:
                    time.sleep(0.01)
                release_factory.set()
                assert [future.result(timeout=3) for future in futures] == [
                    {"value": "computed"}
                ] * 8

            assert calls == 1
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_get_or_set_can_opt_out_of_dogpile_protection():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=4,
            serializer=JsonSerializer(),
        ) as cache:
            calls = 0
            calls_lock = threading.Lock()
            release_factory = threading.Event()

            def factory():
                nonlocal calls
                with calls_lock:
                    calls += 1
                release_factory.wait(timeout=2)
                return {"value": "computed"}

            with ThreadPoolExecutor(max_workers=4) as executor:
                futures = [
                    executor.submit(
                        cache.get_or_set,
                        "unprotected-key",
                        factory,
                        ttl_ms=60_000,
                        dogpile=False,
                    )
                    for _ in range(4)
                ]
                while calls < 4:
                    time.sleep(0.01)
                release_factory.set()
                assert [future.result(timeout=3) for future in futures] == [
                    {"value": "computed"}
                ] * 4

            assert calls == 4
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_lease_denial_times_out_instead_of_recomputing():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=2,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=50,
                retry_interval_ms=5,
                retry_jitter_ms=0,
            ),
        ) as cache:
            key = cache.key("stuck")
            lease = cache._client.start_lease(cache.namespace, key, 5_000)
            assert lease.lease_token

            with pytest.raises(DogpileTimeoutError):
                cache.get_or_set("stuck", lambda: {"should": "not run"})
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_returns_stale_to_waiter_while_refresh_lease_is_active():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=2,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=500,
                retry_interval_ms=5,
                retry_jitter_ms=0,
                return_stale=True,
            ),
        ) as cache:
            cache.set(
                "stale-key",
                {"value": "old"},
                ttl_ms=1,
                stale_ttl_ms=60_000,
            )
            time.sleep(0.02)

            calls = 0
            refresh_started = threading.Event()
            release_refresh = threading.Event()

            def factory():
                nonlocal calls
                calls += 1
                refresh_started.set()
                release_refresh.wait(timeout=2)
                return {"value": "new"}

            with ThreadPoolExecutor(max_workers=2) as executor:
                refresh = executor.submit(
                    cache.get_or_set,
                    "stale-key",
                    factory,
                    ttl_ms=60_000,
                    stale_ttl_ms=60_000,
                )
                assert refresh_started.wait(timeout=2)
                stale = executor.submit(
                    cache.get_or_set,
                    "stale-key",
                    lambda: {"value": "should-not-run"},
                    ttl_ms=60_000,
                )
                assert stale.result(timeout=2) == {"value": "old"}
                release_refresh.set()
                assert refresh.result(timeout=2) == {"value": "new"}

            assert calls == 1
            assert cache.get("stale-key") == {"value": "new"}
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_returns_stale_value_when_refresh_factory_fails():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=2,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=500,
                retry_interval_ms=5,
                retry_jitter_ms=0,
                return_stale_on_error=True,
            ),
        ) as cache:
            cache.set(
                "stale-error-key",
                {"value": "old"},
                ttl_ms=1,
                stale_ttl_ms=60_000,
            )
            time.sleep(0.02)

            def factory():
                raise RuntimeError("origin failed")

            assert cache.get_or_set("stale-error-key", factory, ttl_ms=60_000) == {
                "value": "old"
            }
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_cachebox_can_wait_for_fresh_value_instead_of_returning_stale():
    process, native_addr = spawn_server()
    try:
        with Cachebox.connect_tcp(
            native_addr,
            timeout=2,
            pool_size=2,
            serializer=JsonSerializer(),
            dogpile=DogpilePolicy(
                lease_ttl_ms=5_000,
                wait_timeout_ms=1_000,
                retry_interval_ms=5,
                retry_jitter_ms=0,
                return_stale=False,
            ),
        ) as cache:
            cache.set(
                "wait-fresh-key",
                {"value": "old"},
                ttl_ms=1,
                stale_ttl_ms=60_000,
            )
            time.sleep(0.02)

            refresh_started = threading.Event()
            release_refresh = threading.Event()

            def factory():
                refresh_started.set()
                release_refresh.wait(timeout=2)
                return {"value": "new"}

            with ThreadPoolExecutor(max_workers=2) as executor:
                refresh = executor.submit(
                    cache.get_or_set,
                    "wait-fresh-key",
                    factory,
                    ttl_ms=60_000,
                    stale_ttl_ms=60_000,
                )
                assert refresh_started.wait(timeout=2)
                waiter = executor.submit(
                    cache.get_or_set,
                    "wait-fresh-key",
                    lambda: {"value": "should-not-run"},
                    ttl_ms=60_000,
                )
                time.sleep(0.02)
                assert not waiter.done()
                release_refresh.set()
                assert refresh.result(timeout=2) == {"value": "new"}
                assert waiter.result(timeout=2) == {"value": "new"}
    finally:
        process.terminate()
        process.wait(timeout=5)


def test_async_cachebox_get_or_set_uses_lease_to_prevent_stampede():
    async def run() -> None:
        process, native_addr = spawn_server()
        try:
            async with await AsyncCachebox.connect_tcp(
                native_addr,
                timeout=2,
                pool_size=8,
                serializer=JsonSerializer(),
                dogpile=DogpilePolicy(
                    lease_ttl_ms=5_000,
                    wait_timeout_ms=2_000,
                    retry_interval_ms=5,
                    retry_jitter_ms=0,
                ),
            ) as cache:
                calls = 0
                release_factory = asyncio.Event()

                async def factory():
                    nonlocal calls
                    calls += 1
                    await release_factory.wait()
                    return {"value": "computed"}

                tasks = [
                    asyncio.create_task(
                        cache.get_or_set(
                            "async-hot-key",
                            factory,
                            ttl_ms=60_000,
                        )
                    )
                    for _ in range(8)
                ]
                while calls == 0:
                    await asyncio.sleep(0.01)
                release_factory.set()
                assert await asyncio.gather(*tasks) == [{"value": "computed"}] * 8
                assert calls == 1
        finally:
            process.terminate()
            process.wait(timeout=5)

    asyncio.run(run())
