# Cachebox Benchmarks

Cachebox benchmark results are local baselines only. Do not treat them as
portable performance claims without rerunning the command on the target machine.

## Command

```sh
cargo run --bin cachebox-bench
```

The harness starts loopback servers on random local ports and Unix socket paths,
opens persistent HTTP/2 prior-knowledge, native TCP, and native Unix socket
connections, performs warmup requests, then measures each scenario for a fixed
duration. It reports:

- `p50_ns`, `p95_ns`, and `p99_ns`: sampled request latency percentiles.
- `throughput_ops_s`: completed benchmark operations per second.
- `memory_used_bytes`: `cachebox_memory_used_bytes` from `/metrics` after HTTP
  scenarios, reported without triggering cleanup.
- `cost_score_total`: `cachebox_cost_score_total` from `/metrics` after HTTP
  scenarios, reported without triggering cleanup.

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
- `eviction_pressure`: writes against a 64 KiB memory cap.
- `cost_shaped_writes`: writes cheap large values, expensive small values, and
  TTL-bound cost metadata for cost-aware policy experiments.

## Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes
engine_get engine 2013206 417 541 792 2013205.08 113 0 engine_cached_hit
engine_put engine 451042 1791 2250 2750 451041.44 52222062 0 engine_unique_keys
engine_tag_invalidate_8 engine 41730 8125 8375 10917 121904.07 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8680 115708 131375 148792 8679.01 113 0 cached_hit
single_key_put loopback_h2 9376 101250 122417 141125 9375.29 1079967 0 unique_keys
batch_get_32 loopback_h2 4708 210666 227917 258458 4707.62 1083605 0 32_keys
lease_contention loopback_h2 7605 130375 142708 160959 7604.42 1083605 0 same_missing_key
tag_invalidate_empty loopback_h2 8198 122333 134833 150458 8197.86 1083605 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 910 117916 139875 160625 8290.65 1083605 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 910 1094584 1147833 1173041 909.86 1083605 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8596 118709 129834 148042 8595.36 2074539 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8950 106875 128292 148208 8949.71 65429 0 64KiB_cap
cost_shaped_writes loopback_h2 2764 363500 383542 416542 2763.63 4539977 4298864 cheap_large_expensive_small_mixed_ttl
single_key_get loopback_tcp 38525 25959 28708 40708 38524.59 0 0 cached_hit
single_key_put loopback_tcp 36924 26167 29375 40834 36923.82 0 0 unique_keys
batch_get_32 loopback_tcp 14528 68000 74833 88916 14527.84 0 0 32_keys
lease_contention loopback_tcp 38388 25875 28541 40500 38387.18 0 0 same_missing_key
tag_invalidate_empty loopback_tcp 41167 24084 27792 38833 41166.98 0 0 single_empty_invalidate
tag_invalidate_8 loopback_tcp 3788 36625 40000 51583 26983.48 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_tcp 3783 260958 285917 319625 3782.18 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_tcp 35782 26959 31542 41917 35781.82 0 0 ttl_and_stale_ttl
single_key_get loopback_unix 66843 14292 18000 29208 66842.52 0 0 cached_hit
single_key_put loopback_unix 58246 16208 19542 29583 58245.68 0 0 unique_keys
batch_get_32 loopback_unix 19147 52209 60000 68959 19146.44 0 0 32_keys
lease_contention loopback_unix 62139 15500 18958 28667 62138.54 0 0 same_missing_key
tag_invalidate_empty loopback_unix 66925 14250 17959 29084 66924.21 0 0 single_empty_invalidate
tag_invalidate_8 loopback_unix 5437 27083 30208 40375 37112.48 0 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_unix 5440 180750 202750 238583 5439.42 0 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_unix 54265 17750 21875 32708 54264.42 0 0 ttl_and_stale_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single invalidation requests, and the full multi-request write plus invalidate
workflow. HTTP/2 rows include loopback transport, h2 framing, request parsing,
response construction, and body transfer. Native TCP and Unix rows include
socket transport, fixed header framing, protocol codec work, and body transfer.
Native rows currently report `0` for memory and cost because the native data
plane does not expose metrics.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
