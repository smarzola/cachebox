# Performance Uplift Pipelining

Milestone 3 adds bounded single-connection pipelining to the native server.

## Change

- Native connections now use a reader loop and writer task.
- The reader loop reads complete frames without cancellation.
- Each frame is executed in a task and sends encoded response bytes through a
  bounded per-connection channel.
- In-flight request work is bounded by `MAX_IN_FLIGHT_PER_CONNECTION`.
- Responses may be returned out of order. Clients must match by `request_id`.

The public `NativeClient::request` method remains sequential. The protocol and
benchmark harness now exercise pipelining by writing many frames before reading
responses.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 23500 | 31792 | +35.3% |
| `pipelined_get_32` | `loopback_tcp` | n/a | 7608 | new |
| `concurrent_get_16` | `loopback_tcp` | 100583 | 111500 | +10.9% |
| `single_key_get` | `loopback_unix` | 13834 | 19250 | +39.1% |
| `pipelined_get_32` | `loopback_unix` | n/a | 7059 | new |
| `concurrent_get_16` | `loopback_unix` | 71500 | 81625 | +14.2% |

The pipeline row is the first local benchmark below `10 us` over both TCP and
Unix sockets. The tradeoff is also clear: spawning per request adds overhead to
non-pipelined sequential clients and hurts concurrent-client rows.

## Interpretation

This milestone proves the protocol can support multiple outstanding requests on
one connection and that request-id matching is required. It does not yet provide
the best server execution model for mixed workloads.

The next performance work should reduce the task-per-request overhead or make
the execution model adaptive:

- Keep inline execution for non-pipelined single-request flows.
- Switch to spawned execution only when multiple frames are queued or a
  connection is actually pipelining.
- Combine this with sharded engine ownership so spawned work can run in
  parallel without immediately contending on one global mutex.
