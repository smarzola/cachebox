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
engine_get engine 1828185 458 500 1209 1828184.16 113 0 engine_cached_hit
engine_put engine 454248 1791 2250 2750 454247.47 52593958 0 engine_unique_keys
engine_tag_invalidate_8 engine 41413 8167 8958 11334 120689.57 0 0 remove_8_tagged_keys
single_key_get loopback_tcp 41944 23166 28708 48750 41943.54 0 0 cached_hit
single_key_put loopback_tcp 38105 25709 28667 40167 38104.69 0 0 unique_keys
batch_get_32 loopback_tcp 15003 67000 72209 83416 15002.67 0 0 32_keys
lease_contention loopback_tcp 39467 25458 28000 39667 39466.67 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 42609 23000 27166 37291 42608.56 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3845 35417 38417 49791 28033.61 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3819 260542 284083 310917 3818.81 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 36664 26166 31000 40792 36663.83 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_tcp 153777 100458 165417 199167 153715.23 0 0 16_clients_cached_hit
concurrent_put_16 loopback_tcp 143614 102459 171166 206708 143514.11 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13327 70250 77042 92000 13326.03 0 0 connect_get_close
single_key_get loopback_unix 68333 14083 17084 28875 68332.44 0 0 cached_hit
single_key_put loopback_unix 60950 15250 18625 28458 60949.62 0 0 unique_keys
batch_get_32 loopback_unix 21598 43833 57542 63416 21597.12 0 0 32_keys
lease_contention loopback_unix 69065 13958 17875 27916 69064.44 0 0 same_missing_key
tag_invalidate_empty loopback_unix 71404 13208 16709 28375 71403.60 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5768 25916 29041 39417 39021.69 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5772 172583 190125 211542 5771.01 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54809 17083 20375 30000 54808.17 0 0 ttl_and_stale_ttl
concurrent_get_16 loopback_unix 222149 70000 119125 143625 222079.89 0 0 16_clients_cached_hit
concurrent_put_16 loopback_unix 189174 74708 128167 157708 189127.47 0 0 16_clients_unique_keys
short_connection_get loopback_unix 28232 32000 37916 55083 28231.77 0 0 connect_get_close
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
