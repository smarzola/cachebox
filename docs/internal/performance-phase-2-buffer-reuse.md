# Performance Phase 2 Buffer Reuse

Milestone 2 reduces allocation and ownership churn for non-pipelined native
connections.

## Change

- Native connections now keep the writer in the connection task until the
  connection proves it is pipelined.
- Non-pipelined frames are parsed as ranges inside the per-connection read
  buffer instead of being copied into a new request frame buffer.
- Non-pipelined responses encode into one reusable per-connection response
  buffer and write directly to the socket.
- Pipelined connections still switch to the bounded spawned execution path.
  Those frames are copied into owned buffers before spawning so mutable
  connection buffers are not shared across tasks.
- Response coalescing remains on the pipelined writer task only.

Max frame size enforcement is unchanged: oversized payload lengths close the
native connection before execution.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 25292 | 24875 | -1.6% |
| `single_key_put` | `loopback_tcp` | 29625 | 25208 | -14.9% |
| `lease_contention` | `loopback_tcp` | 28917 | 24709 | -14.6% |
| `concurrent_get_16` | `loopback_tcp` | 103583 | 97791 | -5.6% |
| `concurrent_get_16_distinct` | `loopback_tcp` | 103041 | 97083 | -5.8% |
| `single_key_get` | `loopback_unix` | 16584 | 14708 | -11.3% |
| `single_key_put` | `loopback_unix` | 18000 | 15459 | -14.1% |
| `lease_contention` | `loopback_unix` | 16333 | 13333 | -18.4% |
| `concurrent_get_16` | `loopback_unix` | 70292 | 63625 | -9.5% |
| `concurrent_get_16_distinct` | `loopback_unix` | 63750 | 59042 | -7.4% |

The largest sequential gains came from removing the inline response channel and
per-request response allocation. TCP cached get barely moved in this run, which
suggests its remaining cost is dominated by the socket read/write path,
scheduler wakeups, or engine access accounting rather than the buffer ownership
removed here.

## Remaining Bottleneck

The process-only hot path remains below `1 us`:

- `protocol_decode_get`: `375 ns`
- `protocol_encode_hit`: `167 ns`
- `engine_get_ref_encode`: `542 ns`

After this milestone, native Unix cached get is around `14.7 us` and native TCP
cached get is around `25.4 us`. Milestone 3 should measure get access
accounting directly, because cached get still performs approximate LRU mutation
inside the engine while the codec and borrowed response encoding are already
sub-microsecond.
