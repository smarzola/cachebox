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
engine_get engine 2007815 417 541 875 2007814.25 113 0 engine_cached_hit
engine_put engine 451883 1792 2250 2791 451882.98 52319618 0 engine_unique_keys
engine_tag_invalidate_8 engine 41400 8208 8542 10292 120452.73 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 40276 24458 28833 50791 40275.52 0 0 cached_hit
single_key_put loopback_tcp 37487 26083 28708 40917 37486.53 0 0 unique_keys
batch_get_32 loopback_tcp 14816 67209 73250 89708 14815.34 0 0 32_keys
lease_contention loopback_tcp 38568 25875 28333 40584 38567.98 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41347 24042 28375 38959 41347.00 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3782 36125 39167 54458 27392.86 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3790 260542 282500 323292 3789.62 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 36281 26541 30292 41625 36280.35 0 0 ttl_and_stale_ttl
single_key_get loopback_unix 67477 14167 17709 28958 67476.19 0 0 cached_hit
single_key_put loopback_unix 59479 15792 19167 29125 59478.36 0 0 unique_keys
batch_get_32 loopback_unix 19790 46958 59583 68416 19789.74 0 0 32_keys
lease_contention loopback_unix 61955 15417 19041 29000 61954.93 0 0 same_missing_key
tag_invalidate_empty loopback_unix 68453 14083 17292 28541 68451.85 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5567 27375 29667 40667 36690.48 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5583 177500 194792 227500 5582.57 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54665 17458 21416 30917 53239.93 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
