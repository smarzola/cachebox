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
  scenario, reported without triggering cleanup.
- `cost_score_total`: `cachebox_cost_score_total` from `/metrics` after the
  scenario, reported without triggering cleanup.

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
engine_get engine 2026486 417 542 792 2026485.07 113 0 engine_cached_hit
engine_put engine 448067 1791 2292 2958 448066.72 51876962 0 engine_unique_keys
engine_tag_invalidate_8 engine 41476 8208 8917 11375 120095.62 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8790 113541 129833 142875 8789.75 113 0 cached_hit
single_key_put loopback_h2 9323 102083 121667 135584 9322.28 1073925 0 unique_keys
batch_get_32 loopback_h2 4683 211791 230375 255458 4682.07 1077563 0 32_keys
lease_contention loopback_h2 7653 129875 142125 155041 7652.90 1077563 0 same_missing_key
tag_invalidate_empty loopback_h2 8145 122458 135042 146750 8144.66 1077563 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 910 117916 139916 154375 8269.17 1077563 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 907 1099083 1147667 1183792 906.72 1077563 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8617 118125 130083 143042 8616.40 2070891 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8993 106875 125958 138709 8992.64 65499 0 64KiB_cap
cost_shaped_writes loopback_h2 2768 361834 381292 400833 2767.96 4539777 4304868 cheap_large_expensive_small_mixed_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single HTTP invalidation requests, and the full multi-request write plus
invalidate workflow. HTTP/2 rows include loopback transport, h2 framing,
request parsing, response construction, and body transfer.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
