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
- `transport`: `engine`, `process`, `loopback_tcp`, or `loopback_unix`.
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
- `protocol_decode_get`: decode a prebuilt native GET frame without engine or
  socket work.
- `protocol_encode_hit`: encode a borrowed native HIT response without engine
  or socket work.
- `engine_get_ref_encode`: engine cached get with borrowed response encoding,
  without socket work.
- `sharded_get_ref_encode`: sharded engine cached get with shard mutex,
  access update, and borrowed response encoding, without socket work.
- `sharded_get_ref_no_access_encode`: same sharded get and encode path, but
  without updating approximate LRU access metadata.
- `tokio_spawn_ready`: spawn an empty Tokio task and await its completion.
- `tokio_spawn_mpsc_response`: spawn a Tokio task that sends a response vector
  over an `mpsc` channel, matching the current pipelined server handoff shape.
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
- `client_sequential_get_32`: official native client issuing 32 cached GET
  requests sequentially, reported as per-request average latency.
- `client_pipelined_get_32`: official native client issuing 32 cached GET
  requests with `request_pipelined`, reported as per-request average latency.
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
engine_get engine 1990564 417 542 834 1990563.25 113 0 engine_cached_hit
engine_put engine 451289 1791 2292 2875 451288.66 52250714 0 engine_unique_keys
engine_tag_invalidate_8 engine 41639 8125 8375 11333 121492.51 0 0 remove_8_tagged_keys
protocol_decode_get process 2341381 375 417 500 2341380.61 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4639683 167 208 209 4639682.23 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1649943 542 584 750 1649942.59 120 0 engine_get_ref_plus_borrowed_encode
sharded_get_ref_encode process 1113451 833 916 1166 1113450.40 128 0 shard_lock_get_ref_access_update_encode
sharded_get_ref_no_access_encode process 1044361 875 959 1250 1044360.83 138 0 shard_lock_get_ref_without_access_update_encode
tokio_spawn_ready process 137168 6958 10750 12750 137167.94 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 107782 9125 10916 12667 107781.21 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 39378 25084 28459 48417 39377.90 0 0 cached_hit
single_key_put loopback_tcp 38127 25250 29583 40584 38126.29 0 0 unique_keys
batch_get_32 loopback_tcp 13787 71834 77708 89667 13786.66 0 0 32_keys
lease_contention loopback_tcp 39656 24834 29209 38584 39655.47 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 40773 24375 27667 37708 40772.27 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3457 39417 43125 54917 25044.36 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3452 287292 307125 336000 3451.42 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 33347 29542 32667 44292 33346.58 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 178368 5501 6841 7522 178348.90 0 0 one_connection_32_outstanding_gets
client_sequential_get_32 loopback_tcp 39264 25429 26787 27576 39244.61 0 0 official_client_32_sequential_gets
client_pipelined_get_32 loopback_tcp 165792 5916 7321 8214 165763.01 0 0 official_client_32_pipelined_gets
concurrent_get_16 loopback_tcp 157334 97833 163000 196500 157300.91 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 162177 96125 156583 182416 162138.60 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 148822 101917 167625 197458 148768.77 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13308 70833 77709 90291 13307.45 0 0 connect_get_close
single_key_get loopback_unix 68133 13834 18250 28750 68132.85 0 0 cached_hit
single_key_put loopback_unix 56339 17042 19792 29583 56338.01 0 0 unique_keys
batch_get_32 loopback_unix 15875 65500 68583 76542 15874.34 0 0 32_keys
lease_contention loopback_unix 75511 13000 16792 27042 75510.03 0 0 same_missing_key
tag_invalidate_empty loopback_unix 68909 14292 18000 28625 68908.90 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5058 29958 33708 42875 33848.19 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5131 193375 210208 232250 5130.13 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 56104 17166 20958 30291 56103.85 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 203808 4826 6089 6782 203798.46 0 0 one_connection_32_outstanding_gets
client_sequential_get_32 loopback_unix 67936 14694 15617 16651 67917.99 0 0 official_client_32_sequential_gets
client_pipelined_get_32 loopback_unix 172384 5679 7218 7911 172355.06 0 0 official_client_32_pipelined_gets
concurrent_get_16 loopback_unix 236628 64458 115667 144125 236554.77 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 259716 59042 104167 123333 259637.06 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 216689 64958 112250 135167 216600.98 0 0 16_clients_unique_keys
short_connection_get loopback_unix 27911 32708 39000 53584 27910.78 0 0 connect_get_close
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. The `process` rows isolate codec, engine-plus-encode, and Tokio
task handoff costs without socket I/O. Tag invalidation is separated into
engine-only invalidation, single invalidation requests, and the full
multi-request write plus invalidate workflow. Native TCP and Unix rows include
socket transport, fixed header framing, protocol codec work, and body transfer.
Native rows currently report `0` for memory and cost because the native data
plane does not expose metrics. Pipelined rows report per-request average
latency from rounds where one connection has 32 outstanding requests.
`client_*_get_32` rows use the official Rust native client API and compare
sequential request/response flow with `request_pipelined`.
Concurrent rows aggregate samples across 16 client connections and expose
multi-client server behavior. The hot-key concurrent get row intentionally maps
to one shard; the distinct-key row better exercises sharded engine ownership.
Short-connection rows include connection setup, one cached get, and connection
close.
