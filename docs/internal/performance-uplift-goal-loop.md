# Performance Uplift Goal Loop Prompt

Use this prompt to drive Cachebox from the current native socket baseline toward
substantially lower latency and higher concurrency. The work must be evidence
driven: measure first, optimize one bottleneck at a time, then record
before/after numbers.

## Role

You are hardening Cachebox's performance after the native socket migration.
Preserve the implemented cache semantics: raw byte values, namespaces, TTL,
stale TTL, tags, leases, memory limits, metrics, bounded expiration cleanup,
approximate LRU eviction, native TCP, native Unix sockets, and admin-only HTTP
for health and metrics.

Do not turn Cachebox into Redis, a database, or a general RPC framework. The
goal is a fast cache-native server with simple official clients.

## Current Baseline

The current local benchmark shape is:

- Engine cached get p50: about `417 ns`.
- Engine unique put p50: about `1.8 us`.
- Native Unix cached get p50: about `14 us`.
- Native TCP cached get p50: about `24 us`.
- Native Unix single-key put p50: about `15 us`.
- Native TCP single-key put p50: about `26 us`.

The first native hardening pass removed obvious frame-buffer churn. The
remaining gap is likely in:

- Syscall and scheduler overhead.
- Frame decoding into owned `String` and `Vec` values.
- Per-request response allocation for returned values.
- Sequential per-connection request handling.
- Single global engine mutex under concurrent clients.
- Atomic metrics updates if they become visible in profiles.

## Target State

Cachebox should have a measured, defensible path toward these local development
targets:

- Native Unix cached get p50 under `5 us`.
- Native TCP cached get p50 under `10 us`.
- Native Unix single-key put p50 under `8 us`.
- Native TCP single-key put p50 under `15 us`.
- Tag invalidation of eight keys under `20 us` over Unix sockets.
- Concurrency benchmark throughput scales when clients increase, without one
  global engine lock dominating.

These are stretch targets, not claims. If a target cannot be reached without a
larger architectural change, document the measured blocker and the next design
choice clearly.

## Design Principles

- Optimize measured bottlenecks, not guesses.
- Keep benchmark scenarios stable enough for before/after comparisons.
- Prefer hot-path structural improvements over micro-tuning.
- Avoid extra copies for keys, namespaces, tags, values, and response frames.
- Keep the engine transport-independent.
- Keep metrics observational and side-effect free.
- Preserve deterministic protocol errors and response states.
- Preserve simple official client behavior.
- Add concurrency only where ordering semantics are explicit.

## Definition Of Done

This performance uplift loop is done when:

- Benchmarks include sequential and concurrent native TCP and Unix socket
  scenarios.
- Benchmarks report before/after numbers for every optimization milestone.
- Hot get and put paths avoid avoidable frame allocations and owned decode
  copies where practical.
- The server has a clear answer for pipelining: either implemented with
  request-id response matching or explicitly deferred with measured rationale.
- The engine no longer has avoidable global-lock contention under multi-client
  workloads, or the remaining lock bottleneck is quantified and documented.
- User-facing docs and benchmark docs reflect the current performance surface.
- All correctness tests, spawned-client tests, and benchmarks pass.
- Every milestone is committed and pushed before moving to the next milestone.

## Operating Loop

For every milestone:

1. Restate the target behavior and current measured gap.
2. Inspect the current implementation before editing.
3. Capture a baseline with `cargo run --bin cachebox-bench`.
4. Add a focused benchmark if the target bottleneck is not already measured.
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

## Milestone 0: Baseline And Measurement Quality

Goal:

- Make performance measurements trustworthy enough to guide optimization.

Implementation:

- Run the current benchmark harness and record full output.
- Review whether benchmark rows include enough concurrent and pipelined
  pressure to expose the real bottlenecks.
- Add missing benchmark rows without changing production behavior:
  - concurrent native get with many clients
  - concurrent native put with many clients
  - many requests on one persistent connection
  - many short-lived connections if startup/teardown matters
- Keep existing sequential rows stable.

Checkpoint:

- Benchmark docs include the current sequential baseline.
- Internal performance notes include what is and is not measured.
- No production behavior changes unless they only expose better measurements.

## Milestone 1: Borrowed Decode Hot Path

Goal:

