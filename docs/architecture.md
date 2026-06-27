# Cachebox Architecture

## Product Boundary

Cachebox is a cache server, not a database. It should optimize for ephemeral
values, bounded memory, low latency, and explicit invalidation. Redis
compatibility is an adoption layer; native cache features are the differentiator.

The MVP should be intentionally small:

- RESP2 wire protocol.
- One in-memory keyspace.
- Byte-string keys and values.
- TTL metadata.
- Bounded memory.
- A small command set.
- No persistence.
- No replication.
- No scripting.

## High-Level System

```text
TCP listener
  -> connection task
  -> RESP parser
  -> command router
  -> cache engine
  -> RESP encoder
  -> socket writer
```

The initial implementation can use a single shared engine protected by a lock if
that makes correctness faster to reach. The performance-oriented design should
move to sharded ownership:

```text
accept loop
  -> connection tasks parse requests
  -> key hash selects shard
  -> shard owns map, TTL index, eviction state
  -> response returns to connection task
```

Each shard should own its data so ordinary `GET` and `SET` paths avoid global
locks. Cross-key commands such as `MGET` can fan out to multiple shards.

## Core Components

### Network Runtime

Use `tokio` for the MVP. It provides enough performance and keeps the codebase
approachable. Revisit lower-level networking only after benchmark data shows
Tokio overhead dominates.

Responsibilities:

- Bind TCP address.
- Accept connections.
- Enforce max connection count.
- Parse pipelined RESP requests.
- Write responses without unnecessary allocation.
- Apply read/write timeouts where appropriate.

### RESP Layer

Support RESP2 first:

- Simple strings.
- Errors.
- Integers.
- Bulk strings.
- Arrays.
- Null bulk strings.

Parser rules:

- Enforce maximum frame size.
- Reject malformed input with Redis-like protocol errors.
- Preserve keys and values as bytes.
- Avoid UTF-8 assumptions.

### Command Layer

The command layer maps protocol frames to typed operations.

MVP command set:

- `PING`
- `GET`
- `SET`
- `DEL`
- `EXISTS`
- `EXPIRE`
- `TTL`
- `MGET`
- `MSET`
- `FLUSHDB`

Compatibility principle:

- Match Redis behavior where it is cheap and clear.
- Return explicit unsupported-command errors for everything else.
- Add commands only when real target applications need them.

### Cache Engine

The cache engine owns key/value data and metadata.

Entry shape:

```text
key: bytes
value: bytes
expires_at: optional instant
last_access: approximate timestamp or counter
memory_cost: estimated bytes
```

Expiration:

- Lazy expiration on reads and writes.
- Background expiration pass.
- TTL lookup should not require scanning the full map.

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
- Request counts by command.
- Error counts.
- Cache hits and misses.
- Expired keys.
- Evicted keys.
- Memory used and memory limit.
- Current connection count.

Metrics can start as an HTTP `/metrics` endpoint or periodic log output. A
Prometheus endpoint is preferred once the server has an HTTP admin listener.

## Native Cache API Direction

The Redis adapter should not constrain the native model. Native features should
be designed around common cache failure modes.

### Stampede Protection

Proposed operation:

```text
GET_OR_LEASE key lease=10s stale=60s
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

```text
SET key value ttl=5m tags=user:42,workspace:abc
INVALIDATE_TAG user:42
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
- Enforce max frame size and max value size.
- Avoid panic paths on malformed client input.

## Performance Principles

- Optimize single-key hot paths first.
- Measure p99 latency, not only throughput.
- Avoid global locks on reads in the sharded design.
- Keep values as bytes and avoid unnecessary copies.
- Make memory accounting cheap and approximate enough to stay fast.
- Prefer predictable bounded work per request.

## Open Design Questions

- Should the native API be binary TCP, HTTP/2, or both?
- How much Redis behavior should `SET` options support in the MVP?
- Should unsupported Redis commands return `ERR unknown command` or a clearer
  Cachebox-specific error in compatibility mode?
- Which first real applications should drive compatibility testing?
- Should eviction start with random eviction for simplicity or approximate LRU
  for better cache behavior?
