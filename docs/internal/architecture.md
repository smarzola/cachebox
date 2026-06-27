# Cachebox Architecture

## Product Boundary

Cachebox is a cache server, not a database. It should optimize for ephemeral
values, bounded memory, low latency, explicit invalidation, and recomputation
coordination. The primary API is native HTTP/2; Redis compatibility is a
possible future adapter, not an MVP constraint.

The MVP should be intentionally small:

- HTTP/2 server.
- Namespaced in-memory keyspace.
- Byte-string keys and values.
- TTL and stale TTL metadata.
- Bounded memory.
- A small cache operation set.
- No persistence.
- No replication.
- No scripting.

## High-Level System

```text
HTTP/2 listener
  -> request router
  -> auth and namespace resolution
  -> cache operation decoder
  -> cache engine
  -> response encoder
```

The initial implementation can use a single shared engine protected by a lock if
that makes correctness faster to reach. The performance-oriented design should
move to sharded ownership:

```text
HTTP server
  -> request tasks decode cache operations
  -> key hash selects shard
  -> shard owns map, TTL index, eviction state
  -> response returns to request task
```

Each shard should own its data so ordinary read and write paths avoid global
locks. Batch operations can fan out to multiple shards.

## Current Module Baseline

The repository starts with narrow module boundaries that match the MVP plan:

- `config` parses startup options without adding a CLI dependency yet.
- `api` owns HTTP route constants and will grow route parsing.
- `operation` owns typed cache operation definitions.
- `engine` owns the in-memory cache implementation boundary.
- `server` owns startup behavior and will grow the Tokio HTTP listener.

The binary is intentionally runnable before networking exists. It prints the
resolved startup configuration and exits successfully.

## Core Components

### HTTP Runtime

Use `tokio` plus `axum` on Hyper. That stack supports HTTP/2 cleanly while
keeping raw-byte bodies efficient and avoiding JSON requirements for cached
values.

The current binary starts this stack with a shared in-memory engine protected by
a mutex. That is intentionally simple for correctness while the MVP behavior is
still being established.

Responsibilities:

- Bind an HTTP address.
- Serve HTTP/2 cleartext by default.
- Enforce max connection and request limits.
- Preserve request and response bodies as bytes.
- Apply request body size limits.
- Apply request deadlines where appropriate.

### API Layer

The data plane uses HTTP paths, headers, and raw-byte bodies.

Initial endpoints:

- `GET /v1/namespaces/{namespace}/keys/{key}`
- `PUT /v1/namespaces/{namespace}/keys/{key}`
- `DELETE /v1/namespaces/{namespace}/keys/{key}`
- `POST /v1/namespaces/{namespace}/batch/get`
- `POST /v1/namespaces/{namespace}/tags/{tag}/invalidate`
- `POST /v1/namespaces/{namespace}/leases/{key}`
- `PUT /v1/namespaces/{namespace}/leases/{key}/complete`
- `GET /healthz`
- `GET /metrics`

The current contract parser accepts those route shapes without opening sockets.
Key and tag path segments are percent-decoded into raw bytes so cache values and
keys do not depend on UTF-8. Namespaces are intentionally ASCII-only for the
MVP.

Value rules:

- Cache values are raw bytes.
- Metadata travels in headers for simple operations.
- PUT metadata headers are `Cachebox-TTL`, `Cachebox-Stale-TTL`,
  `Cachebox-Tags`, `Cachebox-Cost`, and `Content-Type`.
- Structured states such as lease responses use JSON.
- Keys in paths must be percent-encoded.
- Batch endpoints use JSON request bodies initially and may gain a binary format
  later.

### Operation Layer

The operation layer maps HTTP requests to typed cache operations.

MVP operation set:

- Get key.
- Put key with TTL, stale TTL, tags, and optional cost metadata.
- Delete key.
- Batch get.
- Invalidate tag.
- Start lease for stampede protection.
- Complete lease with refreshed value.

The current parser validates method, path, headers, and control bodies before
building operations. Raw value bodies are preserved as bytes for PUT and lease
completion. Batch and lease-start bodies use small JSON control envelopes.
The HTTP handler executes get, put, delete, batch get, tag invalidation, lease
start, and lease completion against the engine. Lease behavior is process-local
and in-memory: concurrent misses grant one lease, stale values are served while a
refresh lease is active, and expired leases can be reacquired.

API principle:

- Make cache semantics explicit.
- Keep value transport byte-oriented.
- Use JSON only where structured control responses are useful.
- Add endpoints only when the engine has tests for the underlying behavior.

### Cache Engine

The cache engine owns key/value data and metadata.

Entry shape:

