# Performance Uplift Write Path

Milestone 6 reduces response write overhead where it is safe to do so.

## Change

- Native TCP accepted sockets now explicitly enable `TCP_NODELAY`.
- The native connection writer opportunistically coalesces already queued
  response frames into one bounded write buffer.
- Coalescing is capped at 32 response frames or 64 KiB per write batch.
- Response semantics are unchanged: responses may still arrive out of order and
  clients must match by `request_id`.

The writer does not wait to build a larger batch. Waiting would improve syscall
amortization at the cost of adding latency to non-pipelined clients.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 32417 | 32375 | -0.1% |
| `pipelined_get_32` | `loopback_tcp` | 7673 | 7830 | +2.0% |
| `concurrent_get_16` | `loopback_tcp` | 112000 | 110833 | -1.0% |
| `short_connection_get` | `loopback_tcp` | 77625 | 76750 | -1.1% |
| `single_key_get` | `loopback_unix` | 19958 | 19916 | -0.2% |
| `pipelined_get_32` | `loopback_unix` | 7114 | 6875 | -3.4% |
| `concurrent_get_16` | `loopback_unix` | 81291 | 81583 | +0.4% |
| `short_connection_get` | `loopback_unix` | 41000 | 40750 | -0.6% |

The change helps Unix pipelined reads but does not improve TCP pipelined reads
in this local run. Sequential and short-connection rows are effectively flat.

## Interpretation

Opportunistic write coalescing is safe and cheap, but it is not the main latency
blocker. The remaining gap is likely dominated by per-request task spawning,
reader allocation, scheduler handoff, and lock scope around get/update access
tracking.

The next release-gate milestone should summarize this honestly: Cachebox now
has a better native transport than HTTP/2 for this project, but the local
targets for sequential native operations still require a larger execution model
change, most likely adaptive inline execution for non-pipelined requests.
