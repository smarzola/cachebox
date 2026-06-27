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

## Scenarios

- `single_key_get`: cached GET hit.
- `single_key_put`: unique-key PUT writes.
- `batch_get_32`: batch get for 32 keys.
- `lease_contention`: repeated lease attempts for the same missing key.
- `tag_invalidation_8`: put eight tagged values, then invalidate the tag.
- `ttl_heavy_writes`: writes with TTL and stale TTL headers.
- `eviction_pressure`: writes against a 64 KiB memory cap.

## Baseline

Captured locally with:

```sh
cargo run --bin cachebox-bench
```

```text
scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes notes
single_key_get loopback_http1 10416 96083 107417 142792 10415.40 113 cached_hit
single_key_put loopback_http1 2059 451709 830583 873833 2058.76 245829 unique_keys
batch_get_32 loopback_http1 4955 199500 214459 250791 4954.93 249467 32_keys
lease_contention loopback_http1 9187 107708 115125 140250 9186.54 249467 same_missing_key
tag_invalidation_8 loopback_http1 138 7176625 7281167 7338666 137.10 249467 put_then_invalidate
ttl_heavy_writes loopback_http1 937 1053916 1195000 1232375 936.57 367275 ttl_and_stale_ttl
eviction_pressure loopback_http1 3231 310208 363209 398334 3230.88 65532 64KiB_cap
```
