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
engine_get engine 1974293 417 542 875 1974292.59 113 0 engine_cached_hit
engine_put engine 454241 1791 2250 2792 454240.96 52593146 0 engine_unique_keys
engine_tag_invalidate_8 engine 41212 8208 9041 11416 119788.00 0 0 remove_8_tagged_keys
protocol_decode_get process 2380641 375 417 500 2380640.90 0 0 decode_prebuilt_get_frame
protocol_encode_hit process 4616783 167 208 209 4616782.23 0 0 encode_borrowed_hit_response
engine_get_ref_encode process 1646212 542 584 750 1646211.45 120 0 engine_get_ref_plus_borrowed_encode
sharded_get_ref_encode process 1111995 833 917 1166 1111994.86 128 0 shard_lock_get_ref_access_update_encode
sharded_get_ref_no_access_encode process 1042283 875 1000 1209 1042282.22 138 0 shard_lock_get_ref_without_access_update_encode
tokio_spawn_ready process 136071 6958 10792 13208 136070.58 0 0 spawn_empty_task_and_join
tokio_spawn_mpsc_response process 108190 8958 10875 12750 108189.18 0 0 spawn_task_send_response_vec
single_key_get loopback_tcp 40053 24959 28291 48833 40052.72 0 0 cached_hit
single_key_put loopback_tcp 37998 25292 29833 40625 37997.46 0 0 unique_keys
batch_get_32 loopback_tcp 13829 71667 77916 88500 13828.03 0 0 32_keys
lease_contention loopback_tcp 39941 24709 29209 38375 39940.67 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 40544 24625 27791 37750 40543.95 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3439 39500 43250 53916 25046.78 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3449 287875 308000 332292 3448.87 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 33302 29666 32708 44375 33301.76 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_tcp 178656 5500 6806 7579 178636.25 0 0 one_connection_32_outstanding_gets
client_sequential_get_32 loopback_tcp 39104 25535 26742 27595 39094.25 0 0 official_client_32_sequential_gets
client_pipelined_get_32 loopback_tcp 166208 5904 7276 8134 166178.90 0 0 official_client_32_pipelined_gets
concurrent_get_16 loopback_tcp 157502 97625 163625 197584 157451.59 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_tcp 161458 96541 157125 184500 161412.13 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_tcp 148441 102208 167250 196000 148388.32 0 0 16_clients_unique_keys
short_connection_get loopback_tcp 13225 71000 77542 90083 13224.84 0 0 connect_get_close
single_key_get loopback_unix 66700 14666 18417 28875 66699.65 0 0 cached_hit
single_key_put loopback_unix 56584 16959 19708 29625 56583.55 0 0 unique_keys
batch_get_32 loopback_unix 16671 61792 67875 76292 16670.55 0 0 32_keys
lease_contention loopback_unix 71530 13541 17417 27542 71529.11 0 0 same_missing_key
tag_invalidate_empty loopback_unix 67878 14833 17792 28667 67876.98 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5140 29875 33708 42667 33787.78 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5185 191125 208625 227959 5184.58 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 55778 17542 20875 30500 55777.30 0 0 ttl_and_stale_ttl
pipelined_get_32 loopback_unix 202272 4876 6138 6842 202271.46 0 0 one_connection_32_outstanding_gets
client_sequential_get_32 loopback_unix 67712 14809 15842 16653 67683.92 0 0 official_client_32_sequential_gets
client_pipelined_get_32 loopback_unix 179776 5464 6889 7613 179769.69 0 0 official_client_32_pipelined_gets
concurrent_get_16 loopback_unix 234223 64875 117625 151209 234159.08 0 0 16_clients_cached_hit
concurrent_get_16_distinct loopback_unix 260758 58708 104000 123167 260680.95 0 0 16_clients_distinct_cached_hits
concurrent_put_16 loopback_unix 215751 64833 112000 134875 215655.65 0 0 16_clients_unique_keys
short_connection_get loopback_unix 27735 32750 38917 53292 27734.64 0 0 connect_get_close
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
