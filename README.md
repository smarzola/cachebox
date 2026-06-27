# Cachebox

Cachebox is a self-hosted cache server for applications that need explicit
cache semantics: raw-byte values, TTLs, stale windows, tag invalidation,
bounded memory, metrics, and lease-based stampede protection.

The server runs as a single Rust binary. Cache operations use Cachebox's native
socket protocol over TCP by default and Unix domain sockets when configured.
HTTP is kept as an admin surface for health and Prometheus-style metrics.

## Design Principles

- **Cache semantics first.** Reads, writes, TTLs, stale windows, tags, leases,
  memory limits, and metrics are first-class operations.
- **Native data plane.** Cache operations use a compact binary protocol instead
  of HTTP routing, headers, and JSON envelopes.
- **Raw bytes in the data path.** Values are stored and returned as bytes.
  Structured metadata stays small and explicit.
- **Self-hostable by default.** The binary should be easy to run on a laptop,
  VPS, homelab machine, or small internal service.
- **Bounded memory.** Cachebox accounts for approximate entry memory, enforces
  configured limits, and evicts approximate least-recently-used entries under
  pressure.
- **Observable behavior.** Cache outcomes, errors, leases, expirations,
  evictions, memory, and cost-score metadata are visible through `/metrics`.
- **Provider-neutral helpers.** AI-oriented helpers build deterministic cache
  keys and generation flows without calling model providers.

## Features

- Single `cachebox` server binary.
- Native TCP cache data plane enabled by default on `127.0.0.1:7401`.
- Optional native Unix socket data plane.
- Admin HTTP health and metrics on `127.0.0.1:7400`.
- Get, put, delete, batch get, tag invalidation, lease start, and lease
  completion.
- Namespaced byte keys and raw-byte values.
- Fresh TTL and stale TTL metadata.
- Tag-based invalidation.
- Lease-based stampede protection for expensive recomputation.
- Approximate memory accounting with max body, max value, and max memory limits.
- Approximate LRU eviction.
- Prometheus-style `/metrics`.
- Reserved cost metadata with side-effect-free aggregate cost-score metrics.
- Rust AI helper utilities for prompt keys, embedding keys, generation leases,
  and buffer-then-commit stream capture.
- Local benchmark harness for engine, native TCP, and native Unix socket paths.

## Quickstart

Build:

```sh
cargo build
```

Run:

```sh
cargo run --bin cachebox
```

The default listeners are:

```text
admin HTTP: 127.0.0.1:7400
native TCP: 127.0.0.1:7401
```

Use the Rust native client:

```rust
use cachebox::api::Ttl;
use cachebox::client::NativeClient;
use cachebox::protocol::{Metadata, ResponsePayload};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = NativeClient::connect_tcp("127.0.0.1:7401").await?;

    client
        .put(
            "default",
            b"user:123".to_vec(),
            Metadata {
                ttl: Some(Ttl {
                    milliseconds: 300_000,
                }),
                tags: vec!["user:123".to_string(), "org:9".to_string()],
                cost: Some(42),
                ..Metadata::default()
            },
            b"cached bytes".to_vec(),
        )
        .await?;

    let value = client.get("default", b"user:123".to_vec()).await?;
    assert_eq!(value, ResponsePayload::Hit(b"cached bytes".to_vec()));

    assert!(client.delete("default", b"user:123".to_vec()).await?);

    Ok(())
}
```

Check admin metrics:

```sh
curl 'http://127.0.0.1:7400/metrics'
```

More examples are in [docs/quickstart.md](docs/quickstart.md) and
[docs/usage.md](docs/usage.md).

## Configuration

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --native-bind 127.0.0.1:7401 \
  --native-unix /tmp/cachebox.sock \
  --max-body-bytes 8388608 \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

Show all options:

```sh
cargo run --bin cachebox -- --help
```

## Benchmarks

Run the local benchmark harness:

```sh
cargo run --bin cachebox-bench
```

The harness covers cached hits, unique writes, batch reads, lease contention,
tag invalidation, TTL-heavy writes, eviction pressure, and cost-shaped writes
across engine, native TCP, and native Unix socket paths. Current baseline output
and scenario descriptions are in [docs/benchmarks.md](docs/benchmarks.md).

## Documentation

User-facing docs:

- [Documentation index](docs/README.md)
- [Quickstart](docs/quickstart.md)
- [Usage guide](docs/usage.md)
- [Native sockets](docs/native-sockets.md)
- [AI helpers](docs/ai-helpers.md)
- [Benchmarks](docs/benchmarks.md)

Internal docs:

- [Internal docs](docs/internal/)

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Ignored spawned-binary smoke tests can be run explicitly:

```sh
cargo test --test spawned_client -- --ignored
```

## Repository Layout

```text
src/
  ai.rs                AI-oriented helper utilities
  api.rs               shared metadata and admin HTTP route constants
  client.rs            native socket client
  config.rs            CLI and startup configuration parsing
  engine.rs            in-memory cache engine
  lib.rs               library module exports
  main.rs              binary entrypoint
  protocol.rs          native socket protocol codec
  server.rs            admin HTTP and native socket server
docs/
  README.md            documentation index
  quickstart.md        first-run guide
  usage.md             user-facing native client examples
  native-sockets.md    native socket protocol and client usage
  ai-helpers.md        AI helper examples
  benchmarks.md        benchmark command and baseline
  internal/            planning, architecture, and historical notes
tests/
  spawned_client.rs    spawned-binary smoke tests
```
