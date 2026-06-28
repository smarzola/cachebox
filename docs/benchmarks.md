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
engine_get engine 1957873 458 541 667 1957872.27 113 0 engine_cached_hit
engine_put engine 452068 1791 2250 2792 452067.51 52341078 0 engine_unique_keys
engine_tag_invalidate_8 engine 41445 8125 8709 11292 121204.19 0 0 remove_8_tagged_keys
protocol_decode_get process 2374419 375 416 500 2374418.31 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4603213 167 208 209 4603212.81 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1615794 542 584 750 1615793.66 120 0 engine_get_ref_plus_borrowed_encode
sharded_get_ref_encode process 1117574 833 916 1125 1117573.67 128 0 shard_lock_get_ref_access_update_encode
sharded_get_ref_no_access_encode process 1046353 875 959 1209 1046352.91 138 0 shard_lock_get_ref_without_access_update_encode
tokio_spawn_ready process 138913 6916 10625 13125 138912.81 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 108326 8916 10833 12541 108325.37 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 40321 24834 28166 48750 40320.02 0 0 cached_hit
single_key_put loopback_tcp 38320 25166 29208 39917 38319.90 0 0 unique_keys
batch_get_32 loopback_tcp 13858 71667 77292 89625 13857.41 0 0 32_keys
lease_contention loopback_tcp 39596 24875 29291 38750 39595.81 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 32779 30292 33416 44625 32778.58 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3606 42333 45667 57625 23515.58 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3611 275084 295916 318667 3610.98 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 37896 25334 30375 40250 37895.43 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 178752 5493 6815 7550 178731.24 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 157320 97666 163875 197792 157266.20 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 162192 96208 156333 182042 162155.11 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 150165 99958 163792 194667 149724.11 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13247 70833 77208 91083 13246.55 0 0 connect_get_close
single_key_get loopback_unix 67359 14458 18292 29042 67358.39 0 0 cached_hit
single_key_put loopback_unix 62543 15125 18333 29292 62542.40 0 0 unique_keys
batch_get_32 loopback_unix 16176 64083 67125 75292 16175.46 0 0 32_keys
lease_contention loopback_unix 73532 13208 16667 26875 73531.55 0 0 same_missing_key
tag_invalidate_empty loopback_unix 46176 21416 24667 34667 46175.27 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5673 32833 36167 46834 31280.09 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5631 177250 194000 219042 5630.71 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 55250 17000 19750 29625 55249.20 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 209312 4708 5929 6606 209304.67 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 240866 63500 112833 139541 240788.91 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 259243 59125 104542 124375 259161.27 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 230265 62208 109208 129917 230201.88 0 0 16_clients_unique_keys
short_connection_get loopback_unix 27672 32792 39000 55750 27671.96 0 0 connect_get_close
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