```text
key: bytes
value: bytes
expires_at: optional instant
stale_until: optional instant
tags: zero or more byte/string tags
last_access: approximate timestamp or counter
memory_cost: estimated bytes
```

Expiration:

- Lazy expiration on reads and writes.
- Expiration lookup uses an ordered expiry index, so reclaiming expired values
  walks expired deadlines instead of scanning the full keyspace.
- A bounded background worker periodically reclaims expired entries from the
  expiry index. `--cleanup-interval-ms 0` disables it.
- Stale values can be served only when an operation explicitly allows stale
  responses.

The current engine implements the in-memory path with an injectable clock. It
stores byte keys and values per namespace, returns distinct fresh, stale, and
miss outcomes, removes expired entries lazily, and keeps tag indexes scoped by
namespace. It also tracks estimated memory use and enforces configured memory
and value-size limits. `Cachebox-Cost` is stored as optional entry metadata and
exposed as an aggregate accounted cost score for experiments, but it does not
affect the default eviction policy.

Eviction:

- MVP uses approximate LRU eviction.
- Later versions should support cost-aware policies:
  - access recency
  - access frequency
  - value size
  - recomputation cost
  - namespace priority
  - expiry distance

Memory accounting:

- Track estimated bytes per entry.
- Enforce a hard configured memory cap.
- Refuse or evict before accepting writes that exceed the cap.
- Add a maximum value size.

Current accounting is intentionally approximate. Each entry counts namespace,
key, value, tag bytes, and a fixed per-entry overhead. Writes reject a single
value over `--max-value-bytes`, reject an entry that cannot fit
`--max-memory-bytes`, reclaim expired entries, and then evict least-recently-used
live entries until the new value fits. The implementation keeps an ordered
expiry index and uses bounded-sample approximate LRU for memory-pressure
eviction, avoiding full-map scans on the hot write path.

### Observability

MVP observability should include:

- Structured logs.
- Request counts by endpoint and operation.
- Error counts.
- Cache hits and misses.
- Stale responses served.
- Lease grants and denials.
- Expired keys.
- Evicted keys.
- Memory used and memory limit.
- Current connection count.

Expose metrics through HTTP. A Prometheus-compatible `/metrics` endpoint is the
preferred default.

The current `/metrics` endpoint emits Prometheus-style text backed by handler
counters and engine stats. It is observational: scraping metrics does not
reclaim expired entries or increment request counters. Startup and Ctrl-C
shutdown logs are key-value text. Per-connection tracking is not wired yet, so
the connection gauge is exposed as zero until a connection instrumentation layer
lands.

## Cache API Semantics

Native features should be designed around common cache failure modes.

### Stampede Protection

Proposed operation:

```http
POST /v1/namespaces/default/leases/user%3A123
Content-Type: application/json

{"lease_ttl_ms":10000,"allow_stale_ms":60000}
```

Responses:

- Fresh value.
- Stale value with refresh already leased.
- Lease token allowing this client to recompute.
- Miss without lease if the namespace is overloaded.

### Stale-While-Revalidate

Entries can have:

- Fresh TTL.
- Stale TTL.

After fresh TTL expires, Cachebox can serve stale values while one client
refreshes the entry.

### Tags

Native writes can attach tags:

```http
PUT /v1/namespaces/default/keys/dashboard%3A42
Cachebox-TTL: 5m
Cachebox-Tags: user:42,workspace:abc
Content-Type: application/octet-stream

<raw bytes>
```

This requires a reverse index from tag to keys and careful cleanup when entries
expire or are evicted.

### Namespaces

Namespaces allow one Cachebox instance to serve multiple apps or tenants.

Each namespace can define:

- Default TTL.
- Memory quota.
- Max key/value size.
- Eviction policy.
- Stale behavior.

## Security Defaults

- Bind to `127.0.0.1` by default.
- Require explicit configuration for public interfaces.
- Support simple token auth before broad deployment.
- Enforce max request size and max value size.
- Avoid panic paths on malformed client input.

## Performance Principles

- Optimize single-key hot paths first.
- Measure p99 latency, not only throughput.
- Avoid global locks on reads in the sharded design.
- Keep values as bytes and avoid unnecessary copies.
- Make memory accounting cheap and approximate enough to stay fast.
- Prefer predictable bounded work per request.

The current benchmark harness is `cargo run --bin cachebox-bench`. It measures
loopback HTTP scenarios and documents local baseline output in
`docs/internal/benchmarks.md`.

## Open Design Questions

- Which first official clients should be written: TypeScript, Python, Rust, or
  Go?
- Should batch endpoints gain a compact binary format after the JSON MVP?
- When should namespace-specific quotas and auth be introduced?
