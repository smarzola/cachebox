# MVP Goal Loop Prompt

Use this prompt to drive implementation of the Cachebox MVP in small,
checkpointed loops.

## Role

You are building Cachebox, a Rust cache server for self-hosted applications. The
MVP must be correct, measurable, and intentionally narrow. Do not build a full
Redis clone. Build a cache-first server with a small Redis-compatible adapter.

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

- Add module stubs for protocol, command parsing, engine, config, and server.
- Add a `--help` friendly CLI using a minimal dependency only if needed.
- Keep the binary runnable even before the TCP server exists.

Checkpoint:

- `cargo run` exits cleanly or prints startup configuration.
- `cargo test` passes.
- README and architecture docs match the current scope.

Testing techniques:

- Unit test any config parsing.
- Snapshot-free tests only; prefer explicit assertions.

## Milestone 1: RESP2 Parser and Encoder

Goal:

- Parse and encode enough RESP2 for Redis cache commands.

Implementation:

- Parse arrays, bulk strings, simple strings, integers, errors, and null bulk
  strings.
- Preserve byte strings without assuming UTF-8.
- Enforce maximum frame size.
- Return structured parse errors.

Checkpoint:

- Parser handles complete frames, partial frames, malformed frames, and pipelined
  frames.
- Encoder round-trips expected Redis-compatible responses.

Testing techniques:

- Unit tests for every RESP type.
- Fuzz-like table tests for malformed inputs.
- Tests for partial buffers and multiple frames in one buffer.

## Milestone 2: Command Parsing

Goal:

- Convert RESP arrays into typed cache commands.

Implementation:

- Support `PING`, `GET`, `SET`, `DEL`, `EXISTS`, `EXPIRE`, `TTL`, `MGET`, `MSET`,
  and `FLUSHDB`.
- Handle command names case-insensitively.
- Validate arity and return Redis-like errors.
- Preserve key/value bytes.

Checkpoint:

- Every supported command has valid and invalid arity tests.
- Unsupported commands produce a deterministic error.

Testing techniques:

- Table-driven command parser tests.
- Compatibility tests for common Redis command spellings and casing.

## Milestone 3: In-Memory Cache Engine

Goal:

- Implement correct cache semantics without networking.

Implementation:

- Store byte keys and byte values.
- Implement `GET`, `SET`, `DEL`, `EXISTS`, `MGET`, `MSET`, and `FLUSHDB`.
- Add TTL support for `SET` options if included, `EXPIRE`, and `TTL`.
- Use lazy expiration on access.

Checkpoint:

- Expired keys disappear from reads.
- `TTL` returns Redis-compatible values for missing, no-expiry, and expiring
  keys.
- Multi-key commands behave correctly with mixed hits and misses.

Testing techniques:

- Unit tests with controlled time through an injectable clock.
- Boundary tests for zero, negative, and very large TTLs.
- Property-style tests for idempotent deletes and existence counts if practical.

## Milestone 4: TCP Server

Goal:

- Serve the command set over TCP using RESP2.

Implementation:

- Use `tokio`.
- Accept multiple clients.
- Support pipelined requests.
- Apply read frame limits.
- Keep shutdown behavior simple and testable.

Checkpoint:

- `redis-cli PING` works.
- `redis-cli SET foo bar` and `redis-cli GET foo` work.
- Multiple commands in one connection work.

Testing techniques:

- Async integration tests opening TCP connections.
- Raw RESP request tests.
- Optional smoke test using `redis-cli` when available.

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
- Track command counts, hits, misses, expirations, evictions, errors, memory, and
  connection count.
- Expose metrics through a simple HTTP endpoint or a text admin command.

Checkpoint:

- Metrics update under unit and integration tests.
- Logs include bind address and shutdown events.

Testing techniques:

- Unit tests against metrics counters.
- Integration test for metrics endpoint if HTTP is added.

## Milestone 7: Compatibility Smoke Tests

Goal:

- Prove Cachebox works with real Redis clients for the supported subset.

Implementation:

- Add smoke tests using at least one Rust Redis client.
- Add a small script or test fixture that exercises the server like an app cache.
- Document supported commands.

Checkpoint:

- Client library can connect, set, get, expire, and delete.
- Unsupported commands fail predictably.

Testing techniques:

- Integration tests spawning the Cachebox binary on a random local port.
- Contract tests comparing selected behavior to Redis when Redis is available.

## Milestone 8: Benchmark Harness

Goal:

- Establish performance baselines before optimizing.

Implementation:

- Add benchmarks for `GET`, `SET`, `MGET`, pipelined `GET`, TTL-heavy writes,
  and eviction pressure.
- Track p50, p95, p99, throughput, and memory overhead.

Checkpoint:

- Benchmarks run locally with one command.
- Baseline results are documented.
- No performance claim is made without a reproducible command.

Testing techniques:

- Keep benchmarks separate from correctness tests.
- Benchmark against loopback.
- Include warmup and fixed-duration runs.

## Milestone 9: Native Cache Feature Spike

Goal:

- Add the first feature that makes Cachebox more than a Redis subset.

Recommended first feature:

- Lease-based stampede protection.

Implementation:

- Add an internal lease state per key.
- Add a native command or experimental endpoint for `GET_OR_LEASE`.
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
- Supported Redis cache commands work over TCP.
- TTL behavior is correct.
- Memory limits are enforced.
- Basic metrics exist.
- Integration tests cover real client behavior.
- Benchmarks establish a baseline.
- Documentation clearly states supported and unsupported behavior.

## Commit Discipline

Each milestone should end with:

- Passing `cargo fmt --check`.
- Passing `cargo test`.
- Passing `cargo clippy --all-targets -- -D warnings` when dependencies allow.
- A short commit message describing the user-visible capability.
