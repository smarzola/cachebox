# Cachebox Benchmarks

Cachebox benchmark results are local baselines only. Do not treat them as
portable performance claims without rerunning the command on the target machine.

## Command

```sh
cargo run --bin cachebox-bench
```

The harness starts loopback HTTP servers on random local ports, performs warmup
requests, then measures each scenario for a fixed duration. It reports:

- `p50_ns`, `p95_ns`, and `p99_ns`: sampled request latency percentiles.
- `throughput_ops_s`: completed benchmark operations per second.
- `memory_used_bytes`: `cachebox_memory_used_bytes` from `/metrics` after the
  scenario.
- `cost_score_total`: `cachebox_cost_score_total` from `/metrics` after the
  scenario.

## Scenarios

- `single_key_get`: cached GET hit.
- `single_key_put`: unique-key PUT writes.
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
single_key_get loopback_http1 10338 95750 105084 127125 10337.67 113 0 cached_hit
single_key_put loopback_http1 2064 452625 821583 859667 2062.72 246399 0 unique_keys
batch_get_32 loopback_http1 5004 197667 211458 239250 5003.13 250037 0 32_keys
lease_contention loopback_http1 9266 106958 114541 129500 9265.86 250037 0 same_missing_key
tag_invalidation_8 loopback_http1 138 7174583 7268041 7325041 137.22 250037 0 put_then_invalidate
ttl_heavy_writes loopback_http1 936 1055084 1196125 1243792 935.12 367731 0 ttl_and_stale_ttl
eviction_pressure loopback_http1 3247 308167 364834 400417 3246.30 65532 0 64KiB_cap
cost_shaped_writes loopback_http1 233 4250125 4557625 4660333 232.66 653448 499833 cheap_large_expensive_small_mixed_ttl
```
