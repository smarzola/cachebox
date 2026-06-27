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

## Core Components

### HTTP Runtime

Use `tokio` plus a Rust HTTP stack that supports HTTP/2 cleanly. The exact
framework should be chosen during implementation, but the transport should keep
raw-byte bodies efficient and avoid forcing JSON for cached values.

Responsibilities:

- Bind an HTTP address.
- Serve HTTP/2 by default and allow HTTP/1.1 for local tooling if cheap.
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

Value rules:

- Cache values are raw bytes.
- Metadata travels in headers for simple operations.
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
- Background expiration pass.
- TTL lookup should not require scanning the full map.
- Stale values can be served only when an operation explicitly allows stale
  responses.

Eviction:

- MVP can use simple random or approximate LRU eviction.
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

## Open Design Questions

- Which Rust HTTP stack should the MVP use?
- Should local development allow HTTP/1.1 by default alongside HTTP/2?
- Should batch endpoints start as JSON or use a compact binary format
  immediately?
- Which first official clients should be written: TypeScript, Python, Rust, or
  Go?
- Should eviction start with random eviction for simplicity or approximate LRU
  for better cache behavior?
