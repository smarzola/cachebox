# Cachebox Benchmarks

Cachebox benchmark results are local baselines only. Do not treat them as
portable performance claims without rerunning the command on the target machine.

## Command

```sh
cargo run --bin cachebox-bench
```

The harness starts loopback HTTP servers on random local ports, opens persistent
HTTP/2 prior-knowledge connections, performs warmup requests, then measures each
scenario for a fixed duration. It reports:

- `p50_ns`, `p95_ns`, and `p99_ns`: sampled request latency percentiles.
- `throughput_ops_s`: completed benchmark operations per second.
- `memory_used_bytes`: `cachebox_memory_used_bytes` from `/metrics` after the
  scenario.
- `cost_score_total`: `cachebox_cost_score_total` from `/metrics` after the
  scenario.

## Scenarios

- `single_key_get`: cached GET hit.
- `single_key_put`: unique-key PUT writes.
- `engine_get`: in-process engine cached hit without HTTP.
- `engine_put`: in-process engine unique-key write without HTTP.
- `engine_tag_invalidate_8`: in-process invalidation of eight tagged keys
  without HTTP. Setup writes are outside the timed sample.
- `batch_get_32`: batch get for 32 keys.
- `lease_contention`: repeated lease attempts for the same missing key.
- `tag_invalidate_empty`: one HTTP tag invalidation request with no matching
  entries.
- `tag_invalidate_8`: one HTTP tag invalidation request after eight tagged
  values have been prepared. Setup writes are outside the timed sample.
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
engine_get engine 1305656 667 834 1292 1305655.95 113 0 engine_cached_hit
engine_put engine 579377 792 1083 1375 534034.37 67108804 0 engine_unique_keys
engine_tag_invalidate_8 engine 49472 6083 6667 8458 161973.27 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8691 115292 131000 145292 8690.41 113 0 cached_hit
single_key_put loopback_h2 9309 102417 121292 135167 9308.58 1072329 0 unique_keys
batch_get_32 loopback_h2 4511 219791 239708 261208 4510.58 1075967 0 32_keys
lease_contention loopback_h2 7643 129750 141875 154500 7642.88 1075967 0 same_missing_key
tag_invalidate_empty loopback_h2 8194 122542 134500 147917 8193.03 1075967 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 914 117083 139750 155750 8350.87 1075967 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 917 1092625 1135250 1156167 916.28 1075967 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8758 116500 128042 138375 8757.55 2085369 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 3571 294084 337167 364250 3570.97 65532 0 64KiB_cap
cost_shaped_writes loopback_h2 2787 360042 379667 398542 2786.87 4570633 4333387 cheap_large_expensive_small_mixed_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single HTTP invalidation requests, and the full multi-request write plus
invalidate workflow. HTTP/2 rows include loopback transport, h2 framing,
request parsing, response construction, and body transfer.
