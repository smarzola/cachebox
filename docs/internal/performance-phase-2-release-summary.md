# Performance Phase 2 Release Summary

Phase 2 reduced native socket overhead, separated remaining costs, and made the
official Rust client able to use pipelining ergonomically.

All numbers below are local loopback benchmark results from:

```sh
cargo run --bin cachebox-bench
```

They are useful for same-machine comparisons only. They are not portable
production performance claims.

## What Improved

| Area | Evidence |
| --- | --- |
| Scheduler overhead visibility | `tokio_spawn_ready` and `tokio_spawn_mpsc_response` rows isolate task handoff cost. |
| Codec and engine visibility | `protocol_decode_get`, `protocol_encode_hit`, `engine_get_ref_encode`, and sharded get rows isolate non-socket work. |
| Adaptive execution | Non-pipelined native requests execute inline until a connection proves it is pipelined. |
| Buffer reuse | Non-pipelined frames borrow from the per-connection read buffer and encode into a reusable response buffer. |
| Access accounting | Sharded get with access update measured `833 ns`; the no-access experiment measured `875 ns`, so production LRU updates stayed intact. |
| Tag routing | Empty Unix tag invalidation moved to `14.3 us`; populated eight-key Unix invalidation moved to `30.0 us`. |
| Client batching | Official Unix client pipelined get measured `5.7 us` per request versus `14.7 us` for 32 sequential gets. |

## Final Target Snapshot

| Target | Final p50 | Status |
| --- | ---: | --- |
| Native Unix cached get under `8 us` | `13.8 us` | Missed |
| Native TCP cached get under `15 us` | `25.1 us` | Missed |
| Native Unix single-key put under `12 us` | `17.0 us` | Missed |
| Native TCP single-key put under `20 us` | `25.3 us` | Missed |
| Native Unix pipelined cached get under `5 us` | `4.8 us` manual, `5.7 us` official client | Met for manual path; official helper is close |
| Unix `tag_invalidate_8` under `20 us` | `30.0 us` | Missed |

## What Did Not Move Enough

Sequential socket latency is still an order of magnitude higher than the
process-only hot path:

- `protocol_decode_get`: `375 ns`
- `protocol_encode_hit`: `167 ns`
- `engine_get_ref_encode`: `542 ns`
- `sharded_get_ref_encode`: `833 ns`
- Unix cached get: `13.8 us`
- TCP cached get: `25.1 us`

That means the next large gains are unlikely to come from ordinary codec or LRU
metadata tweaks. The remaining gap is primarily socket read/write wakeups,
runtime scheduling, and per-request async I/O overhead.

Tag invalidation no longer blindly scans every shard, but removing eight tagged
entries still costs about `30.0 us` over Unix sockets. The remaining work is
inside routed shard locking and entry removal from shard-local indexes.

## Next Architectural Choices

- Consider a dedicated blocking or polling native socket loop for the hot
  single-request path, measured against the Tokio `AsyncReadExt`/`AsyncWriteExt`
  loop.
- Investigate io_uring or platform-specific socket batching only if it can be
  kept behind the native transport boundary.
- Add process-only measurements for shard-local tag removal to separate tag
  directory routing cost from entry cleanup cost.
- Consider specialized multi-get or read-only pipeline helpers for clients that
  can tolerate batched application flow.
- Keep Redis compatibility out of the hot path; it does not address the
  measured native socket overhead.

## Release Gate

Phase 2 completed these required checks before the final checkpoint:

- Benchmark harness distinguishes scheduler/task, allocation, lock/access,
  codec, engine, socket, sequential, pipelined, and client-helper costs.
- Non-pipelined native get and put no longer spawn task-per-request work.
- Per-connection request and response hot buffers are reused where ownership
  allows.
- Get access accounting was measured and documented; it was not changed because
  it was not a material blocker.
- Tag invalidation uses shard routing rather than blind all-shard scanning.
- User-facing benchmark and usage docs describe the current performance and
  batching behavior without portable-performance claims.
- Milestone checkpoints were committed and pushed before moving on.
