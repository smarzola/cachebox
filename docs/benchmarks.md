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
engine_get engine 1986380 417 541 875 1986379.75 113 0 engine_cached_hit
engine_put engine 454404 1791 2250 2791 454403.36 52612054 0 engine_unique_keys
engine_tag_invalidate_8 engine 41039 8209 9000 11083 120088.36 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 31631 31875 35500 51792 31630.24 0 0 cached_hit
single_key_put loopback_tcp 30601 31625 35166 47750 30600.51 0 0 unique_keys
batch_get_32 loopback_tcp 13266 75458 82125 91917 13265.35 0 0 32_keys
lease_contention loopback_tcp 31105 32208 34583 46542 31104.03 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 28849 34084 38375 48208 28848.90 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3294 46583 49750 60584 21261.89 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3284 301792 331875 367375 3283.64 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31146 31167 34583 46083 31145.78 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 146144 6727 7986 8846 146119.07 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 137008 108750 198083 274458 136977.36 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 142734 109375 175875 209708 142708.87 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 134101 114625 182500 215292 134046.80 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12235 76958 84750 102958 12234.11 0 0 connect_get_close
single_key_get loopback_unix 50165 19792 21958 32209 50164.38 0 0 cached_hit
single_key_put loopback_unix 43170 22833 25042 37666 43169.95 0 0 unique_keys
batch_get_32 loopback_unix 14473 69875 73292 82792 14472.85 0 0 32_keys
lease_contention loopback_unix 47103 21083 24500 34417 47102.24 0 0 same_missing_key
tag_invalidate_empty loopback_unix 38200 25958 28875 39791 38199.93 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4226 37250 40416 51042 27303.93 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4208 235958 254750 284458 4207.81 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 41282 23709 25958 38083 41281.79 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 139360 7127 8494 9247 139335.85 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 190361 81166 138542 174708 190294.80 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 205397 76459 124750 146166 205345.24 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 178881 80167 131375 157625 178826.14 0 0 16_clients_unique_keys
short_connection_get loopback_unix 21996 40875 54834 84417 21995.64 0 0 connect_get_close
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
