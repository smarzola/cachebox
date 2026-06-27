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
engine_get engine 2044730 417 500 666 2044729.74 113 0 engine_cached_hit
engine_put engine 453726 1791 2250 2792 453725.39 52533406 0 engine_unique_keys
engine_tag_invalidate_8 engine 41310 8167 8750 11500 120239.41 0 0 remove_8_tagged_keys
single_key_get loopback_h2 8851 112625 129750 142083 8850.78 113 0 cached_hit
single_key_put loopback_h2 9351 101709 121125 134959 9350.21 1077117 0 unique_keys
batch_get_32 loopback_h2 4692 211500 229375 254291 4691.90 1080755 0 32_keys
lease_contention loopback_h2 7638 129916 141708 154167 7636.88 1080755 0 same_missing_key
tag_invalidate_empty loopback_h2 8198 122250 134625 149375 8197.34 1080755 0 single_empty_invalidate
tag_invalidate_8 loopback_h2 915 116417 138584 159959 8355.38 1080755 0 single_invalidate_8_tagged_keys
tag_workflow_put8_invalidate loopback_h2 916 1092083 1140500 1167000 915.70 1080755 0 8_puts_plus_invalidate
ttl_heavy_writes loopback_h2 8699 117292 129291 141417 8698.42 2083431 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 8884 114125 126291 140417 8883.44 65523 0 64KiB_cap
cost_shaped_writes loopback_h2 2737 365916 386250 420875 2736.36 4525595 4258337 cheap_large_expensive_small_mixed_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. Tag invalidation is separated into engine-only invalidation,
single HTTP invalidation requests, and the full multi-request write plus
invalidate workflow. HTTP/2 rows include loopback transport, h2 framing,
request parsing, response construction, and body transfer.
Memory-pressure writes use indexed expiry cleanup plus bounded-sample
approximate LRU, so the eviction pressure row avoids a full keyspace scan per
evicted entry.
