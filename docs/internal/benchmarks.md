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
engine_get engine 2035285 417 541 750 2035283.90 113 0 engine_cached_hit
engine_put engine 453292 1791 2250 2791 453291.81 52483062 0 engine_unique_keys
engine_tag_invalidate_8 engine 41488 8167 8917 11292 120624.04 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8741 114667 130959 143541 8740.93 113 0 cached_hit
single_key_put loopback_h2 9318 102250 121709 135125 9317.38 1073355 0 unique_keys
batch_get_32 loopback_h2 4700 211209 228542 248042 4699.35 1076993 0 32_keys
lease_contention loopback_h2 7649 129833 142041 155167 7648.12 1076993 0 same_missing_key
tag_invalidate_empty loopback_h2 8206 122292 134458 147541 8205.60 1076993 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 913 117125 138458 152208 8355.45 1076993 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 913 1094167 1136208 1162791 912.35 1076993 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8732 117000 128917 140584 8731.62 2083431 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8901 114417 126625 139500 8900.85 65532 0 64KiB_cap
cost_shaped_writes loopback_h2 2782 360833 379916 396791 2781.17 4564385 4325882 cheap_large_expensive_small_mixed_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single HTTP invalidation requests, and the full multi-request write plus
invalidate workflow. HTTP/2 rows include loopback transport, h2 framing,
request parsing, response construction, and body transfer.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
