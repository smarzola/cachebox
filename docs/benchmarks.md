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
- `concurrent_get_16_distinct`: 16 clients repeatedly read separate cached
  keys, exposing shard distribution instead of one hot-key shard.
- `concurrent_put_16`: 16 clients write unique keys concurrently.
- `short_connection_get`: connect, read one cached key, and close.

## Current Local Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
engine_get engine 2044616 417 500 583 2044615.66 113 0 engine_cached_hit
engine_put engine 453740 1791 2209 2750 453739.04 52535030 0 engine_unique_keys
engine_tag_invalidate_8 engine 41095 8166 8917 11000 120716.32 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 30644 32375 35834 51208 30643.45 0 0 cached_hit
single_key_put loopback_tcp 30664 31334 35459 49917 30663.52 0 0 unique_keys
batch_get_32 loopback_tcp 13198 74917 83458 100000 13197.82 0 0 32_keys
lease_contention loopback_tcp 31592 31958 35458 48666 31591.53 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 28715 34084 38541 50208 28714.18 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3283 46458 50042 61000 21258.34 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3282 300709 328667 368250 3281.73 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31199 31041 35042 46875 31198.35 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 126848 7830 9335 9964 126831.58 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 136492 110833 192458 247000 136435.73 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 140827 111083 177375 208542 140784.76 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 134451 114041 182208 212917 134400.76 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12254 76750 84250 101166 12253.75 0 0 connect_get_close
single_key_get loopback_unix 49809 19916 22250 32709 49808.13 0 0 cached_hit
single_key_put loopback_unix 42910 22875 25167 37792 42909.51 0 0 unique_keys
batch_get_32 loopback_unix 14470 69916 73250 83834 14469.69 0 0 32_keys
lease_contention loopback_unix 46019 21291 24625 35125 46018.65 0 0 same_missing_key
tag_invalidate_empty loopback_unix 37828 26125 29250 40041 37827.82 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4209 37334 40375 51584 27263.06 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4180 238250 255333 289333 4179.44 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 41172 23791 26333 38125 41171.98 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 144032 6875 8442 9097 144024.85 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 187629 81583 143625 185875 187574.02 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 205444 76417 125000 146709 205392.82 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 178201 79750 131417 160542 178153.00 0 0 16_clients_unique_keys
short_connection_get loopback_unix 22660 40750 47375 65709 22659.73 0 0 connect_get_close
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. Native TCP and Unix rows include socket transport, fixed header
framing, protocol codec work, and body transfer. Native rows currently report
`0` for memory and cost because the native data plane does not expose metrics.
Pipelined rows report per-request average latency from rounds where one
connection has 32 outstanding requests. Concurrent rows aggregate samples across
16 client connections and expose multi-client server behavior. The hot-key
concurrent get row intentionally maps to one shard; the distinct-key row better
exercises sharded engine ownership. Short-connection rows include connection
setup, one cached get, and connection close.
