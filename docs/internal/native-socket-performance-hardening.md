# Native Socket Performance Hardening

Milestone 9 focused on the native transport hot path after HTTP cache routes
were removed.

## Evidence

The benchmark rows showed native socket latency was dominated by transport and
codec overhead rather than engine work:

- Engine cached get p50: `417 ns`.
- Native TCP cached get p50 before: `24458 ns`.
- Native Unix cached get p50 before: `14167 ns`.

Inspection found avoidable per-request allocation and copying:

- `encode_request_frame` and `encode_response_frame` built a temporary payload
  vector, then copied it into a second frame vector.
- Native clients allocated request and response frame buffers on every request.
- The server allocated request and response frame buffers on every frame inside
  a persistent connection loop.

## Change

- Added `encode_request_frame_into` and `encode_response_frame_into` so callers
  can reuse a frame buffer and encode payload bytes directly after the header.
- Reused request and response buffers in `cachebox::client::NativeClient`.
- Reused request and response buffers per native server connection.
- Reused request and response buffers in the benchmark native client so measured
  rows exercise the optimized path.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 24458 | 23875 | -2.4% |
| `single_key_put` | `loopback_tcp` | 26083 | 25708 | -1.4% |
| `batch_get_32` | `loopback_tcp` | 67209 | 65291 | -2.9% |
| `tag_invalidate_empty` | `loopback_tcp` | 24042 | 23125 | -3.8% |
| `single_key_get` | `loopback_unix` | 14167 | 14000 | -1.2% |
| `single_key_put` | `loopback_unix` | 15792 | 15125 | -4.2% |
| `batch_get_32` | `loopback_unix` | 46958 | 43750 | -6.8% |
| `lease_contention` | `loopback_unix` | 15417 | 14333 | -7.0% |
| `tag_invalidate_8` | `loopback_unix` | 27375 | 25792 | -5.8% |

The gains are modest but consistent. The remaining gap between engine and
native rows is now less about obvious frame allocation churn and more about
syscall scheduling, per-request task/connection handling, frame decoding into
owned command structs, and the single global engine lock.

## Next Bottlenecks

The next meaningful performance work should target:

- Direct command execution from borrowed decode views for get/delete/tag paths,
  avoiding owned `String` and `Vec` construction where possible.
- Pipelined request handling so one connection can have multiple outstanding
  requests by request id.
- Sharded engine ownership to reduce global lock contention under concurrent
  clients.
- Per-shard metrics counters if the global metrics atomics become visible in
  profiles.
