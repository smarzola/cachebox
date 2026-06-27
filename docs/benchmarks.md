# Benchmarks

Cachebox includes a local loopback benchmark harness for common cache paths.
Use it to compare changes on the same machine. Do not treat the checked-in
baseline as a portable performance claim.

## Run

```sh
cargo run --bin cachebox-bench
```

The harness starts local Cachebox servers on random loopback ports, opens a
persistent HTTP/2 prior-knowledge connection to each server, warms each
scenario, measures for a fixed duration, and prints one row per scenario.

## Columns

- `scenario`: benchmark scenario name.
- `transport`: current harness transport.
- `iterations`: completed measured operations.
- `p50_ns`, `p95_ns`, `p99_ns`: sampled latency percentiles.
- `throughput_ops_s`: measured operations per second.
- `memory_used_bytes`: `cachebox_memory_used_bytes` after the scenario.
- `cost_score_total`: `cachebox_cost_score_total` after the scenario.
- `notes`: short scenario description.

## Scenarios

- `single_key_get`: cached GET hit.
- `single_key_put`: unique-key PUT writes.
- `batch_get_32`: batch get for 32 keys.
- `lease_contention`: repeated lease attempts for the same missing key.
- `tag_invalidation_8`: put eight tagged values, then invalidate the tag.
- `ttl_heavy_writes`: writes with TTL and stale TTL headers.
- `eviction_pressure`: writes against a 64 KiB memory cap.
- `cost_shaped_writes`: writes cheap large values, expensive small values, and
  TTL-bound cost metadata for cost-aware policy experiments.

## Current Local Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
single_key_get loopback_h2 8685 115167 130875 146292 8684.62 113 0 cached_hit
single_key_put loopback_h2 2083 461416 800083 835459 2082.30 248565 0 unique_keys
batch_get_32 loopback_h2 4506 220292 239459 256375 4505.47 252203 0 32_keys
lease_contention loopback_h2 7573 131167 143625 156375 7572.94 252203 0 same_missing_key
tag_invalidation_8 loopback_h2 143 6999042 7066083 7095834 142.89 252203 0 put_then_invalidate
ttl_heavy_writes loopback_h2 969 1033667 1174375 1215375 968.57 373659 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 3297 314666 344166 376208 3296.07 65532 0 64KiB_cap
cost_shaped_writes loopback_h2 237 4212750 4536750 4575209 236.59 662812 505837 cheap_large_expensive_small_mixed_ttl
```
