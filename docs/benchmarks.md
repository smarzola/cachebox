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
engine_get engine 2005155 417 541 792 2005154.92 113 0 engine_cached_hit
engine_put engine 454959 1791 2250 2791 454958.68 52676434 0 engine_unique_keys
engine_tag_invalidate_8 engine 41369 8166 8792 10708 121247.04 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 30547 32459 35792 52334 30546.93 0 0 cached_hit
single_key_put loopback_tcp 30839 31209 35167 49500 30838.34 0 0 unique_keys
batch_get_32 loopback_tcp 13215 73875 83750 99375 13214.47 0 0 32_keys
lease_contention loopback_tcp 31074 32208 35083 47292 31073.07 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 28930 34083 38416 49333 28929.99 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3279 46625 51917 61459 21130.87 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3296 299833 328459 368708 3295.22 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31184 31041 34916 47000 31183.10 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 126880 7842 9296 10106 126862.69 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 137584 110792 188833 236291 137534.89 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 140821 111375 177083 205916 140770.85 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 133384 114209 184375 219583 133354.09 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12250 76750 83959 101917 12249.21 0 0 connect_get_close
single_key_get loopback_unix 49355 20041 22375 32750 49354.83 0 0 cached_hit
single_key_put loopback_unix 42566 23083 25167 38042 42565.27 0 0 unique_keys
batch_get_32 loopback_unix 14368 70208 73542 83375 14367.83 0 0 32_keys
lease_contention loopback_unix 45938 21333 24791 34875 45937.54 0 0 same_missing_key
tag_invalidate_empty loopback_unix 37660 26250 29292 40083 37659.61 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4193 37583 40834 51792 26973.04 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4153 239500 257041 279375 4152.40 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 40924 23916 26084 38292 40923.50 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 149056 6651 8049 8691 149017.58 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 190585 80958 139125 174041 190524.21 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 205131 76500 124959 147125 205085.19 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 178200 80292 131666 157875 178124.66 0 0 16_clients_unique_keys
short_connection_get loopback_unix 22393 40958 49500 67083 22392.94 0 0 connect_get_close
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
