# Cachebox Python Client

This package is the official Python client for Cachebox. It is being moved to
a native Python implementation so it can support normal Python sockets,
`asyncio`, gevent-compatible usage, pure Python wheels, and source
distributions without requiring a Rust toolchain.

The current package includes a pure Python native protocol codec and
synchronous socket client. Async clients, decorators, serializers, and dogpile
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
from cachebox import Client, protocol

with Client.connect_tcp("127.0.0.1:7401") as client:
    client.put("default", b"user:123", b"cached bytes")

    result = client.get("default", b"user:123")
    assert result == protocol.Hit(b"cached bytes")
```
