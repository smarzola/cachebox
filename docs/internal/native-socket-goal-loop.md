# Native Socket Transport Goal Loop Prompt

Use this prompt to replace Cachebox's HTTP/2 data plane with a native socket
transport in small, checkpointed loops. The goal is a faster, simpler protocol
that official clients can implement directly over TCP or Unix domain sockets.

## Role

You are evolving Cachebox from an HTTP/2-first cache server into a native
socket cache server. Preserve the cache semantics that already work: raw byte
values, namespaces, TTL, stale TTL, tags, leases, memory limits, metrics,
bounded expiration cleanup, and approximate LRU eviction. Do not turn Cachebox
into Redis, a database, or a general RPC framework.

## Target State

Cachebox should expose a compact native protocol over:

- TCP sockets for network clients.
- Unix domain sockets for same-host clients on Unix platforms.

The hot data path should avoid HTTP routing, URI parsing, header maps, JSON for
ordinary cache operations, and avoidable body copies. Official clients should
use the native protocol. HTTP/2 should be removed from the supported data plane
once equivalent native behavior, tests, docs, and benchmarks exist.

Metrics may remain HTTP temporarily only if clearly documented as transitional.
The final target should either expose metrics through a small admin endpoint or
explicitly keep a minimal admin HTTP surface separate from the cache data plane.

## Design Principles

- Optimize the common path first: get, put, delete, batch get, tag invalidation,
  and leases.
- Keep values and keys as raw bytes.
- Use length-prefixed frames, not line parsing or URL encoding.
- Make response states deterministic and cheap to parse.
- Make clients simple enough to implement in multiple languages.
- Keep metrics observational and side-effect free.
- Bound all request body, frame, cleanup, and memory work.
- Prefer explicit versioning over compatibility guesses.
- Benchmark every milestone against the current HTTP/2 baseline before making
  broader claims.

## Definition Of Done

This transport migration is done when:

- Native TCP and Unix socket listeners support the full current cache operation
  set.
- The protocol has a documented binary frame format with request and response
  examples.
- Official client smoke tests use the native protocol.
- HTTP/2 data-plane routes are removed or intentionally isolated as a deprecated
  compatibility layer with a removal checkpoint.
- Benchmarks include engine-only, native TCP, native Unix socket, and any
  remaining HTTP/admin paths.
- Native transport is materially faster than the old HTTP/2 baseline for cached
  get, put, lease contention, tag invalidation, TTL-heavy writes, and eviction
  pressure.
- Metrics remain side-effect free.
- All docs, tests, examples, and startup configuration match the implemented
  behavior.

## Operating Loop

For every milestone:

1. Restate the target behavior and the current implementation gap.
2. Inspect current code and docs before editing.
3. Make the smallest coherent implementation step that moves toward the native
   transport target.
4. Add or update tests that prove the behavior.
5. Run formatting, tests, clippy, and milestone-specific benchmarks.
6. Update docs and benchmark baselines when behavior or measured paths change.
7. Commit the checkpoint with a concise message.
8. Push the branch before starting the next milestone.

Use these standard checks after code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Use `cargo fmt` before committing if formatting fails. For transport milestones,
also run:

```sh
cargo run --bin cachebox-bench
```

## Milestone 0: Baseline And Transport Audit

Goal:

- Capture the current HTTP/2 costs and identify the exact code paths to replace.

Implementation:

- Audit `server`, `operation`, `api`, benchmarks, docs, and spawned-client
  tests.
- Record current HTTP/2 benchmark rows for sequential cached get, put, batch
  get, lease contention, tag invalidation, TTL-heavy writes, and eviction
  pressure.
- Identify which behavior is data plane and which behavior is admin/control
  plane.

Checkpoint:

- A short design note lists the data-plane operations that must move to native
  sockets.
- Current benchmarks are reproducible.
- No behavior changes are made unless they are documentation-only.

## Milestone 1: Binary Frame Specification

Goal:

- Define the native protocol before writing the listener.

Implementation:

- Specify a fixed frame header with protocol magic, version, command, flags,
  request id, and payload length.
- Specify payload layouts for get, put, delete, batch get, tag invalidation,
  lease start, and lease completion.
- Specify response layouts for hit, stale, miss, lease granted, lease denied,
  deleted, stored, invalidated, and errors.
- Specify size limits, invalid frame handling, unknown command handling, and
  connection close behavior.
- Specify whether one connection supports pipelining and how request ids map
  responses.

Checkpoint:

- Protocol docs include byte layout tables and at least one hex or pseudo-binary
  example.
- Tests are planned for malformed frames before the listener is implemented.
- No client is expected to infer behavior from Rust structs.

## Milestone 2: Protocol Codec Module

Goal:

- Implement encode/decode logic without opening sockets.

Implementation:

- Add a protocol module with request and response frame types.
- Decode frames from byte buffers with strict length and limit checks.
- Encode responses without JSON.
- Convert decoded native requests into engine commands or direct operation
  execution inputs.
