# Cachebox Python Client

This package is the official Python client for Cachebox. It is being moved to
a native Python implementation so it can support normal Python sockets,
`asyncio`, gevent-compatible usage, pure Python wheels, and source
distributions without requiring a Rust toolchain.

The current package is a pure Python skeleton. The protocol codec, sync and
async clients, connection pools, decorators, serializers, and dogpile
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
import cachebox

assert cachebox.__version__ == "0.1.0"
```
