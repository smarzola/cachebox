# Performance Uplift Baseline

Milestone 0 adds measurement coverage for the performance uplift loop. It does
not change production server behavior.

## Baseline Command

```sh
cargo run --bin cachebox-bench
```

## Added Coverage

The benchmark harness now includes:

- `concurrent_get_16`: 16 persistent clients repeatedly read the same cached
  key.
- `concurrent_put_16`: 16 persistent clients write unique keys concurrently.
- `short_connection_get`: connect, read one cached key, and close.

These rows run for native TCP and native Unix sockets. Existing sequential row
names remain unchanged.

## Key Local Results

Sequential hot paths remain in the same range as the prior native baseline:

| Scenario | Transport | p50 ns | Throughput ops/s |
| --- | --- | ---: | ---: |
| `single_key_get` | `loopback_tcp` | 24042 | 40743.72 |
| `single_key_put` | `loopback_tcp` | 25666 | 38452.78 |
| `single_key_get` | `loopback_unix` | 14125 | 67956.73 |
| `single_key_put` | `loopback_unix` | 15083 | 62406.60 |

New concurrent rows:

| Scenario | Transport | p50 ns | p95 ns | Throughput ops/s |
| --- | --- | ---: | ---: | ---: |
| `concurrent_get_16` | `loopback_tcp` | 101416 | 166042 | 152706.95 |
| `concurrent_put_16` | `loopback_tcp` | 102625 | 171500 | 143424.67 |
| `concurrent_get_16` | `loopback_unix` | 71541 | 121000 | 217476.58 |
| `concurrent_put_16` | `loopback_unix` | 74416 | 127833 | 189443.14 |

Short-lived connection rows:

| Scenario | Transport | p50 ns | p95 ns | Throughput ops/s |
| --- | --- | ---: | ---: | ---: |
| `short_connection_get` | `loopback_tcp` | 71666 | 78333 | 13089.03 |
| `short_connection_get` | `loopback_unix` | 32375 | 38000 | 28007.66 |

## Interpretation

The new concurrent rows show higher aggregate throughput than one persistent
client, but much worse per-operation latency. That is the measurement needed
before sharding or task-owned engine work: multi-client pressure is now visible
instead of inferred.

The short-connection rows quantify connection setup cost. They are not the
recommended production pattern for hot paths, but they give future client and
deployment work a concrete baseline.

## Still Not Measured

- True pipelining with multiple outstanding requests on one connection.
- Out-of-order response handling by request id.
- Per-shard or per-worker metrics overhead.
- CPU profiles or lock wait distributions.
- Payload-size sweeps for larger values.

Those are intentionally left for later milestones.
