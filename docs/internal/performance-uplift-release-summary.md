# Performance Uplift Release Summary

This note closes the performance uplift goal loop. The benchmark values are
local loopback measurements from one development machine and are not portable
production claims.

## Final Local Shape

Final benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected final p50 results:

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

The pipelined native Unix path is the only measured socket path near the
original single-digit-microsecond target. Sequential native socket operations
remain well above the stretch targets.

## What Improved

- The benchmark harness now covers sequential, concurrent, pipelined, and
  short-connection native TCP and Unix socket scenarios.
- Hot native decode avoids owned copies for get, delete, tag invalidation, and
  lease start.
- Native cached get response encoding can write borrowed value bytes directly
  while the engine lock is held.
- Native connections support bounded pipelining with out-of-order responses
  matched by `request_id`.
- Server engine ownership is sharded, so distributed-key workloads no longer
  pass through one global engine mutex.
- Metrics counters are striped and aggregated on scrape while preserving metric
  names and side-effect-free `/metrics` semantics.
- TCP sockets explicitly use `TCP_NODELAY`, and native response writes are
  opportunistically coalesced when responses are already queued.

## What Did Not Improve Enough

- Sequential native get and put remain dominated by per-request task spawning,
  scheduler handoff, socket I/O, and frame allocation.
- Hot-key concurrent get still concentrates on one shard by design.
- Tag invalidation got slower after sharding because the tag index is
  shard-local and invalidation must visit every shard.
- Metrics striping was structurally useful but did not materially move local
  benchmark latency.
- Write coalescing helped Unix pipelining, but TCP pipelining was flat to
  slightly worse in the final local run.

## Next Decisions

- Add adaptive execution for native connections: execute inline for
  non-pipelined single-request flows, and spawn only when a connection has
  multiple queued requests or a command can block other work.
- Reuse per-connection frame and response buffers to reduce allocation churn.
- Decide whether `get` should update access metadata on every hit or move to a
  cheaper sampled/access-log LRU signal.
- Replace all-shard tag invalidation with a global tag directory or a tag
  routing structure if tag invalidation must meet the sub-20 us Unix target.
- Add profile-backed syscall/task instrumentation before the next optimization
  pass, so future work separates socket cost from scheduler and lock cost.

## Release Gate

This loop produced a measured path and clear blockers, not full achievement of
the original stretch targets. The current implementation is faster and more
cache-native than the HTTP/2 data plane it replaced, but Redis-class local
latency still requires a larger execution model change.
