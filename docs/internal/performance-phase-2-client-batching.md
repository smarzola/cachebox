# Performance Phase 2 Client Batching

Milestone 5 adds ergonomic client-side pipelining without changing server
semantics.

## Change

- `NativeClient::request_pipelined` accepts multiple `(Command, RequestPayload)`
  pairs.
- The client writes all request frames before reading responses.
- Responses are matched by `request_id` and returned in request order.
- The existing single-request client API is unchanged.
- Structured server errors are propagated as `ClientError::Server` after the
  response batch is read, so the connection remains aligned.

Unit tests cover out-of-order response matching, command mismatches, and server
error propagation. Spawned-client smoke tests exercise the public helper
against the real server.

## Benchmark

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | p50 ns | Notes |
| --- | --- | ---: | --- |
| `client_sequential_get_32` | `loopback_tcp` | 25535 | Official client, 32 sequential cached gets. |
| `client_pipelined_get_32` | `loopback_tcp` | 5904 | Official client, 32 pipelined cached gets. |
| `pipelined_get_32` | `loopback_tcp` | 5500 | Manual benchmark-client pipelining. |
| `client_sequential_get_32` | `loopback_unix` | 14809 | Official client, 32 sequential cached gets. |
| `client_pipelined_get_32` | `loopback_unix` | 5464 | Official client, 32 pipelined cached gets. |
| `pipelined_get_32` | `loopback_unix` | 4876 | Manual benchmark-client pipelining. |

The helper gives applications access to the already-supported server pipeline
path. It does not make individual non-pipelined requests faster; it amortizes
the socket round trip across independent requests.
