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
engine_get engine 2007089 417 541 875 2007088.00 113 0 engine_cached_hit
engine_put engine 453296 1791 2209 2792 453295.96 52483526 0 engine_unique_keys
engine_tag_invalidate_8 engine 41516 8125 8375 10833 121633.28 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 40954 23500 28750 51459 40953.97 0 0 cached_hit
single_key_put loopback_tcp 38139 25708 28291 40458 38138.85 0 0 unique_keys
batch_get_32 loopback_tcp 15098 65417 71833 86875 15097.80 0 0 32_keys
lease_contention loopback_tcp 39569 25458 27792 40208 39568.56 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 42212 23125 27292 37750 42211.08 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3841 35375 38416 50250 28007.84 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3844 256541 278667 317708 3843.30 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 37116 26000 29875 41084 37115.13 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_tcp 152504 100583 169167 205875 152457.11 0 0 16_clients_cached_hit
concurrent_put_16 loopback_tcp 143758 102833 170042 204417 143719.17 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13228 70583 77417 96834 13227.48 0 0 connect_get_close
single_key_get loopback_unix 69021 13834 17375 28750 69020.04 0 0 cached_hit
single_key_put loopback_unix 61547 15208 18583 28666 61546.94 0 0 unique_keys
batch_get_32 loopback_unix 21234 44125 57958 66333 21233.44 0 0 32_keys
lease_contention loopback_unix 68231 14000 17916 27916 68230.07 0 0 same_missing_key
tag_invalidate_empty loopback_unix 71514 13125 17000 28584 71513.96 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5687 26333 29209 40083 38266.66 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5695 174125 192042 225917 5694.72 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54561 17167 20667 30542 54560.37 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_unix 215770 71500 124250 154584 215713.85 0 0 16_clients_cached_hit
concurrent_put_16 loopback_unix 190040 74292 128208 158916 189993.13 0 0 16_clients_unique_keys
short_connection_get loopback_unix 28193 32125 38125 55792 28192.52 0 0 connect_get_close
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
