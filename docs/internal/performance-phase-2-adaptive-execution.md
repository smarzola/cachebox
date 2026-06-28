# Performance Phase 2 Adaptive Execution

Milestone 1 avoids task-per-request overhead for non-pipelined native
connections while preserving spawned concurrent execution for actual pipelined
traffic.

## Change

- Native connections now read into a per-connection buffer and drain all
  complete frames already available.
- A single-frame batch executes inline and sends the response to the writer
  task without spawning an execution task.
- A multi-frame batch marks the connection as pipelined; subsequent frames use
  the existing bounded spawned execution path.
- The per-connection in-flight bound still applies to spawned pipelined work.
- Response semantics are unchanged: pipelined clients must match responses by
  `request_id`.

This keeps the reader loop cancellation-safe because it waits only on
`read_buf`, then parses complete frames from the owned buffer.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 32375 | 25375 | -21.6% |
| `single_key_put` | `loopback_tcp` | 31458 | 29875 | -5.0% |
| `pipelined_get_32` | `loopback_tcp` | 7838 | 5421 | -30.8% |
| `concurrent_get_16` | `loopback_tcp` | 111458 | 104166 | -6.5% |
| `single_key_get` | `loopback_unix` | 19958 | 16833 | -15.7% |
| `single_key_put` | `loopback_unix` | 23166 | 17833 | -23.0% |
| `pipelined_get_32` | `loopback_unix` | 6723 | 4691 | -30.2% |
| `concurrent_get_16` | `loopback_unix` | 80125 | 70083 | -12.5% |

The main target moved materially: sequential native get and put no longer pay a
spawn-per-request execution cost. Pipelined rows also improved because queued
frames are parsed from the connection buffer together before work is dispatched.

## Remaining Bottleneck

The process-only rows still show the non-socket hot path is much cheaper than
the socket path:

- `protocol_decode_get`: `375 ns`
- `protocol_encode_hit`: `167 ns`
- `engine_get_ref_encode`: `542 ns`

After adaptive execution, native Unix cached get is still around `16.8 us` and
native TCP cached get is around `25.4 us`. The next milestone should focus on
per-connection buffer reuse and reducing remaining read/write allocation and
copy costs.
