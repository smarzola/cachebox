# Benchmarks

Cachebox includes a local loopback benchmark harness for common cache paths.
Use it to compare changes on the same machine. Do not treat the checked-in
baseline as a portable performance claim.

## Run

```sh
cargo run --bin cachebox-bench
```

The harness starts local Cachebox native servers on random loopback ports and
Unix socket paths, opens native TCP and native Unix socket clients, warms each
scenario, measures for a fixed duration, and prints one row per scenario.

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
- `concurrent_get_16`: 16 clients repeatedly read the same cached key.
- `concurrent_put_16`: 16 clients write unique keys concurrently.
- `short_connection_get`: connect, read one cached key, and close.

## Current Local Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
engine_get engine 2030333 417 542 750 2030332.58 113 0 engine_cached_hit
engine_put engine 449934 1791 2292 2875 449933.49 52093534 0 engine_unique_keys
engine_tag_invalidate_8 engine 41463 8125 8417 11250 121527.95 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 40744 24042 28791 50333 40743.72 0 0 cached_hit
single_key_put loopback_tcp 38453 25666 28500 39792 38452.78 0 0 unique_keys
batch_get_32 loopback_tcp 15118 65875 71917 83875 15117.28 0 0 32_keys
lease_contention loopback_tcp 39674 25500 28042 39667 39673.50 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41949 23250 27709 37333 41948.27 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3831 35583 38291 50000 27849.96 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3835 257667 278583 307667 3834.72 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 37145 26041 29625 40750 37144.96 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_tcp 152761 101416 166042 197041 152706.95 0 0 16_clients_cached_hit
concurrent_put_16 loopback_tcp 143495 102625 171500 207042 143424.67 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13090 71666 78333 91792 13089.03 0 0 connect_get_close
single_key_get loopback_unix 67957 14125 17209 28625 67956.73 0 0 cached_hit
single_key_put loopback_unix 62407 15083 18500 28375 62406.60 0 0 unique_keys
batch_get_32 loopback_unix 21279 43709 57583 63833 21278.65 0 0 32_keys
lease_contention loopback_unix 66507 14333 18208 27584 66506.82 0 0 same_missing_key
tag_invalidate_empty loopback_unix 69433 13791 17166 27958 69432.25 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5781 25958 29209 40000 38817.34 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5792 172209 190083 212500 5791.94 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54532 17500 20500 29875 54531.55 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_unix 217555 71541 121000 145750 217476.58 0 0 16_clients_cached_hit
concurrent_put_16 loopback_unix 189490 74416 127833 157709 189443.14 0 0 16_clients_unique_keys
short_connection_get loopback_unix 28008 32375 38000 55209 28007.66 0 0 connect_get_close
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
Concurrent rows aggregate samples across 16 client connections and expose
multi-client server behavior. Short-connection rows include connection setup,
one cached get, and connection close.
