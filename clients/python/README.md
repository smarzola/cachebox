# Cachebox Python Client

This package provides the official Python client for Cachebox. It wraps the
Rust `cachebox-client` crate, so Python callers use the same native protocol
implementation as the Rust client.

## Local Development

Build and install the package in editable mode:

```sh
uv run --with maturin maturin develop --manifest-path clients/python/Cargo.toml
```

Check the Rust extension crate:

```sh
cargo check -p cachebox-python
```

Run the Python tests from the repository root with the local package built and
installed into the same environment as pytest:

```sh
uv run --with pytest --with clients/python pytest clients/python/tests
```

## Example

```python
from cachebox import Client, GetResult, Metadata, ai_prompt_cache_key

client = Client.connect_tcp("127.0.0.1:7401")
client.put("default", b"user:123", b"cached bytes", Metadata(ttl_ms=300_000))

result = client.get("default", b"user:123")
assert result == GetResult.hit(b"cached bytes")

key = ai_prompt_cache_key(
    "openai",
    "gpt-example",
    "workspace-a",
    [{"role": "user", "content": "Summarize Cachebox."}],
)
assert key.startswith(b"ai:prompt:v1:")
```
