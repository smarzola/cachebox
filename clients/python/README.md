# Cachebox Python Client

This package is the official Python client for Cachebox. It is being moved to
a native Python implementation so it can support normal Python sockets,
`asyncio`, gevent-compatible usage, pure Python wheels, and source
distributions without requiring a Rust toolchain.

The current package includes a pure Python native protocol codec, synchronous
socket client, asyncio client, optional sync/async connection pools,
serializer helpers, deterministic key builders, and high-level sync/async
caching APIs. Dogpile protection is implemented in a follow-up native Python
milestone.

## Local Development

Run the Python tests from the repository root with the local package built and
installed into the same environment as pytest:

```sh
uv run --with pytest --with clients/python pytest clients/python/tests
```

Build the source distribution and universal Python wheel:

```sh
uv build clients/python
```

## Example

```python
from cachebox import AsyncCachebox, Cachebox, JsonSerializer

with Cachebox.connect_tcp(
    "127.0.0.1:7401",
    serializer=JsonSerializer(),
    key_prefix="app",
    key_version=1,
) as cache:
    cache.set("user:123", {"id": 123, "name": "Ada"}, ttl_ms=60_000, tags=("users",))

    result = cache.get("user:123")
    assert result == {"id": 123, "name": "Ada"}

    @cache.memoize(ttl_ms=60_000, tags=("users",))
    def load_user(user_id: int):
        return {"id": user_id}

    assert load_user(456) == {"id": 456}

async with await AsyncCachebox.connect_tcp(
    "127.0.0.1:7401",
    serializer=JsonSerializer(),
) as cache:
    await cache.set("user:789", {"id": 789}, ttl_ms=60_000)

    result = await cache.get("user:789")
    assert result == {"id": 789}
```

Low-level clients and high-level cache APIs build on explicit serializers and
deterministic keys:

```python
from cachebox import JsonSerializer, build_function_key, make_metadata

def load_user(user_id: int, include_profile: bool = True):
    ...

serializer = JsonSerializer()
key = build_function_key(load_user, 123, prefix="users", version=1)
metadata = make_metadata(ttl_ms=60_000, tags=("users",), content_type=serializer.content_type)
payload = serializer.encode({"id": 123, "name": "Ada"})
```
