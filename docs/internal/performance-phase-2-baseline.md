# Performance Phase 2 Baseline

Milestone 0 adds measurement split rows for the remaining native socket cost.
It does not change production server behavior.

## Baseline Command

```sh
cargo run --bin cachebox-bench
```

## New Measurement Rows

| Scenario | Transport | p50 ns | What It Isolates |
| --- | --- | ---: | --- |
| `protocol_decode_get` | `process` | 375 | Decoding a prebuilt native GET frame. |
| `protocol_encode_hit` | `process` | 167 | Encoding a borrowed HIT response frame. |
| `engine_get_ref_encode` | `process` | 542 | Engine get with access update plus borrowed response encoding. |
| `tokio_spawn_ready` | `process` | 6958 | Spawning an empty Tokio task and awaiting it. |
| `tokio_spawn_mpsc_response` | `process` | 9542 | Spawning a task that sends one response vector over an `mpsc` channel. |

## Interpretation

The codec and engine-plus-encode paths are not the dominant remaining cost:
they are sub-microsecond in this local run. The task handoff shape is much
larger. An empty spawn/join is about `7 us`, and spawn plus channel response
handoff is about `9.5 us` before any socket read/write cost.

That supports the next milestone's direction: adaptive native execution should
avoid task-per-request overhead for non-pipelined single-request flows while
keeping spawned concurrent execution for actual pipelining.

## Existing Socket Rows

Selected p50 rows from the same run:

| Scenario | Transport | p50 ns |
| --- | --- | ---: |
| `single_key_get` | `loopback_tcp` | 32333 |
| `single_key_put` | `loopback_tcp` | 31500 |
| `pipelined_get_32` | `loopback_tcp` | 7794 |
| `single_key_get` | `loopback_unix` | 19666 |
| `single_key_put` | `loopback_unix` | 22250 |
| `pipelined_get_32` | `loopback_unix` | 6537 |

The pipelined rows already amortize part of the task/socket cost. Sequential
rows still pay the full server task handoff and socket round trip per request.
