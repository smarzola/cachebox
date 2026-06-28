# Performance Phase 2 Goal Loop Prompt

Use this prompt to drive the next Cachebox performance phase after the first
native socket performance uplift. The first phase proved the native protocol,
removed obvious copy costs, added pipelining, sharded engine ownership, striped
metrics, and bounded response coalescing. It did not reach the sequential
socket latency targets.

## Role

You are reducing the remaining native socket latency and contention in
Cachebox. Preserve existing cache semantics: raw byte values, namespaces, TTL,
stale TTL, tags, leases, memory limits, side-effect-free metrics, bounded
cleanup, approximate LRU behavior, native TCP, native Unix sockets, and
admin-only HTTP for health and metrics.

Do not turn Cachebox into Redis, a database, a general RPC framework, or a
protocol compatibility project. Keep it a cache-native server with simple
official clients.

## Current Measured State

Final benchmark command from the previous phase:

```sh
cargo run --bin cachebox-bench
```

Selected local p50 results:

| Scenario | Transport | p50 ns |
| --- | --- | ---: |
| `engine_get` | `engine` | 417 |
| `engine_put` | `engine` | 1791 |
| `single_key_get` | `loopback_tcp` | 32459 |
| `single_key_put` | `loopback_tcp` | 31209 |
| `pipelined_get_32` | `loopback_tcp` | 7842 |
| `concurrent_get_16` | `loopback_tcp` | 110792 |
| `concurrent_put_16` | `loopback_tcp` | 114209 |
| `single_key_get` | `loopback_unix` | 20041 |
| `single_key_put` | `loopback_unix` | 23083 |
| `pipelined_get_32` | `loopback_unix` | 6651 |
| `concurrent_get_16` | `loopback_unix` | 80958 |
| `concurrent_put_16` | `loopback_unix` | 80292 |
| `tag_invalidate_8` | `loopback_unix` | 37583 |

These are local loopback numbers, not portable production claims.

## Target State

Cachebox should have a measured, defensible path toward these local development
targets:

- Native Unix cached get p50 under `8 us` for non-pipelined single-key reads.
- Native TCP cached get p50 under `15 us` for non-pipelined single-key reads.
- Native Unix single-key put p50 under `12 us`.
- Native TCP single-key put p50 under `20 us`.
- Native Unix pipelined cached get p50 under `5 us`.
- Tag invalidation of eight keys under `20 us` over Unix sockets.
- Multi-client distributed-key workloads scale without one shard, task queue,
  metrics counter, or tag index becoming the obvious bottleneck.

If a target cannot be reached without a larger design change, document the
measured blocker and the next design choice clearly.

## Design Principles

- Measure before and after each change.
- Optimize the measured hot path, not attractive guesses.
- Keep benchmark scenarios stable enough for comparisons.
- Prefer fewer tasks, fewer syscalls, fewer allocations, and shorter lock
  scopes over small local code tricks.
- Do not add latency to non-pipelined requests to improve batch throughput.
- Keep request-id response matching and explicit pipelining semantics.
- Keep metrics observational and side-effect free.
- Keep changes scoped to cache behavior and native transport performance.

## Definition Of Done

This phase is done when:

- The benchmark harness can distinguish scheduler/task overhead, allocation
  overhead, lock/access-update overhead, and socket read/write overhead well
  enough to guide the next phase.
- Non-pipelined native get and put paths no longer spawn a task per request
  when there is no queued work requiring concurrent execution.
- Per-connection hot buffers are reused where practical.
- `get` access accounting is either made cheaper or its lock/update cost is
  measured and documented as the next blocker.
- Tag invalidation no longer requires a blind scan of all shard-local tag
  indexes, or the remaining blocker is quantified and documented.
- User-facing benchmark docs and internal notes reflect the current
  performance surface without overclaiming.
- Formatting, tests, clippy, spawned-client smoke tests, and benchmarks pass.
- Every milestone is committed and pushed before moving to the next milestone.

## Operating Loop

For every milestone:

1. Restate the target behavior and current measured gap.
2. Inspect the current implementation before editing.
3. Capture a baseline with `cargo run --bin cachebox-bench`.
4. Add a focused benchmark or instrumentation row if the bottleneck is not
   already isolated.
5. Make the smallest coherent optimization.
6. Run formatting, tests, clippy, spawned-client smoke tests, and benchmarks.
7. Update docs with before/after numbers and remaining bottlenecks.
8. Commit the checkpoint with a concise message.
9. Push the branch before starting the next milestone.

Use these standard checks after code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Use these milestone-specific checks for server/native transport changes:

