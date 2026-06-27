# Benchmarks

Cachebox includes a local loopback benchmark harness for common cache paths.
Use it to compare changes on the same machine. Do not treat the checked-in
baseline as a portable performance claim.

## Run

```sh
cargo run --bin cachebox-bench
```

The harness starts local Cachebox servers on random loopback ports and Unix
socket paths, opens persistent HTTP/2 prior-knowledge, native TCP, and native
Unix socket connections, warms each scenario, measures for a fixed duration,
and prints one row per scenario.

## Columns

- `scenario`: benchmark scenario name.
- `transport`: `engine`, `loopback_h2`, `loopback_tcp`, or `loopback_unix`.
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
engine_get engine 2022749 417 541 792 2022748.66 113 0 engine_cached_hit
engine_put engine 452005 1791 2250 2791 452004.13 52333770 0 engine_unique_keys
engine_tag_invalidate_8 engine 41610 8166 8875 11250 121166.13 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8741 114625 129958 143167 8740.47 113 0 cached_hit
single_key_put loopback_h2 9320 102167 122375 135750 9319.68 1073583 0 unique_keys
batch_get_32 loopback_h2 4698 211125 229125 254416 4697.98 1077221 0 32_keys
lease_contention loopback_h2 7638 129875 141958 155958 7637.39 1077221 0 same_missing_key
tag_invalidate_empty loopback_h2 8157 122416 134875 147500 8156.70 1077221 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 913 117500 138958 147375 8296.01 1077221 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 912 1095291 1139500 1171208 911.89 1077221 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8662 117708 129541 142041 8661.43 2075679 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8925 112625 127166 139458 8924.98 65529 0 64KiB_cap
cost_shaped_writes loopback_h2 2774 361208 380875 405208 2773.77 4549737 4313874 cheap_large_expensive_small_mixed_ttl
single_key_get loopback_tcp 40838 24542 28125 39084 40836.92 0 0 cached_hit
single_key_put loopback_tcp 37141 26250 29375 40667 37140.45 0 0 unique_keys
batch_get_32 loopback_tcp 14739 67958 73708 86458 14738.38 0 0 32_keys
lease_contention loopback_tcp 38494 25917 28584 40333 38493.96 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41032 24166 28292 38750 41031.13 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3805 36166 39834 50917 27400.36 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3801 260042 282584 317417 3800.63 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 36107 26666 30750 41583 36106.48 0 0 ttl_and_stale_ttl
single_key_get loopback_unix 66483 14375 17875 28958 66482.55 0 0 cached_hit
single_key_put loopback_unix 58569 16125 19584 29250 58568.30 0 0 unique_keys
batch_get_32 loopback_unix 19581 47292 59459 68667 19580.14 0 0 32_keys
lease_contention loopback_unix 63667 15250 18667 28417 63666.08 0 0 same_missing_key
tag_invalidate_empty loopback_unix 67545 14167 17708 28875 67544.95 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5508 27000 29625 40916 37209.57 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5508 179375 196417 232125 5507.53 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54274 17750 21875 31833 54273.15 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. HTTP/2 rows include loopback transport, h2 framing, request parsing,
response construction, and body transfer. Native TCP and Unix rows include
socket transport, fixed header framing, protocol codec work, and body transfer.
Native rows currently report `0` for memory and cost because the native data
plane does not expose metrics.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