- Reduce owned allocation and copy costs while decoding hot native requests.

Implementation:

- Introduce borrowed request views for hot commands where safe:
  - get
  - delete
  - tag invalidation
  - lease start
- Decode namespaces, keys, and tags as borrowed slices or borrowed strings
  before copying into engine-owned structures.
- Add direct execution paths from borrowed views to engine methods where the
  engine can accept borrowed inputs.
- Keep owned `RequestPayload` decoding available for tests, client APIs, and
  less common paths.

Checkpoint:

- Unit tests cover borrowed decode validation and malformed frames.
- Sequential get/delete/tag benchmarks show before/after numbers.
- No protocol format changes.

## Milestone 2: Response Encoding Without Extra Value Copies

Goal:

- Avoid unnecessary response allocation and value copying on hit/stale paths.

Implementation:

- Audit `Engine::get`, batch get, and lease responses for cloned value bytes.
- Consider response encoders that can write borrowed value bytes directly into
  the connection buffer.
- Preserve the public client API's owned response behavior.
- Keep engine ownership rules simple and safe; do not return references that
  outlive locks.

Checkpoint:

- Cached get and batch get p50 improve or the remaining copy is quantified.
- Tests prove returned values remain correct after subsequent writes/deletes.
- Docs record whether value-copy avoidance was implemented or rejected.

## Milestone 3: Pipelined Single-Connection Execution

Goal:

- Let one native connection keep multiple requests in flight when request ids
  are present.

Implementation:

- Define the ordering contract:
  - sequential ordered responses, or
  - concurrent execution with out-of-order responses matched by request id.
- If out-of-order responses are implemented, update protocol docs and client
  behavior.
- Add benchmarks for one connection with multiple outstanding requests.
- Bound in-flight work per connection.
- Preserve backpressure and frame-size limits.

Checkpoint:

- Protocol docs state response ordering rules.
- Client/server tests cover pipelined requests and request-id matching.
- Benchmarks show whether pipelining improves throughput or latency.

## Milestone 4: Sharded Engine Ownership

Goal:

- Remove the single global engine mutex as the main multi-client bottleneck.

Implementation:

- Profile or benchmark concurrent get/put workloads to confirm lock contention.
- Split the keyspace into fixed shards by namespace/key hash.
- Give each shard independent map, expiry index, tag index, lease state,
  eviction state, and metrics accounting where needed.
- Preserve namespace isolation, tag invalidation semantics, memory limits, TTL
  cleanup, cost accounting, and lease correctness.
- Decide whether memory pressure is enforced per shard or by coordinated global
  accounting; document the tradeoff.

Checkpoint:

- Concurrent client benchmarks scale materially better than the global-lock
  baseline.
- Existing engine behavior tests are ported or duplicated for sharded behavior.
- Expiration cleanup remains bounded.

## Milestone 5: Metrics And Counter Contention

Goal:

- Ensure observability does not dominate hot paths under concurrency.

Implementation:

- Measure atomic counter overhead in concurrent native benchmarks.
- Consider per-shard or per-worker counters with aggregation on metrics scrape.
- Keep `/metrics` side-effect free.
- Preserve metric names unless a rename is unavoidable.

Checkpoint:

- Concurrent benchmark numbers show whether metrics changes matter.
- Metrics tests prove scrapes remain observational.
- Docs describe any aggregation tradeoffs.

## Milestone 6: Syscall And Write Path Reduction

Goal:

- Reduce socket overhead where the codec and engine are no longer the main
  bottlenecks.

Implementation:

- Review whether reads/writes can be coalesced or batched safely.
- Consider vectored writes for header plus payload if it beats contiguous buffer
  encoding.
- Consider TCP_NODELAY defaults and document the choice.
- Consider client-side batching helpers without changing server semantics.

Checkpoint:

- Benchmarks isolate syscall/write-path changes.
- TCP and Unix socket results are reported separately.
- Any transport option change is documented.

## Milestone 7: Performance Documentation And Release Gate

Goal:

- Make the performance story accurate, reproducible, and hard to overstate.

Implementation:

- Refresh `docs/benchmarks.md` with final local output.
- Add an internal summary of what improved, what did not, and why.
- List the next major architectural decisions if targets remain unmet.
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
