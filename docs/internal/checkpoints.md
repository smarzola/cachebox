# Cachebox Checkpoints

This file records clean implementation checkpoints from the MVP goal loop.

## Milestone 0: Repository Baseline

What works:

- The crate builds as a binary plus library.
- Module boundaries exist for API routing, config, engine, operation parsing,
  and server startup.
- The binary is runnable before the HTTP server exists.
- `--help` and `--bind <addr:port>` are parsed without extra dependencies.

Deferred:

- No HTTP listener is started yet.
- Engine and operation modules are boundaries only.

Next risk:

- Keeping route and operation parsing byte-oriented once a real HTTP framework
  is introduced.

## Milestone 1: HTTP API Contract

What works:

- MVP route shapes are parsed under `/v1/namespaces/{namespace}`.
- Keys and tags are percent-decoded into raw bytes without UTF-8 assumptions.
- PUT metadata parsing covers TTL, stale TTL, tags, cost, and content type.
- Error responses have deterministic codes and messages.
- `axum` on `tokio`/Hyper is selected as the future HTTP server stack.

Deferred:

- No socket-level HTTP handlers exist yet.
- Batch and lease JSON bodies are not parsed until operation parsing.
- Response encoding is represented by contract types, not framework responses.

Next risk:

- Milestone 2 needs typed cache operations that preserve raw request bodies while
  producing deterministic errors for malformed methods, paths, headers, and
  bodies.

## Milestone 2: Operation Parsing

What works:

- HTTP request parts are converted into typed cache operations.
- Get, put, delete, batch get, tag invalidation, lease start, and lease
  completion are represented with namespace, key, metadata, and raw value bytes.
- GET, DELETE, and tag invalidation reject unexpected request bodies.
- PUT and lease completion preserve raw byte bodies without UTF-8 assumptions.
- Batch get accepts a small JSON control body with percent-encoded keys.
- Lease start accepts a small JSON control body with `lease_ttl_ms` and optional
  `allow_stale_ms`.
- Lease completion requires `Cachebox-Lease-Token`.
- Unsupported routes, unsupported methods, invalid headers, and malformed bodies
  produce structured operation errors.

Deferred:

- Control-body JSON parsing is intentionally narrow and dependency-free until
  the HTTP server stack lands.
- Operation parsing does not execute cache behavior yet.
- Health and metrics routes remain outside the cache operation parser.

Next risk:

- Milestone 3 needs an injectable clock and byte-keyed in-memory engine that can
  execute these operations with correct TTL, stale TTL, delete, batch, and tag
  invalidation semantics.

## Milestone 3: In-Memory Cache Engine

What works:

- The engine stores byte keys and byte values per namespace.
- Get, put, delete, batch get, and tag invalidation are implemented without
  networking.
- TTL and stale TTL are evaluated with an injectable clock.
- Expired entries are removed lazily on access and length checks.
- Stale entries return a distinct stale outcome until the stale window expires.
- Deletes are idempotent.
- Batch get preserves input order and returns mixed hits, misses, and stale
  outcomes.
- Replacing an entry cleans old tag index references.
- Tag invalidation is namespace-scoped.

Deferred:

- Lease state waits for the HTTP server milestone before it is exposed to
  clients.
- Background expiration is not implemented.
- Memory accounting, value size limits, and eviction wait for Milestone 5.

Next risk:

- Milestone 4 needs to wire the parser and engine through an async HTTP server
  without losing raw-byte response bodies or deterministic error behavior.

## Milestone 4: HTTP Server

What works:

- The binary starts an axum/tokio HTTP server bound to the configured address.
- HTTP/2 support is enabled through the axum/Hyper stack.
- `/healthz` responds through the live server.
- PUT and GET preserve raw byte values through the HTTP API.
- DELETE, batch get, and tag invalidation execute through the HTTP handler.
- Lease start and lease completion execute through the HTTP handler.
- Request bodies are rejected when they exceed `--max-body-bytes`.
- Handler tests exercise raw bytes, batch get, tag invalidation, health checks,
  and body-limit errors without opening sockets.
- A live localhost smoke test verified PUT/GET raw bytes with matching
  `00ff76616c7565` request and response bytes.

Deferred:

- Metrics still return a placeholder body until Milestone 6.
- Shutdown uses Ctrl-C only; richer shutdown coordination is deferred.

Next risk:

- Milestone 5 needs memory accounting, max value size, and eviction without
  breaking the raw-byte HTTP paths that now work.

## Milestone 5: Memory Limits and Eviction

What works:

- Startup configuration accepts `--max-memory-bytes` and `--max-value-bytes`.
- The engine tracks estimated memory per entry and total memory used.
- Estimated entry size includes namespace bytes, key bytes, value bytes, tag
  bytes, and a fixed entry overhead.
