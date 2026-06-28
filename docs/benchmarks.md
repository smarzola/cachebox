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
engine_get engine 1994151 417 542 875 1994150.83 113 0 engine_cached_hit
engine_put engine 454278 1791 2250 2792 454277.77 52597438 0 engine_unique_keys
engine_tag_invalidate_8 engine 41107 8167 8959 11167 120501.19 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 29866 32958 38375 49250 29865.31 0 0 cached_hit
single_key_put loopback_tcp 30900 31166 35125 49292 30899.45 0 0 unique_keys
batch_get_32 loopback_tcp 13268 73333 83458 98125 13267.52 0 0 32_keys
lease_contention loopback_tcp 31144 31667 34958 47375 31143.87 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 28544 34250 38916 50750 28543.35 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3289 46625 50541 63083 21153.71 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3288 300417 328583 372250 3287.21 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31235 30917 34875 46416 31234.08 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 128704 7709 9046 9820 128671.96 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 135914 111833 192875 243542 135852.57 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 141754 109875 177000 208625 141724.72 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 133745 114042 183041 216708 133691.18 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12268 76625 84625 102209 12267.74 0 0 connect_get_close
single_key_get loopback_unix 50166 19792 22083 32375 50165.76 0 0 cached_hit
single_key_put loopback_unix 43045 22791 24917 37667 43044.15 0 0 unique_keys
batch_get_32 loopback_unix 14461 69875 73250 82208 14460.99 0 0 32_keys
lease_contention loopback_unix 46016 21250 24583 34875 46015.18 0 0 same_missing_key
tag_invalidate_empty loopback_unix 38166 26041 28834 40000 38165.99 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4230 37208 40125 52542 27302.12 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4200 236416 254333 283791 4199.83 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 41499 23458 25958 37750 41498.07 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 138752 7144 8535 9221 138735.41 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 188599 81750 140458 177875 188544.24 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 204145 77083 125542 147333 204088.03 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 176843 80709 133042 160750 176760.40 0 0 16_clients_unique_keys
short_connection_get loopback_unix 22420 41125 47666 66042 22419.07 0 0 connect_get_close
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
