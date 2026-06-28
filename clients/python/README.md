# Cachebox Python Client

This package is the official Python client for Cachebox. It is being moved to
a native Python implementation so it can support normal Python sockets,
`asyncio`, gevent-compatible usage, pure Python wheels, and source
distributions without requiring a Rust toolchain.

The current package includes a pure Python native protocol codec, synchronous
socket client, asyncio client, optional sync/async connection pools,
serializer helpers, deterministic key builders, and high-level sync/async
caching APIs with lease-backed dogpile protection.

## Local Development

Install the package in editable mode:

```sh
uv pip install -e clients/python
```

Run the Python tests from the repository root with the local package built and
installed into the same environment as pytest:

```sh
uv run --with pytest --with clients/python pytest clients/python/tests
```

Run the optional gevent compatibility test:

```sh
uv run --with pytest --with gevent --with clients/python pytest clients/python/tests/test_gevent.py
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

`get_or_set`, `memoize`, and `cached` use Cachebox leases by default so
concurrent misses do not stampede the origin. The caller that receives the
lease computes and completes it; callers denied a lease wait and retry until a
cached value appears or the dogpile wait timeout expires. If a stale value is
available while another caller holds a refresh lease, it is returned by
default. A caller that receives a refresh lease computes and stores the new
value; if that refresh raises and the server supplied a stale value, the stale
value is returned by default. Cachebox currently has no lease-abort command, so
failed refresh leases expire according to `lease_ttl_ms`.

## gevent

The synchronous client uses Python socket APIs, so it can cooperate with gevent
when applications monkey patch before opening Cachebox connections:

```python
from gevent import monkey

monkey.patch_all()

from cachebox import Cachebox
```

Patch before importing application modules that create clients or pools. The
default Python package does not start a Rust or Tokio runtime, so gevent is not
bypassed by native extension work in normal installations.

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

## Release Readiness

The Python package is pure Python by default. Normal wheel or source
distribution installation does not require a Rust toolchain. Repository release
metadata is managed by automation; do not edit generated changelog, version
tag, or release artifact metadata manually.