- Single values larger than `max_value_bytes` are rejected.
- Entries that cannot fit the configured memory cap are rejected clearly.
- Writes reclaim expired entries before evicting live entries.
- Writes evict least-recently-used entries until the incoming entry fits.
- Replacing an existing key updates memory accounting and tag indexes.
- Eviction and expiration counters increase in engine stats.
- HTTP PUT maps oversized values and memory-fit failures to deterministic JSON
  errors.
- Handler tests cover max value rejection and memory-cap eviction through the
  HTTP API.

Deferred:

- Accounting is approximate and intentionally not allocator-exact.
- Eviction is global across namespaces; namespace quotas are still future work.
- Metrics expose neither memory nor eviction counters until Milestone 6.

Next risk:

- Milestone 6 needs structured logs and metrics that surface request counts,
  hits, misses, stale responses, expirations, evictions, errors, memory usage,
  memory limit, and connection behavior.

## Milestone 6: Metrics and Logging

What works:

- Server startup and Ctrl-C shutdown logs use key-value text.
- `/metrics` returns Prometheus-style text instead of a placeholder.
- Metrics include total requests, health requests, metrics requests, get, put,
  delete, batch get, and tag invalidation request counts.
- Metrics include hit, miss, stale, lease grant, lease denial, error,
  expiration, eviction, memory used, and memory limit values.
- Metrics are backed by handler counters plus engine stats.
- Handler tests prove metrics update after hits, misses, oversized-value errors,
  and LRU eviction.

Deferred:

- `cachebox_connections_current` is exposed as `0`; actual per-connection
  tracking is deferred until the server accepts a connection instrumentation
  layer.
- Logs are plain key-value text, not a tracing subscriber yet.
- Metrics are process-local and reset on restart.

Next risk:

- Milestone 7 needs end-to-end client smoke tests that spawn the binary and
  exercise the supported HTTP operations from a real client.

## Milestone 7: Client Smoke Tests

What works:

- The engine grants one active lease on a miss or stale value.
- Active leases deny duplicate miss refresh attempts.
- Stale values are returned while a lease is active.
- Lease completion stores refreshed raw bytes when the token matches.
- Expired lease tokens are rejected.
- HTTP lease start returns structured states: `hit`, `stale`,
  `lease_granted`, and `lease_denied`.
- HTTP lease completion refreshes values through the same memory-limited write
  path as PUT.
- A spawned-binary client smoke test exercises health, put, get, delete, batch
  get, tag invalidation, lease start, lease completion, and unsupported-route
  errors over real TCP.

Deferred:

- The spawned-binary smoke is marked ignored by default because it binds a
  localhost TCP port; run it explicitly with
  `cargo test --test spawned_client -- --ignored`.
- There is not a packaged official client module yet.
- Lease hardening still needs deterministic concurrent-miss tests and load
  coverage in Milestone 9.

Next risk:

- Milestone 8 needs a reproducible benchmark harness with baseline commands and
  no performance claims beyond measured local results.

## Milestone 8: Benchmark Harness

What works:

- `cargo run --bin cachebox-bench` runs the benchmark harness with one command.
- The harness benchmarks loopback HTTP requests against local Cachebox servers.
- Scenarios cover single-key get, single-key put, batch get, lease contention,
  tag invalidation, TTL-heavy writes, and eviction pressure.
- Each scenario includes warmup and fixed-duration measurement.
- Output includes p50, p95, p99, throughput, and memory used.
- Baseline results and the exact command are documented in
  `docs/internal/benchmarks.md`.

Deferred:

- The harness measures persistent HTTP/2 loopback requests.
- Results are local baselines only and are not portable performance claims.
- Richer benchmark export formats and longer-duration runs are future work.

Next risk:

- Milestone 9 needs to harden lease-based stampede protection with deterministic
  concurrent-miss behavior, stale serving during refresh, and lease expiry tests
  under load.

## Milestone 9: Native Cache Feature Hardening

What works:

- Lease-based stampede protection is the first hardened native cache feature.
- An active lease survives value expiry, so a refresh in progress still protects
  a hot missing key until the lease expires.
- A deterministic barrier-based concurrency test proves concurrent misses grant
  exactly one lease.
- Controlled-clock tests prove stale values can be served while a lease is
  active and that lease tokens expire if the client never completes refresh.
- A spawned-binary load smoke proves many real TCP clients requesting the same
  missing key receive exactly one `lease_granted` response and the rest receive
  `lease_denied`.
- A spawned-binary HTTP/2 smoke proves `curl --http2-prior-knowledge` can use
  cache operations including put, get, batch get, tag invalidation, lease start,
  lease completion, and delete.
- `docs/internal/supported-behavior.md` documents supported endpoints and
  unsupported MVP behavior.

Deferred:

- Lease state is process-local and in-memory only.
- Namespace overload policy is not implemented, so `lease_denied` currently
  means an active lease already exists.
- Longer-duration load tests and richer contention metrics are future work.

Next risk:

- The remaining MVP audit needs to confirm every definition-of-done item against
  current commands and docs before marking the goal complete.
