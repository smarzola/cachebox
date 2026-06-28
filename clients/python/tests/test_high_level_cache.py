import asyncio

from cachebox import AsyncCachebox, Cachebox, JsonSerializer
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
