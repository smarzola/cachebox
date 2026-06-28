# Cachebox

Cachebox is a small self-hosted cache server for applications that want
explicit cache behavior without running a large service stack. It stores raw
bytes behind byte keys, adds cache metadata such as TTLs and tags, coordinates
stampede protection with leases, and exposes a compact native socket protocol
over TCP or Unix domain sockets.

The shape is intentionally direct: one Rust binary, one in-memory cache engine,
native sockets for cache traffic, and a small admin HTTP surface for health and
metrics.

## Why Cachebox

Application caches often start as a map in process and become painful when
multiple workers need shared state, TTLs, invalidation, and refresh
coordination. Cachebox is meant for that middle ground:

- You want one local or internal cache service that is easy to run.
- You cache raw bytes, not database rows or provider-specific objects.
- You need TTLs, stale reads, tags, and bounded memory.
- You want cache stampede protection for expensive recomputation.
- You want a simple protocol official clients can implement directly.
- You care about predictable resource usage on small machines.

Cachebox is useful for web fragments, generated reports, model responses,
embedding artifacts, API responses, and other application-owned bytes where the
application knows the right key and freshness policy.

## Design Principles

- Keep cache semantics explicit: freshness, staleness, invalidation, leases,
  and memory limits are visible operations rather than side effects hidden in a
  generic key/value API.
- Keep the data plane compact: cache traffic uses persistent native sockets
  with fixed binary framing and request ids.
- Keep values application-owned: Cachebox stores raw bytes and small metadata;
  serialization belongs to the client.
- Keep resource use bounded: memory has a configured cap, expiration work is
  budgeted, and metrics reads do not mutate cache state.
- Keep local operation practical: one binary should be easy to run on small
  machines, sidecars, internal tools, and development environments.

## Design

Cachebox is built around cache semantics rather than generic storage semantics.
The main data-plane operations are:

- `get`: read a fresh, stale, or missing value.
- `put`: write bytes with optional TTL, stale TTL, tags, cost, and content
  type metadata.
- `delete`: remove one key.
- `batch_get`: read many keys in one command.
- `tag_invalidate`: remove all entries with a tag in one namespace.
- `lease_start` and `lease_complete`: let exactly one client refresh a missing
  or stale value while other clients avoid duplicated work.

The native protocol keeps request framing compact and explicit. Responses echo
the request id, so a client can pipeline many requests and match responses even
when the server completes them out of order.

Internally, Cachebox uses sharded in-memory maps, bounded cleanup, approximate
LRU eviction, a routed tag directory, striped metrics counters, adaptive native
connection execution, reusable connection buffers, and response batching for
pipelined traffic. See [docs/internals.md](docs/internals.md) for the detailed
implementation walkthrough.

## Resource Profile

Cachebox is designed to fit resource-constrained environments such as a small
VPS, homelab host, sidecar-style process, or internal tool box.

Local measurements on this macOS arm64 checkout:

| Item | Observed value |
| --- | ---: |
| Optimized `cachebox` binary | `2.1 MB` |
| Idle RSS with admin TCP, native TCP, and native Unix listeners | about `2.6 MiB` |
| Default memory cap | `64 MiB` |
| Default max value size | `8 MiB` |
| Default max frame payload | `8 MiB` |

The binary and idle RSS numbers are local measurements, not portable guarantees.
The important operational point is that memory growth is bounded by the
configured cache limit plus process overhead, and expired cleanup is budgeted so
observability calls do not perform surprise reclamation work.

## Performance Highlights

Run the local benchmark harness with:

```sh
cargo run --bin cachebox-bench
```

Current local p50 highlights from [docs/benchmarks.md](docs/benchmarks.md):

| Scenario | Transport | p50 |
| --- | --- | ---: |
| Engine cached get | in-process engine | `417 ns` |
| Native Unix cached get | loopback Unix socket | `13.8 us` |
| Native TCP cached get | loopback TCP | `25.1 us` |
| Manual pipelined get, depth 32 | loopback Unix socket | `4.8 us/request` |
| Official client pipelined get, depth 32 | loopback Unix socket | `5.7 us/request` |
| Empty tag invalidation | loopback Unix socket | `14.3 us` |
| Eight-key tag invalidation | loopback Unix socket | `30.0 us` |

These numbers are local loopback measurements for comparing changes on the
same machine. They are not production latency promises.

## Quickstart

Build and run the server:

```sh
cargo build
cargo run --bin cachebox
```

Default listeners:

```text
admin HTTP: 127.0.0.1:7400
native TCP: 127.0.0.1:7401
```

Store and read bytes with the Rust native client:

```rust
use cachebox::protocol::{Metadata, Ttl};
use cachebox_client::{GetResult, NativeClient};

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
                stale_ttl: Some(Ttl {
                    milliseconds: 60_000,
                }),
                tags: vec!["user:123".to_string(), "org:9".to_string()],
                cost: Some(42),
                ..Metadata::default()
            },
            b"cached bytes".to_vec(),
        )
        .await?;

    let value = client.get("default", b"user:123".to_vec()).await?;
    assert_eq!(value, GetResult::Hit(b"cached bytes".to_vec()));

    Ok(())
}
```

Check health and metrics:

```sh
curl 'http://127.0.0.1:7400/healthz'
curl 'http://127.0.0.1:7400/metrics'
```

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

## Documentation

- [Quickstart](docs/quickstart.md)
- [Usage guide](docs/usage.md)
- [Native sockets](docs/native-sockets.md)
- [Protocol specification](docs/protocol.md)
- [Internals and performance design](docs/internals.md)
- [AI helpers](docs/ai-helpers.md)
- [Benchmarks](docs/benchmarks.md)

Internal planning notes and development checkpoints live under
[docs/internal/](docs/internal/).

## Development

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Spawned-binary smoke tests:

```sh
cargo test --test spawned_client -- --ignored
```
