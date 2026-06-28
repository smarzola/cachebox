# Cachebox Python Client

This package is the official Python client for Cachebox. It is being moved to
a native Python implementation so it can support normal Python sockets,
`asyncio`, gevent-compatible usage, pure Python wheels, and source
distributions without requiring a Rust toolchain.

The current package includes a pure Python native protocol codec, synchronous
socket client, asyncio client, optional sync/async connection pools,
serializer helpers, and deterministic key builders. Decorators and dogpile
protection are implemented in follow-up native Python milestones.

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
from cachebox import AsyncClient, Client, protocol

with Client.connect_tcp("127.0.0.1:7401") as client:
    client.put("default", b"user:123", b"cached bytes")

    result = client.get("default", b"user:123")
    assert result == protocol.Hit(b"cached bytes")

async with await AsyncClient.connect_tcp("127.0.0.1:7401") as client:
    await client.put("default", b"user:456", b"cached bytes")

    result = await client.get("default", b"user:456")
    assert result == protocol.Hit(b"cached bytes")
```

High-level cache APIs build on explicit serializers and deterministic keys:

```python
from cachebox import JsonSerializer, build_function_key, make_metadata

def load_user(user_id: int, include_profile: bool = True):
    ...

serializer = JsonSerializer()
key = build_function_key(load_user, 123, prefix="users", version=1)
metadata = make_metadata(ttl_ms=60_000, tags=("users",), content_type=serializer.content_type)
payload = serializer.encode({"id": 123, "name": "Ada"})
```
