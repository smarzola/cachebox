# Native Socket Transport Audit

This note is Milestone 0 for the native socket transport goal loop. It captures
the current HTTP/2 data-plane baseline, the code paths to replace, and the
boundary between cache data-plane behavior and admin/control behavior.

## Current Transport Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
engine_get engine 2015345 417 542 792 2015344.08 113 0 engine_cached_hit
engine_put engine 450717 1791 2250 2875 450716.91 52184362 0 engine_unique_keys
engine_tag_invalidate_8 engine 41519 8208 8417 11375 120769.64 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8821 113375 129167 143250 8820.05 113 0 cached_hit
single_key_put loopback_h2 9377 101417 121667 134916 9376.93 1080081 0 unique_keys
batch_get_32 loopback_h2 4696 211333 228792 251542 4695.36 1083719 0 32_keys
lease_contention loopback_h2 7656 129875 141959 154667 7655.77 1083719 0 same_missing_key
tag_invalidate_empty loopback_h2 8240 121625 133333 142916 8239.50 1083719 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 920 117000 136042 145750 8398.13 1083719 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 921 1085666 1121584 1136917 920.34 1083719 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8567 118292 128334 139250 8566.60 2071347 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 9048 106667 123791 134625 9047.88 65443 0 64KiB_cap
cost_shaped_writes loopback_h2 2800 359333 376708 387833 2799.60 4567817 4352900 cheap_large_expensive_small_mixed_ttl
```

The important gap is the engine-to-transport spread:

- Cached engine get: `417 ns` p50.
- Loopback HTTP/2 cached get: `113375 ns` p50.
- Engine unique put: `1791 ns` p50.
- Loopback HTTP/2 unique put: `101417 ns` p50.
- Engine tag invalidation of 8 keys: `8208 ns` p50.
- Loopback HTTP/2 tag invalidation of 8 keys: `117000 ns` p50.

The native transport target is not to make sockets nanosecond-fast. The target
is to remove avoidable HTTP/2, routing, header, JSON, percent-decoding, and copy
costs so the socket path is much closer to the engine path.

## Current Data Plane

The current cache data plane is implemented as HTTP/2 routes under
`/v1/namespaces/{namespace}`:

- `GET /v1/namespaces/{namespace}/keys/{key}`
- `PUT /v1/namespaces/{namespace}/keys/{key}`
- `DELETE /v1/namespaces/{namespace}/keys/{key}`
- `POST /v1/namespaces/{namespace}/batch/get`
- `POST /v1/namespaces/{namespace}/tags/{tag}/invalidate`
- `POST /v1/namespaces/{namespace}/leases/{key}`
- `PUT /v1/namespaces/{namespace}/leases/{key}/complete`

These operations must move to the native protocol:

- Get key.
- Put key with TTL, stale TTL, tags, and cost metadata.
- Delete key.
- Batch get.
- Tag invalidation.
- Start lease.
- Complete lease.

Native requests must preserve current semantics:

- Namespaces remain distinct.
- Keys and values remain raw bytes.
- TTL and stale TTL behavior remains unchanged.
- Tag indexes remain namespace-scoped.
- Lease states remain hit, stale, lease granted, and lease denied.
- Memory limits, value limits, cleanup, eviction, and metrics semantics remain
  unchanged.

## Current Admin And Control Plane

The current non-cache routes are:

- `GET /healthz`
- `GET /metrics`

These should not drive the native data-plane frame format. During transition,
they can remain as a minimal HTTP admin surface if useful. If they remain, docs
must clearly say HTTP is admin-only and not the cache data plane.

Metrics must remain observational:

- Scraping metrics must not reclaim expired entries.
- Scraping metrics must not increment request counters.
- Cleanup remains passive on access, write-pressure driven, and bounded by the
  background expiration worker.

## Current Hot Path To Replace

The current HTTP/2 request path goes through these layers:

1. `axum::serve` accepts the connection.
2. `handle_request` receives `Method`, `OriginalUri`, `HeaderMap`, and
   `Bytes`.
3. Metrics request counters are updated for non-metrics routes.
4. Request body size is checked.
5. HTTP method is converted to the internal API method.
6. Headers are copied into lower-case `String` pairs.
7. The request body is copied with `body.to_vec()`.
8. `parse_operation` parses URI routes, percent-decodes keys and tags, parses
   metadata headers, and parses JSON control bodies.
9. `execute_operation` locks the single shared engine and executes the cache
   operation.
10. Responses are built through Axum response conversion, raw bodies, or JSON
    serialization depending on operation.

The native path should remove or avoid:

- Axum routing for cache operations.
- HTTP method conversion.
- URI path parsing.
- Percent-decoding for native keys and tags.
- `HeaderMap` traversal for cache metadata.
- JSON control envelopes for batch and lease operations.
- `body.to_vec()` for operations that do not need owned bodies.
- HTTP status and response conversion for cache operation states.

## Existing Modules Affected

- `src/server.rs`: currently owns listener startup, shared state, request
  handling, metrics, background cleanup, and HTTP response construction.
- `src/operation.rs`: currently maps HTTP request parts into cache operations.
  This is HTTP-specific and should not be reused as the native hot path.
- `src/api.rs`: currently owns HTTP routes, headers, percent-decoding, and
  metadata parsing.
- `src/engine.rs`: should remain transport-independent. Native protocol work
  should call the engine without adding socket-specific behavior to it.
- `src/bin/cachebox-bench.rs`: currently benchmarks engine and loopback HTTP/2.
  It must grow native TCP and Unix socket rows, then remove HTTP/2 rows when the
  data plane moves.
- `tests/spawned_client.rs`: currently proves HTTP/2 client behavior. It must
  move to native client smoke behavior before HTTP/2 routes are removed.

## Native Protocol Requirements For Milestone 1

The next milestone should specify, before implementation:

- Fixed frame header fields: magic, version, command, flags, request id, and
  payload length.
- Payload layouts for all current cache operations.
- Response layouts for all current cache states and errors.
- Maximum frame and value sizes.
- Handling for unsupported versions, unknown commands, truncated frames, and
  oversized payloads.
- Whether pipelining is supported in the first version.
- Endianness and integer widths.
- String and byte encoding rules.
- How TTL, stale TTL, tags, cost, and lease token metadata are represented.

## Milestone 0 Checkpoint

Current state:

- HTTP/2 remains the only implemented network data plane.
- Engine-only benchmarks are already much faster than loopback HTTP/2 rows.
- Native socket work should start with a protocol specification, not with
  listener code.

Verification for this checkpoint:

```sh
cargo fmt --check
cargo test
cargo run --bin cachebox-bench
```
