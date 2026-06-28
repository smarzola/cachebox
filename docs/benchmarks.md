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
engine_get engine 2012483 417 500 625 2012482.25 113 0 engine_cached_hit
engine_put engine 450339 1791 2292 2958 450338.70 52140514 0 engine_unique_keys
engine_tag_invalidate_8 engine 41324 8208 9000 11334 120276.82 0 0 remove_8_tagged_keys
protocol_decode_get process 2370055 375 417 500 2370054.41 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4662256 167 167 209 4662255.61 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1657992 542 584 750 1657991.59 120 0 engine_get_ref_plus_borrowed_encode
tokio_spawn_ready process 135464 6958 10792 13209 135463.47 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 107176 9542 10875 12792 107175.46 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 30667 32333 35584 52333 30666.27 0 0 cached_hit
single_key_put loopback_tcp 30639 31500 35333 48208 30638.24 0 0 unique_keys
batch_get_32 loopback_tcp 13291 73458 83042 93834 13290.15 0 0 32_keys
lease_contention loopback_tcp 31037 32292 35084 46625 31036.08 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 28577 34291 38958 48875 28576.14 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3275 46458 50375 60583 21264.13 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3275 302000 326417 351667 3274.75 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31061 31250 35333 46333 31060.68 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 127360 7794 9292 10091 127334.66 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 136376 110625 193583 251417 136325.47 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 140256 112166 177208 206083 140227.04 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 121752 120166 217667 353417 121709.06 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 11921 78166 111542 157084 11920.08 0 0 connect_get_close
single_key_get loopback_unix 50584 19666 22084 31916 50582.97 0 0 cached_hit
single_key_put loopback_unix 43830 22250 24875 36750 43829.13 0 0 unique_keys
batch_get_32 loopback_unix 14904 64417 74584 115250 14903.52 0 0 32_keys
lease_contention loopback_unix 46278 21208 24459 34875 46277.42 0 0 same_missing_key
tag_invalidate_empty loopback_unix 38451 25958 28708 39292 38450.46 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4310 37041 40208 51292 27404.02 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 4390 230375 253750 309792 4389.88 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 42473 22958 25583 36875 42472.64 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 151328 6537 8058 8777 151321.58 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 188314 81625 141333 178708 188252.94 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 204846 76750 125041 146500 204783.56 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 179464 79916 131167 157291 179419.65 0 0 16_clients_unique_keys
short_connection_get loopback_unix 22464 40667 53709 75833 22463.90 0 0 connect_get_close
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