```sh
cargo test --test spawned_client spawned_binary_supports_native_tcp_client_workflow -- --ignored --exact
cargo test --test spawned_client spawned_binary_supports_native_unix_client_workflow -- --ignored --exact
cargo test --test spawned_client spawned_binary_grants_one_lease_under_client_contention -- --ignored --exact
cargo run --bin cachebox-bench
git diff --check
```

## Milestone 0: Profiling And Measurement Split

Goal:

- Stop treating all remaining latency as one opaque socket cost.

Implementation:

- Add focused benchmark rows or optional instrumentation that separate:
  - decode and execute without socket writes
  - encode without socket writes
  - single-request inline execution
  - spawned execution
  - pipelined execution with queued responses
  - get hit with access update versus a measured no-access-update experiment
- Keep the existing benchmark rows stable.
- Do not leave expensive instrumentation enabled on the default hot path.

Checkpoint:

- Internal notes identify the largest measured remaining costs.
- Benchmark docs explain any new scenario rows.
- No production behavior changes unless they only expose better measurements.

## Milestone 1: Adaptive Native Execution

Goal:

- Avoid task-per-request overhead for non-pipelined single-request flows while
  preserving pipelined concurrency.

Implementation:

- Keep the reader loop cancellation-safe.
- Execute inline when a connection has no queued frames and no evidence of
  pipelining.
- Switch to spawned execution when more than one request is in flight or a
  command shape benefits from concurrency.
- Preserve the per-connection in-flight bound.
- Preserve out-of-order response semantics for pipelined work.
- Make the ordering behavior explicit in tests.

Checkpoint:

- Sequential `single_key_get` and `single_key_put` improve materially or the
  remaining task/scheduler cost is quantified.
- Pipelined tests still prove request-id matching.
- Pipelined benchmark rows do not regress without documentation.

## Milestone 2: Per-Connection Buffer Reuse

Goal:

- Reduce allocation churn in request read and response write paths.

Implementation:

- Reuse a request frame buffer for inline/non-pipelined execution.
- Reuse response buffers where response ownership and pipelined concurrency
  allow it.
- Keep spawned pipelined work safe; do not share mutable buffers across tasks.
- Keep max frame size enforcement unchanged.

Checkpoint:

- Benchmarks show allocation-sensitive rows before and after.
- Tests cover malformed frames, oversized frames, and pipelined requests.
- Docs state which buffers are reused and where ownership requires allocation.

## Milestone 3: Cheaper Get Access Accounting

Goal:

- Reduce lock-held mutation work for hot cached gets.

Implementation:

- Measure the cost of updating approximate LRU access metadata on every get.
- Consider sampled access updates, per-shard access logs, or deferred access
  accounting.
- Preserve eviction behavior well enough for memory limits and approximate LRU
  expectations.
- Do not make metrics or accounting accessors side-effectful.

Checkpoint:

- Hot get benchmarks show whether access accounting was a material blocker.
- Eviction tests cover the selected access-accounting behavior.
- Docs describe the eviction tradeoff honestly.

## Milestone 4: Tag Invalidation Routing

Goal:

- Avoid scanning every shard-local tag index for ordinary tag invalidation.

Implementation:

- Measure current all-shard invalidation cost for empty tags and populated
  eight-key tags.
- Consider a global tag directory that maps `(namespace, tag)` to involved
  shards.
- Preserve namespace isolation and exact invalidation semantics.
- Keep tag replacement and delete cleanup correct.
- Avoid adding a global lock to ordinary get and put paths.

Checkpoint:

- Unix `tag_invalidate_8` moves toward the sub-20 us target or the remaining
  tag-index cost is quantified.
- Tests cover tag replacement, delete cleanup, namespace isolation, and
  multi-shard invalidation.
- Docs describe the chosen global-directory or routing tradeoff.

## Milestone 5: Client Batching Helpers

Goal:

- Let official clients use pipelining ergonomically without changing server
  semantics.

Implementation:

- Add a client helper for sending multiple requests before awaiting responses.
- Preserve request-id matching and error behavior.
- Keep the simple single-request client API.
- Benchmark the helper against manual pipelining and sequential requests.

Checkpoint:

- Client tests cover mixed response order and error propagation.
- Benchmarks show whether client-side batching improves application-visible
  latency or throughput.
- User-facing docs include examples for batching where useful.

## Milestone 6: Final Phase 2 Release Gate

Goal:

- Make the Phase 2 performance story accurate and actionable.

Implementation:

- Refresh `docs/benchmarks.md` with final local output.
- Add an internal Phase 2 summary of what improved, what did not, and why.
- List the next architectural choices if targets remain unmet.
- Ensure docs never claim portable production performance from local loopback
  numbers.

Checkpoint:

- Final benchmark output is checked in.
- All tests and spawned-client smokes pass.
- The branch is clean, committed, and pushed.

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
