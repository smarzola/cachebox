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
- `batch_get_32`: batch get for 32 keys.
- `lease_contention`: repeated lease attempts for the same missing key.
- `tag_invalidation_8`: put eight tagged values, then invalidate the tag.
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
engine_get engine 1326704 667 791 958 1326703.12 113 0 engine_cached_hit
engine_put engine 579377 792 1125 1542 521942.90 67108804 0 engine_unique_keys
single_key_get loopback_h2 8689 115709 131667 141959 8688.88 113 0 cached_hit
single_key_put loopback_h2 9418 101875 120750 131541 9417.87 1084755 0 unique_keys
batch_get_32 loopback_h2 4518 220417 237834 255667 4517.38 1088393 0 32_keys
lease_contention loopback_h2 7643 130292 141917 151250 7642.70 1088393 0 same_missing_key
tag_invalidation_8 loopback_h2 912 1095250 1137583 1154500 911.37 1088393 0 put_then_invalidate
ttl_heavy_writes loopback_h2 8751 116792 127791 138167 8750.16 2096997 0 ttl_and_stale_ttl
eviction_pressure loopback_h2 3621 292625 324041 352042 3619.91 65532 0 64KiB_cap
cost_shaped_writes loopback_h2 2803 361333 382250 397708 2802.23 4596053 4357403 cheap_large_expensive_small_mixed_ttl
```

The engine-only rows show the in-memory cache path is sub-microsecond for hot
get and put. The HTTP/2 rows include loopback transport, h2 framing, request
parsing, response construction, and body transfer.
