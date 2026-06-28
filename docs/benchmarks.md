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
- `pipelined_get_32`: one connection with 32 outstanding cached GET requests,
  reported as per-request average latency for each pipeline round.
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
engine_get engine 2009407 417 541 792 2009406.83 113 0 engine_cached_hit
engine_put engine 451685 1791 2250 2834 451684.92 52296650 0 engine_unique_keys
engine_tag_invalidate_8 engine 41594 8125 8417 11167 121312.96 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 30929 31792 34959 53792 30928.16 0 0 cached_hit
single_key_put loopback_tcp 30153 32542 35375 47958 30152.25 0 0 unique_keys
batch_get_32 loopback_tcp 14533 67958 75750 90417 14532.35 0 0 32_keys
lease_contention loopback_tcp 30716 32542 35250 47625 30715.50 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 31880 31042 33666 46167 31879.57 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3360 38792 42666 56458 25429.16 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3349 295917 320458 360500 3348.58 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31144 31000 34958 46500 31143.15 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 130496 7608 8888 9587 130467.87 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 136721 111500 190500 240042 136678.37 0 0 16_clients_cached_hit
concurrent_put_16 loopback_tcp 131688 111500 187083 231375 131654.25 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12333 76416 84250 103875 12332.06 0 0 connect_get_close
single_key_get loopback_unix 50846 19250 22042 32375 50845.18 0 0 cached_hit
single_key_put loopback_unix 45349 21334 24625 35167 45348.65 0 0 unique_keys
batch_get_32 loopback_unix 16475 62000 64833 73959 16474.04 0 0 32_keys
lease_contention loopback_unix 48430 20750 23958 33042 48429.20 0 0 same_missing_key
tag_invalidate_empty loopback_unix 52332 18542 21541 31708 52331.98 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4388 29750 32583 44042 33612.19 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4330 228375 247083 278375 4329.91 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 40958 23875 26250 37917 40957.53 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 140832 7059 8399 9037 140814.01 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 188265 81625 142208 180750 188194.10 0 0 16_clients_cached_hit
concurrent_put_16 loopback_unix 163171 83000 138459 168458 163119.87 0 0 16_clients_unique_keys
short_connection_get loopback_unix 22356 40667 53750 69583 22355.95 0 0 connect_get_close
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
Pipelined rows report per-request average latency from rounds where one
connection has 32 outstanding requests. Concurrent rows aggregate samples across
16 client connections and expose multi-client server behavior. Short-connection
rows include connection setup, one cached get, and connection close.
