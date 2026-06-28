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
engine_get engine 1997469 417 542 834 1997468.42 113 0 engine_cached_hit
engine_put engine 450630 1791 2250 2792 450629.16 52174270 0 engine_unique_keys
engine_tag_invalidate_8 engine 41385 8209 8875 11417 119689.79 0 0 remove_8_tagged_keys
protocol_decode_get process 2373398 375 417 500 2373397.21 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4656869 167 167 209 4656868.42 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1653445 542 584 750 1653444.04 120 0 engine_get_ref_plus_borrowed_encode
tokio_spawn_ready process 139707 6875 10583 12500 139706.64 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 107588 9375 10875 12583 107587.48 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 40236 24875 28166 49167 40235.30 0 0 cached_hit
single_key_put loopback_tcp 38229 25208 29250 40208 38228.73 0 0 unique_keys
batch_get_32 loopback_tcp 13796 71750 77792 88792 13795.63 0 0 32_keys
lease_contention loopback_tcp 39820 24709 29291 38667 39819.54 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 32602 30250 33833 44750 32601.69 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3628 42459 46083 56792 23442.26 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3615 275000 295792 322625 3614.17 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 37686 25334 30458 40250 37685.40 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 179040 5487 6774 7460 179036.21 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 157050 97791 164250 199083 157014.38 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 160741 97083 157500 183917 160708.68 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 147359 102417 169958 203916 147301.98 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13186 71042 77750 91667 13185.16 0 0 connect_get_close
single_key_get loopback_unix 66783 14708 18250 28916 66782.32 0 0 cached_hit
single_key_put loopback_unix 61670 15459 18333 29209 61669.10 0 0 unique_keys
batch_get_32 loopback_unix 15945 64375 67583 75292 15944.22 0 0 32_keys
lease_contention loopback_unix 72717 13333 17000 27333 72716.36 0 0 same_missing_key
tag_invalidate_empty loopback_unix 45473 21542 24792 35208 45472.74 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5667 32916 35875 45750 31180.13 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5677 175917 192041 215375 5676.48 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54435 16875 20042 36250 54434.69 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 207200 4753 6039 6733 207184.05 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 240208 63625 113209 140458 240139.14 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 259599 59042 104125 123000 259465.81 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 229165 62667 109750 130417 229101.89 0 0 16_clients_unique_keys
short_connection_get loopback_unix 28290 32458 38916 55875 28289.46 0 0 connect_get_close
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