- Keep allocation behavior visible and minimal.

Checkpoint:

- Unit tests cover valid frames for every operation.
- Unit tests cover malformed magic, unsupported version, unknown command,
  truncated frames, oversized payloads, and invalid metadata.
- Codec tests use raw byte keys and values.

## Milestone 3: Native TCP Listener

Goal:

- Serve the native protocol over persistent TCP connections.

Implementation:

- Add configurable TCP bind support for the native listener.
- Accept connections with Tokio.
- Read frames in a loop.
- Execute operations against the existing engine.
- Write binary responses.
- Enforce max frame and value limits.
- Keep the existing cleanup worker and metrics semantics unchanged.

Checkpoint:

- Integration tests open a TCP socket and perform put, get, delete, batch get,
  tag invalidation, lease start, and lease completion.
- Invalid frames return deterministic protocol errors or close the connection as
  documented.
- Native TCP benchmark rows exist alongside the old HTTP/2 rows.

## Milestone 4: Unix Socket Listener

Goal:

- Support same-host clients without TCP loopback overhead.

Implementation:

- Add Unix domain socket configuration on Unix platforms.
- Clean up stale socket files only when safe.
- Use the same frame codec and operation execution path as TCP.
- Keep platform-specific code isolated.

Checkpoint:

- Unix socket integration tests pass on supported platforms.
- Docs show Unix socket startup and client examples.
- Benchmarks include native Unix socket rows.

## Milestone 5: Shared Operation Execution Without HTTP

Goal:

- Avoid keeping HTTP parser abstractions in the native hot path.

Implementation:

- Extract engine execution helpers that operate on already-decoded native
  commands.
- Keep HTTP-specific route/header parsing out of native code.
- Avoid `body.to_vec`, `HeaderMap`, URI decoding, and JSON on native cache
  operations.
- Preserve exact cache semantics across native and any remaining HTTP tests
  during transition.

Checkpoint:

- Native operation execution tests prove parity with existing cache behavior.
- Hot native get and put paths avoid HTTP-specific types.
- Benchmarks show the impact of bypassing HTTP parsing.

## Milestone 6: Official Client Smoke Path

Goal:

- Prove real clients can use the native protocol ergonomically.

Implementation:

- Add a minimal Rust client module or test client that can connect over TCP and
  Unix sockets.
- Implement get, put, delete, batch get, tag invalidation, and leases.
- Keep client framing logic independent from server internals except for shared
  protocol constants if appropriate.

Checkpoint:

- Spawned-client tests use the native protocol.
- Client tests cover raw bytes, TTL, stale TTL, tags, leases, and errors.
- Docs show end-to-end client examples without curl.

## Milestone 7: Benchmark Rewrite

Goal:

- Make performance measurements reflect the target transport.

Implementation:

- Replace HTTP/2 benchmark rows with native TCP and Unix socket rows.
- Keep engine-only rows.
- Add pipelined and concurrent native scenarios if the protocol supports
  request ids.
- Remove misleading HTTP/2 transport assumptions from benchmark docs.

Checkpoint:

- `cargo run --bin cachebox-bench` reports engine, native TCP, and native Unix
  socket rows where supported.
- Benchmarks identify remaining bottlenecks after the transport change.
- Baselines are refreshed and documented as local-only measurements.

## Milestone 8: Remove HTTP/2 Data Plane

Goal:

- Stop supporting HTTP/2 as the primary cache operation protocol.

Implementation:

- Remove HTTP/2 cache routes, operation parsing paths, curl-first examples, and
  HTTP/2 spawned-client tests once native equivalents exist.
- Decide whether to keep a minimal HTTP admin endpoint for health and metrics.
- If admin HTTP remains, document it explicitly as not the data plane.
- Update startup logs and configuration to make native sockets the default.

Checkpoint:

- User-facing docs no longer present HTTP/2 as the cache API.
- Internal docs explain any remaining admin HTTP surface.
- Tests and benchmarks do not depend on HTTP/2 for cache operations.

## Milestone 9: Performance Hardening

Goal:

- Use native transport measurements to remove the next bottlenecks.

Implementation:

- Profile native get and put paths.
- Reduce avoidable allocations.
- Consider sharded engine locks if concurrency is the next bottleneck.
- Consider per-shard metrics counters.
- Consider pipelining improvements and write batching.

Checkpoint:

- Optimizations are driven by benchmark evidence.
- Each performance claim has before/after numbers.
- Correctness tests continue to pass.

## Commit Discipline

Each milestone must end with:

- A clear checkpoint summary.
- Passing required checks.
- Updated docs when behavior changes.
- Updated benchmarks when measured paths change.
- A local commit with a concise message.
- A push to the current branch.

If a check, benchmark, or push is blocked, stop at that milestone and report the
exact blocker before continuing.
