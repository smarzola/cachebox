# Cachebox

Cachebox is a cache-native server for modern self-hosted applications.

The long-term idea is not to clone Redis. It is to build a fast, focused cache
engine with the features applications usually reimplement poorly around Redis:
stampede protection, stale-while-revalidate, tag invalidation, namespace quotas,
cost-aware eviction, and simple observability.

The first MVP should provide a small Redis-compatible adapter for easy adoption,
but the product should be designed around cache semantics rather than generic
data-structure semantics.

## Goals

- Run as a single small Rust binary.
- Be easy to self-host on small VPS and homelab machines.
- Keep memory bounded by default.
- Optimize for cache workloads: small keys, mixed small/medium values, TTLs,
  high concurrency, and predictable tail latency.
- Support a narrow Redis command subset for drop-in cache use cases.
- Add a native cache API for features Redis does not expose cleanly.

## Non-Goals

- Being a full Redis replacement.
- Providing durable database semantics.
- Supporting Lua, modules, streams, clustering, or general-purpose data
  structures in the MVP.
- Hiding unsupported behavior behind partial, surprising compatibility.

## MVP Shape

The MVP should include:

- RESP2 TCP listener.
- Basic Redis cache commands: `PING`, `GET`, `SET`, `DEL`, `EXISTS`, `EXPIRE`,
  `TTL`, `MGET`, `MSET`, and `FLUSHDB`.
- TTL expiration using lazy expiration plus background cleanup.
- Configurable memory limit.
- One eviction policy to start: approximate LRU or random eviction.
- Basic metrics and structured logs.
- Integration tests driven through a Redis client.
- A benchmark harness comparing common cache paths.

## Native API Direction

After the Redis-compatible MVP is useful, Cachebox should grow a native API for:

- `GET_OR_LEASE` stampede protection.
- Stale-while-revalidate responses.
- Cache tags and tag invalidation.
- Per-namespace quotas.
- Cost-aware eviction hints.
- Built-in cache diagnostics.

## Repository Layout

```text
src/
  main.rs              binary entrypoint
docs/
  architecture.md      product and system architecture
  mvp-goal-loop.md     implementation prompt and milestone loop
```

## Development

```sh
cargo fmt
cargo test
cargo run
```

The current binary is only a bootstrap placeholder. The implementation plan is
in [docs/mvp-goal-loop.md](docs/mvp-goal-loop.md).
