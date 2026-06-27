# MVP Goal Loop Prompt

Use this prompt to drive implementation of the Cachebox MVP in small,
checkpointed loops.

## Role

You are building Cachebox, a Rust cache server for self-hosted applications. The
MVP must be correct, measurable, and intentionally narrow. Do not build a full
Redis clone. Build an HTTP/2-first cache server with native cache semantics.

## Operating Loop

For every milestone:

1. Restate the target behavior.
2. Inspect the current code before editing.
3. Make the smallest coherent implementation step.
4. Add or update tests that prove the behavior.
5. Run formatting, tests, and any milestone-specific checks.
6. Record what works, what is deferred, and the next risk.
7. Stop only at a clean checkpoint.

Use these standard checks after code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Use `cargo fmt` before committing if formatting fails.

## Milestone 0: Repository Baseline

Goal:

- Keep the project buildable from a fresh checkout.
- Establish the docs, module layout, and basic binary entrypoint.

Implementation:

- Add module stubs for API routing, operation parsing, engine, config, and
  server.
- Add a `--help` friendly CLI using a minimal dependency only if needed.
- Keep the binary runnable even before the HTTP server exists.

Checkpoint:

- `cargo run` exits cleanly or prints startup configuration.
- `cargo test` passes.
- README and architecture docs match the current scope.

Testing techniques:

- Unit test any config parsing.
- Snapshot-free tests only; prefer explicit assertions.

## Milestone 1: HTTP API Contract

Goal:

- Define and test the HTTP request and response contract for cache operations.

Implementation:

- Choose the HTTP framework.
- Define route shapes under `/v1/namespaces/{namespace}`.
- Define TTL, stale TTL, tag, cost, and content-type metadata.
- Define response status codes and JSON error envelopes.
- Preserve cache values as raw bytes.

Checkpoint:

- API contract is documented in the code and README.
- Raw-byte values can be represented without UTF-8 assumptions.
- Error responses are deterministic.

Testing techniques:

- Unit tests for route parsing and metadata parsing.
- Table tests for malformed TTLs, tags, namespaces, and percent-encoded keys.
- Explicit tests for binary values.

## Milestone 2: Operation Parsing

Goal:

- Convert HTTP requests into typed cache operations.

Implementation:

- Support get, put, delete, batch get, tag invalidation, lease start, and lease
  completion operations.
- Validate request methods, paths, headers, and bodies.
- Preserve key/value bytes.
- Return structured application errors.

Checkpoint:

- Every supported operation has valid and invalid request tests.
- Unsupported routes produce deterministic errors.

Testing techniques:

- Table-driven operation parser tests.
- HTTP handler tests that do not require opening sockets.

## Milestone 3: In-Memory Cache Engine

Goal:

- Implement correct cache semantics without networking.

Implementation:

- Store byte keys and byte values.
- Implement get, put, delete, batch get, and tag invalidation.
- Add TTL and stale TTL support.
- Use lazy expiration on access.

Checkpoint:

- Expired keys disappear from reads.
- TTL metadata behaves correctly for missing, no-expiry, expiring, and stale
  keys.
- Multi-key commands behave correctly with mixed hits and misses.

Testing techniques:

- Unit tests with controlled time through an injectable clock.
- Boundary tests for zero, negative, and very large TTLs.
- Property-style tests for idempotent deletes and existence counts if practical.

## Milestone 4: HTTP/2 Server

Goal:

- Serve the operation set over HTTP.

Implementation:

- Use `tokio`.
- Support HTTP/2 and allow HTTP/1.1 for local tooling if the framework makes it
  cheap.
- Accept multiple clients.
- Apply request body limits.
- Keep shutdown behavior simple and testable.

Checkpoint:

- `curl` or an HTTP client can put and get raw-byte values.
- Batch get works through the HTTP API.
- Tag invalidation works through the HTTP API.

Testing techniques:

- Async integration tests opening an HTTP client.
- Raw-byte body tests.
- HTTP/2 smoke test where the client stack exposes protocol selection.

## Milestone 5: Memory Limits and Eviction

Goal:

- Keep memory bounded.

Implementation:

- Add memory accounting per entry.
- Add config for max memory and max value size.
- Implement one eviction policy: random eviction or approximate LRU.
- Return a clear error when a single value cannot fit.

Checkpoint:

- Writes cannot push the engine past the configured cap except for documented
  accounting approximation.
- Eviction counters increase.
- Oversized values are rejected.

Testing techniques:

- Unit tests with tiny memory caps.
- Tests for replacing an existing key with smaller and larger values.
- Tests proving expired keys are reclaimed before evicting live keys.

## Milestone 6: Metrics and Logging

Goal:

- Make behavior visible to operators.

Implementation:

- Add structured logs.
- Track request counts, hits, misses, stale responses, lease grants, lease
  denials, expirations, evictions, errors, memory, and connection count.
- Expose metrics through an HTTP endpoint.

Checkpoint:

- Metrics update under unit and integration tests.
- Logs include bind address and shutdown events.

Testing techniques:

- Unit tests against metrics counters.
- Integration test for the metrics endpoint.

## Milestone 7: Client Smoke Tests

Goal:

- Prove Cachebox works with real HTTP clients and a first official client.

Implementation:

- Add smoke tests using a Rust HTTP client.
- Add a small script or test fixture that exercises the server like an app cache.
- Start one official client package or client module if the repo layout supports
  it.
- Document supported endpoints.

Checkpoint:

- Client can put, get, delete, batch get, invalidate tags, and use leases.
- Unsupported routes fail predictably.

Testing techniques:

- Integration tests spawning the Cachebox binary on a random local port.
- Contract tests asserting HTTP status codes, headers, and byte bodies.

## Milestone 8: Benchmark Harness

Goal:

- Establish performance baselines before optimizing.

Implementation:

- Add benchmarks for single-key get, single-key put, batch get, lease
  contention, tag invalidation, TTL-heavy writes, and eviction pressure.
- Track p50, p95, p99, throughput, and memory overhead.

Checkpoint:

- Benchmarks run locally with one command.
- Baseline results are documented.
- No performance claim is made without a reproducible command.

Testing techniques:

- Keep benchmarks separate from correctness tests.
- Benchmark against loopback.
- Include warmup and fixed-duration runs.

## Milestone 9: Native Cache Feature Hardening

Goal:

- Harden the first feature that makes Cachebox more than a generic key/value
  cache.

Recommended first feature:

- Lease-based stampede protection.

Implementation:

- Add an internal lease state per key.
- Add an experimental endpoint for lease acquisition.
- Return structured states: hit, stale hit, lease granted, lease denied, miss.

Checkpoint:

- Concurrent miss test grants exactly one lease.
- Stale value can be served while a lease is active.
- Lease expires if the client never completes refresh.

Testing techniques:

- Deterministic concurrency tests using barriers.
- Controlled clock tests for lease expiry.
- Load test with many clients requesting the same missing key.

## Definition of MVP Done

The MVP is done when:

- The server builds and runs as a single binary.
- Supported cache operations work over HTTP/2.
- TTL behavior is correct.
- Memory limits are enforced.
- Basic metrics exist.
- Integration tests cover real HTTP client behavior.
- Benchmarks establish a baseline.
- Documentation clearly states supported and unsupported behavior.

## Commit Discipline

Each milestone should end with:

- Passing `cargo fmt --check`.
- Passing `cargo test`.
- Passing `cargo clippy --all-targets -- -D warnings` when dependencies allow.
- A short commit message describing the user-visible capability.
