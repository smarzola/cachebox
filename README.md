# Cachebox

Cachebox is a cache-native server for modern self-hosted applications.

The idea is not to clone Redis. It is to build a fast, focused cache engine with
the features applications usually reimplement poorly around generic stores:
stampede protection, stale-while-revalidate, tag invalidation, namespace quotas,
cost-aware eviction, and simple observability.

The MVP is HTTP/2-first. It should expose a native cache API with raw-byte values
and structured metadata instead of inheriting Redis protocol constraints. Redis
compatibility is only a possible future adapter.

## Goals

- Run as a single small Rust binary.
- Be easy to self-host on small VPS and homelab machines.
- Keep memory bounded by default.
- Optimize for cache workloads: small keys, mixed small/medium values, TTLs,
  high concurrency, and predictable tail latency.
- Provide simple clients through a conventional HTTP API.
- Support cache-native features Redis does not expose cleanly.

## Non-Goals

- Being a full Redis replacement.
- Providing durable database semantics.
- Supporting Lua, modules, streams, clustering, or general-purpose data
  structures in the MVP.
- Implementing Redis/RESP compatibility in the MVP.
- Hiding cache semantics behind generic data-structure commands.

## MVP Shape

The MVP should include:

- HTTP/2 API for cache reads, writes, deletes, batches, tag invalidation, and
  leases.
- Raw-byte cache values with metadata carried in headers or JSON envelopes.
- TTL expiration using lazy expiration plus background cleanup.
- Configurable memory limit.
- One eviction policy to start: approximate LRU or random eviction.
- Basic metrics and structured logs.
- Integration tests driven through an HTTP client.
- A benchmark harness comparing common cache paths.

## Native API Direction

Cachebox should expose first-class cache operations:

- Lease-based stampede protection.
- Stale-while-revalidate responses.
- Cache tags and tag invalidation.
- Per-namespace quotas.
- Cost-aware eviction hints.
- Built-in cache diagnostics.

The planned HTTP server stack is `axum` on `tokio`/Hyper. The current codebase
defines and tests the API contract before adding the listener dependency.

Example data-plane requests:

```http
PUT /v1/namespaces/default/keys/user%3A123
Cachebox-TTL: 300s
Cachebox-Tags: user:123,org:9
Content-Type: application/octet-stream

<raw bytes>
```

```http
GET /v1/namespaces/default/keys/user%3A123
```

Example coordination request:

```http
POST /v1/namespaces/default/leases/user%3A123
Content-Type: application/json

{"lease_ttl_ms":10000,"allow_stale_ms":60000}
```

## Repository Layout

```text
src/
  api.rs               HTTP API route constants and future routing
  config.rs            no-dependency CLI and startup configuration parsing
  engine.rs            cache engine boundary
  lib.rs               library module exports
  main.rs              binary entrypoint
  operation.rs         typed cache operation boundary
  server.rs            server startup boundary
docs/
  checkpoints.md       clean checkpoint notes from the MVP loop
  benchmarks.md        local benchmark command and baseline
  supported-behavior.md supported and unsupported MVP behavior
  architecture.md      product and system architecture
  mvp-goal-loop.md     implementation prompt and milestone loop
  redis-adapter.md     possible future Redis compatibility layer
```

## Development

```sh
cargo fmt
cargo test
cargo run --bin cachebox -- --bind 127.0.0.1:7400 --max-memory-bytes 67108864 --max-value-bytes 8388608
cargo run --bin cachebox-bench
cargo run --bin cachebox -- --help
```

The binary starts the local HTTP server. The API and operation parsers cover MVP
route shapes, percent-encoded byte keys, PUT metadata headers, typed cache
operations, raw byte value bodies, and deterministic error envelopes. The
in-memory engine supports byte values, TTL, stale TTL, deletes, batch get, and
tag invalidation. Memory is bounded with approximate accounting, a max value
size, and approximate LRU eviction. `/metrics` exposes Prometheus-style process
metrics for requests, cache outcomes, errors, memory, expirations, and
evictions. Lease start and completion provide the first stampede-protection
path. Benchmark baselines and the reproducible command live in
[docs/benchmarks.md](docs/benchmarks.md). Supported and unsupported behavior is
documented in [docs/supported-behavior.md](docs/supported-behavior.md). The
implementation plan is in
[docs/mvp-goal-loop.md](docs/mvp-goal-loop.md), with completed checkpoints in
[docs/checkpoints.md](docs/checkpoints.md).
