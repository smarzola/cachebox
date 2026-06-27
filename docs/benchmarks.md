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
engine_get engine 2010796 417 541 833 2010794.83 113 0 engine_cached_hit
engine_put engine 452303 1791 2209 2750 452302.55 52368338 0 engine_unique_keys
engine_tag_invalidate_8 engine 41523 8209 8458 10792 120426.31 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 40721 23875 28750 49708 40720.82 0 0 cached_hit
single_key_put loopback_tcp 38209 25708 28417 40416 38208.49 0 0 unique_keys
batch_get_32 loopback_tcp 15143 65291 71708 86041 15142.92 0 0 32_keys
lease_contention loopback_tcp 39673 25458 27709 39875 39672.83 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 42465 23125 27000 37792 42464.99 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3829 35583 38458 50750 27866.16 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3836 257291 279834 316875 3835.21 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 37006 26041 29750 41084 37005.94 0 0 ttl_and_stale_ttl
single_key_get loopback_unix 68610 14000 16917 28584 68609.12 0 0 cached_hit
single_key_put loopback_unix 61963 15125 18416 28542 61962.85 0 0 unique_keys
batch_get_32 loopback_unix 21320 43750 57625 66042 21319.98 0 0 32_keys
lease_contention loopback_unix 66172 14333 18166 27666 66171.11 0 0 same_missing_key
tag_invalidate_empty loopback_unix 69632 13667 16791 28250 69630.98 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5721 25792 29375 39916 38845.04 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5648 175166 197084 238583 5647.63 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54992 17167 20416 30333 54991.41 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
