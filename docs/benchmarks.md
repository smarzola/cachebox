# Benchmarks

Cachebox includes a local loopback benchmark harness for common cache paths.
Use it to compare changes on the same machine. Do not treat the checked-in
baseline as a portable performance claim.

## Run

```sh
cargo run --bin cachebox-bench
```

The harness starts local Cachebox native servers on random loopback ports and
Unix socket paths, opens persistent native TCP and native Unix socket
connections, warms each scenario, measures for a fixed duration, and prints one
row per scenario.

## Columns

- `scenario`: benchmark scenario name.
- `transport`: `engine`, `loopback_tcp`, or `loopback_unix`.
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

## Current Local Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
engine_get engine 2012896 417 541 833 2012895.58 113 0 engine_cached_hit
engine_put engine 453250 1791 2250 2791 453249.66 52478190 0 engine_unique_keys
engine_tag_invalidate_8 engine 41668 8167 8417 10625 120923.28 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 40418 24375 28917 50500 40417.47 0 0 cached_hit
single_key_put loopback_tcp 37461 26083 28833 40916 37460.54 0 0 unique_keys
batch_get_32 loopback_tcp 14896 66375 72917 88375 14895.54 0 0 32_keys
lease_contention loopback_tcp 38593 25834 28417 40541 38592.62 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41782 23667 28291 38917 41781.12 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3798 35791 38917 50791 27671.46 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3803 259542 281542 320791 3802.96 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 36517 26417 30167 41416 36516.67 0 0 ttl_and_stale_ttl
single_key_get loopback_unix 66793 14292 17875 29000 66792.49 0 0 cached_hit
single_key_put loopback_unix 59115 15959 19333 29334 59114.81 0 0 unique_keys
batch_get_32 loopback_unix 20909 44583 58417 67209 20907.96 0 0 32_keys
lease_contention loopback_unix 63928 15125 18459 28334 63927.17 0 0 same_missing_key
tag_invalidate_empty loopback_unix 67022 14250 18042 29042 67021.31 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5438 27333 30792 41000 36832.50 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5557 177625 195333 227333 5556.68 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54746 17583 21583 31000 54745.62 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
