# Benchmarks

Cachebox includes a local loopback benchmark harness for common cache paths.
Use it to compare changes on the same machine. Do not treat the checked-in
baseline as a portable performance claim.

## Run

```sh
cargo run --bin cachebox-bench
```

The harness starts local Cachebox servers on random loopback ports, opens
persistent HTTP/2 prior-knowledge and native TCP connections, warms each
scenario, measures for a fixed duration, and prints one row per scenario.

## Columns

- `scenario`: benchmark scenario name.
- `transport`: `engine`, `loopback_h2`, or `loopback_tcp`.
- `iterations`: completed measured operations.
- `p50_ns`, `p95_ns`, `p99_ns`: sampled latency percentiles.
- `throughput_ops_s`: measured operations per second.
- `memory_used_bytes`: `cachebox_memory_used_bytes` after the scenario,
  reported without triggering cleanup.
- `cost_score_total`: `cachebox_cost_score_total` after the scenario, reported
  without triggering cleanup.
- `notes`: short scenario description.

## Scenarios

- `single_key_get`: cached GET hit.
- `single_key_put`: unique-key PUT writes.
- `engine_get`: in-process engine cached hit without HTTP.
- `engine_put`: in-process engine unique-key write without HTTP.
- `engine_tag_invalidate_8`: in-process invalidation of eight tagged keys
  without HTTP. Setup writes are outside the timed sample.
- `batch_get_32`: batch get for 32 keys.
- `lease_contention`: repeated lease attempts for the same missing key.
- `tag_invalidate_empty`: one tag invalidation request with no matching entries.
- `tag_invalidate_8`: one tag invalidation request after eight tagged values
  have been prepared. Setup writes are outside the timed sample.
- `tag_workflow_put8_invalidate`: full workflow that puts eight tagged values,
  then invalidates the tag.
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
engine_get engine 2030825 417 541 792 2030824.32 113 0 engine_cached_hit
engine_put engine 452923 1791 2250 2750 452922.02 52440258 0 engine_unique_keys
engine_tag_invalidate_8 engine 41543 8125 8750 11250 121512.16 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8755 114000 130625 144625 8754.70 113 0 cached_hit
single_key_put loopback_h2 9343 101541 121917 140875 9342.32 1076205 0 unique_keys
batch_get_32 loopback_h2 4706 210709 229750 262625 4705.25 1079843 0 32_keys
lease_contention loopback_h2 7633 129750 142250 160458 7632.71 1079843 0 same_missing_key
tag_invalidate_empty loopback_h2 8173 122292 135250 153917 8172.91 1079843 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 918 116917 139875 153875 8334.63 1079843 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 919 1089208 1141417 1170083 918.64 1079843 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8642 117875 129625 146375 8641.15 2076021 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8991 106375 126375 146584 8990.66 65509 0 64KiB_cap
cost_shaped_writes loopback_h2 2783 360875 382875 417042 2782.10 4557837 4327383 cheap_large_expensive_small_mixed_ttl
single_key_get loopback_tcp 40507 25083 27833 39292 40506.57 0 0 cached_hit
single_key_put loopback_tcp 37220 26125 28958 41083 37218.52 0 0 unique_keys
batch_get_32 loopback_tcp 14827 67041 73416 90375 14826.18 0 0 32_keys
lease_contention loopback_tcp 38614 25792 28375 40500 38613.48 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41423 24000 28208 38833 41422.29 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3833 36208 38875 51333 27381.19 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3827 257917 281000 322666 3826.94 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 36305 26500 30667 41667 36304.37 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. HTTP/2 rows include loopback transport, h2 framing, request parsing,
response construction, and body transfer. Native TCP rows include loopback
transport, fixed header framing, protocol codec work, and body transfer. Native
rows currently report `0` for memory and cost because the native data plane does
not expose metrics.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
