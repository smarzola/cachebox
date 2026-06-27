# Cachebox

Cachebox is a self-hosted cache server for applications that need explicit
cache behavior over a simple HTTP API. It stores raw-byte values, attaches cache
metadata at write time, coordinates expensive refreshes with leases, invalidates
related entries with tags, keeps memory bounded, and exposes Prometheus-style
metrics.

The project is written in Rust and runs as a single binary. The current server
accepts HTTP/2 cleartext and HTTP/1.1 for local tooling.

## Design Principles

- **Cache semantics first.** Reads, writes, TTLs, stale windows, tags, leases,
  memory limits, and metrics are first-class operations.
- **Raw bytes in the data path.** Values are stored and returned as bytes; JSON
  is used only for small control envelopes such as batch reads and lease state.
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
- HTTP API for get, put, delete, batch get, tag invalidation, lease start, and
  lease completion.
- Percent-encoded byte keys under named namespaces.
- Raw-byte values.
- Fresh TTL and stale TTL metadata.
- Tag-based invalidation.
- Lease-based stampede protection for expensive recomputation.
- Approximate memory accounting with max body, max value, and max memory limits.
- Approximate LRU eviction.
- Prometheus-style `/metrics`.
- Reserved `Cachebox-Cost` metadata with live aggregate cost-score metrics.
- Rust AI helper utilities:
  - prompt/result cache keys
  - embedding cache keys
  - generation lease decisions
  - buffer-then-commit stream capture
- Local benchmark harness.

## Quickstart

Build:

```sh
cargo build
```

Run:

```sh
cargo run --bin cachebox -- --bind 127.0.0.1:7400
```

Store a value:

```sh
curl --http1.1 -i \
  -X PUT 'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123' \
  -H 'Cachebox-TTL: 300s' \
  -H 'Cachebox-Tags: user:123,org:9' \
  -H 'Cachebox-Cost: 42' \
  -H 'Content-Type: application/octet-stream' \
  --data-binary 'cached bytes'
```

Read it:

```sh
curl --http1.1 -i \
  'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123'
```

Delete it:

```sh
curl --http1.1 -i \
  -X DELETE 'http://127.0.0.1:7400/v1/namespaces/default/keys/user%3A123'
```

Check metrics:

```sh
curl --http1.1 'http://127.0.0.1:7400/metrics'
```

More examples are in [docs/quickstart.md](docs/quickstart.md) and
[docs/usage.md](docs/usage.md).

## Configuration

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --max-body-bytes 8388608 \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608
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
tag invalidation, TTL-heavy writes, eviction pressure, and cost-shaped writes.
Current baseline output and scenario descriptions are in
[docs/benchmarks.md](docs/benchmarks.md).

## Documentation

User-facing docs:

- [Documentation index](docs/README.md)
- [Quickstart](docs/quickstart.md)
- [Usage guide](docs/usage.md)
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
  api.rs               HTTP API route and metadata parsing
  config.rs            CLI and startup configuration parsing
  engine.rs            in-memory cache engine
  lib.rs               library module exports
  main.rs              binary entrypoint
  operation.rs         typed cache operation parser
  server.rs            HTTP server
docs/
  README.md            documentation index
  quickstart.md        first-run guide
  usage.md             user-facing API examples
  ai-helpers.md        AI helper examples
  benchmarks.md        benchmark command and baseline
  internal/            planning, architecture, and historical notes
tests/
  spawned_client.rs    spawned-binary smoke tests
```
