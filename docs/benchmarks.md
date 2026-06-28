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
engine_get engine 1989633 417 542 875 1989632.09 113 0 engine_cached_hit
engine_put engine 451764 1791 2250 2833 451763.85 52305814 0 engine_unique_keys
engine_tag_invalidate_8 engine 41379 8166 8500 11333 120872.76 0 0 remove_8_tagged_keys
protocol_decode_get process 2406702 375 375 500 2406701.80 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4579334 167 208 250 4579333.05 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1644622 542 584 750 1644621.73 120 0 engine_get_ref_plus_borrowed_encode
tokio_spawn_ready process 138034 6917 10708 13125 138033.95 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 107839 9125 10875 12958 107838.42 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 38172 25375 30250 49958 38171.29 0 0 cached_hit
single_key_put loopback_tcp 32410 29875 33333 45750 32409.83 0 0 unique_keys
batch_get_32 loopback_tcp 13473 73625 79667 91083 13472.84 0 0 32_keys
lease_contention loopback_tcp 34404 28958 31792 44125 34403.30 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 30886 31542 36041 46291 30885.99 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3429 44000 47417 58334 22634.84 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3424 289709 311125 340291 3423.34 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 31292 31167 34000 45750 31291.66 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 181472 5421 6679 7506 181438.06 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_tcp 146391 104166 178375 221000 146348.80 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 151560 102750 166709 194584 151532.72 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 143605 105917 171792 200500 143549.97 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 12611 74458 81334 94292 12611.00 0 0 connect_get_close
single_key_get loopback_unix 58040 16833 19208 28709 58039.91 0 0 cached_hit
single_key_put loopback_unix 54331 17833 20833 30834 54330.77 0 0 unique_keys
batch_get_32 loopback_unix 15109 67000 71000 78000 15108.99 0 0 32_keys
lease_contention loopback_unix 58278 16375 19958 29834 58277.14 0 0 same_missing_key
tag_invalidate_empty loopback_unix 42703 23292 26292 36916 42702.10 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 4971 31458 37458 44875 30517.88 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5097 194875 216792 242792 5096.89 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 52133 18542 21875 31458 52132.70 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 210432 4691 5833 6528 210403.86 0 0 one_connection_32_outstanding_gets
concurrent_get_16 loopback_unix 216200 70083 128750 166584 216137.02 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 240889 64166 110625 131042 240827.75 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 210340 67000 114750 137875 210293.60 0 0 16_clients_unique_keys
short_connection_get loopback_unix 24816 36791 43625 59708 24815.50 0 0 connect_get_close
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
