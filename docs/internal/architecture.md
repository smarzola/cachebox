# Cachebox Architecture

## Product Boundary

Cachebox is a cache server. It optimizes for ephemeral values, bounded memory,
low latency, explicit invalidation, and recomputation coordination.

The primary cache API is the native socket protocol. Redis compatibility,
HTTP-compatible adapters, persistence, replication, clustering, and scripting
are outside the current MVP.

## High-Level System

```text
native TCP listener / native Unix listener
  -> length-prefixed binary frame codec
  -> cache command executor
  -> cache engine
  -> binary response encoder
```

Admin HTTP is separate:

```text
admin HTTP listener
  -> /healthz
  -> /metrics
```

The server uses a fixed set of engine shards. Each shard owns its own map,
expiry index, tag index, lease state, eviction state, and lock:

```text
native connection task
  -> decode request frame
  -> key hash selects shard
  -> shard owns map, TTL index, eviction state
  -> encode response frame
```

Each shard should own its data so ordinary read and write paths avoid global
locks. Batch operations fan out through per-key shard selection. Tag
invalidation visits every shard because tag indexes are shard-local. Memory
pressure is enforced per shard; metrics aggregate shard state at scrape time.

## Current Module Baseline

- `config` parses startup options.
- `api` owns shared metadata and admin HTTP route constants.
- `protocol` owns the native frame codec and command payloads.
- `client` owns the minimal Rust native socket client.
- `engine` owns the in-memory cache implementation.
- `server` owns admin HTTP, native TCP, native Unix sockets, metrics, and the
  background expiration worker.
- `ai` owns provider-neutral helper utilities.

## Runtime

Responsibilities:

- Bind admin HTTP on `--bind`, default `127.0.0.1:7400`.
- Bind native TCP on `--native-bind`, default `127.0.0.1:7401`.
- Optionally bind native Unix sockets on Unix platforms with `--native-unix`.
- Apply native frame payload limits with `--max-body-bytes`.
- Apply value and memory limits through the engine.
- Reclaim expired entries through cache access paths, memory-pressure paths,
  and the bounded background cleanup worker.

## Native Data Plane

The native protocol uses a fixed header plus a command-specific payload. It
supports:

- Get key.
- Put key with TTL, stale TTL, tags, and optional cost metadata.
- Delete key.
- Batch get.
- Invalidate tag.
- Start lease for stampede protection.
- Complete lease with refreshed value.

Keys are byte vectors. Namespaces and tags are validated strings. Values remain
raw bytes.

The protocol is specified in [native-socket-protocol.md](native-socket-protocol.md).

## Cache Engine

The cache engine owns key/value data and metadata.

Entry shape:

```text
key: bytes
value: bytes
expires_at: optional instant
stale_until: optional instant
tags: zero or more strings
last_access: approximate timestamp or counter
memory_cost: estimated bytes
cost_score: optional unsigned integer
```

Expiration:

- Expiration lookup uses an ordered expiry index, so reclaiming expired values
  walks expired deadlines instead of scanning the full keyspace.
- A bounded background worker periodically reclaims expired entries from the
  expiry index. `--cleanup-interval-ms 0` disables it.
- Stale values can be served only when an operation explicitly allows stale
  responses.

Eviction:

- Current policy is approximate LRU.
- Writes reject a single value over `--max-value-bytes`.
- Writes reject an entry that cannot fit `--max-memory-bytes`.
- Writes reclaim expired entries before evicting live entries.
- Memory-pressure eviction uses bounded sampling rather than full-map scans on
  the hot write path.

Cost score is stored as optional entry metadata and exposed as an aggregate
accounted score for experiments. It does not affect the default eviction policy.

## Observability

The admin HTTP endpoint emits Prometheus-style metrics:

- Admin request counts.
- Native operation counts.
- Error counts.
- Cache hits, misses, and stale responses.
- Lease grants and denials.
- Expired keys.
- Evicted keys.
- Memory used and memory limit.
- Current aggregate cost score.

Scraping `/metrics` is observational. It does not reclaim expired entries and
does not increment request counters.

Startup and Ctrl-C shutdown logs are key-value text. Per-connection tracking is
not wired yet, so the connection gauge is exposed as zero until a connection
instrumentation layer lands.

## Stampede Protection

Lease start has four useful outcomes:

- Fresh value exists: return cached bytes.
- Stale value exists: return stale bytes.
- No protected refresh exists: grant a lease token.
- Another client owns the lease: deny the lease.

Lease completion accepts the lease token, refreshed bytes, and write metadata.
Lease state is process-local and in-memory.

This lets application clients use a simple flow:

1. Start lease for a cache key.
2. Return cached bytes for hit or stale responses.
3. Generate only when a lease is granted.
4. Complete the lease with fresh bytes.
5. Retry later or back off when the lease is denied.

## Future Shape

The engine should stay transport-independent. Future work can add official
clients, namespace policies, additional admin diagnostics, or optional adapters
without moving cache semantics back into HTTP routing.
