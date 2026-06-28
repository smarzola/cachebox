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
engine_get engine 1969115 458 500 625 1969114.92 113 0 engine_cached_hit
engine_put engine 454767 1791 2250 2791 454766.70 52654162 0 engine_unique_keys
engine_tag_invalidate_8 engine 41533 8167 8667 11375 120919.96 0 0 remove_8_tagged_keys
protocol_decode_get process 2388985 375 416 500 2388984.20 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4638053 167 208 209 4638052.42 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1620685 542 584 750 1620684.46 120 0 engine_get_ref_plus_borrowed_encode
sharded_get_ref_encode process 1089022 834 917 1167 1089021.27 128 0 shard_lock_get_ref_access_update_encode
sharded_get_ref_no_access_encode process 1029171 917 1000 1250 1029170.27 138 0 shard_lock_get_ref_without_access_update_encode
tokio_spawn_ready process 137154 6917 10708 13042 137147.22 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 105587 9709 10875 12584 105586.29 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 39686 25083 28333 46500 39685.22 0 0 cached_hit
single_key_put loopback_tcp 38056 25333 29667 40709 38055.30 0 0 unique_keys
batch_get_32 loopback_tcp 13842 71583 77375 88917 13841.71 0 0 32_keys
lease_contention loopback_tcp 39718 24875 29208 38667 39717.10 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 40436 24667 27709 37750 40435.19 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3477 39584 43833 54125 25122.94 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3453 287167 307167 330750 3452.55 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 33381 29625 32708 44459 33380.34 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 179712 5458 6759 7490 179701.49 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 157541 97708 162958 196500 157490.80 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 160671 97042 157583 184458 160631.31 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 147518 102709 167667 195958 147465.32 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13232 70958 78000 90666 13231.90 0 0 connect_get_close
single_key_get loopback_unix 67071 14583 18000 28833 67070.66 0 0 cached_hit
single_key_put loopback_unix 56871 16709 19583 29666 56870.32 0 0 unique_keys
batch_get_32 loopback_unix 15978 65042 67959 75833 15977.76 0 0 32_keys
lease_contention loopback_unix 72870 13459 16958 27125 72869.33 0 0 same_missing_key
tag_invalidate_empty loopback_unix 68382 14625 17333 28625 68381.78 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5205 29500 33916 43584 34161.16 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5239 188667 206625 229458 5238.27 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 56169 17375 20667 30166 56166.87 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 213856 4623 5795 6438 213850.39 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 241402 63334 112708 139333 241339.07 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 261392 58625 104042 122750 261307.03 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 216145 64750 112667 135375 216044.12 0 0 16_clients_unique_keys
short_connection_get loopback_unix 27914 32584 38542 53458 27913.01 0 0 connect_get_close
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
Concurrent rows aggregate samples across 16 client connections and expose
multi-client server behavior. The hot-key concurrent get row intentionally maps
to one shard; the distinct-key row better exercises sharded engine ownership.
Short-connection rows include connection setup, one cached get, and connection
close.
