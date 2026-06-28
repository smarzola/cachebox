# Performance Uplift Metrics

Milestone 5 measures and reduces hot-counter contention by striping metrics
counters.

## Change

- `Metrics` now owns 64 independent counter shards.
- Native requests choose a metrics shard from connection id and request id.
- Admin HTTP and direct unit-test execution use a stable admin shard.
- `/metrics` sums all counter shards at scrape time.
- Metric names and scrape output stay unchanged.
- Scrapes remain observational: they do not increment request counters or force
  cache cleanup.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `pipelined_get_32` | `loopback_tcp` | 7608 | 7709 | +1.3% |
| `concurrent_get_16` | `loopback_tcp` | 111167 | 111833 | +0.6% |
| `concurrent_get_16_distinct` | `loopback_tcp` | 110583 | 109875 | -0.6% |
| `concurrent_put_16` | `loopback_tcp` | 112625 | 114042 | +1.3% |
| `pipelined_get_32` | `loopback_unix` | 7138 | 7144 | +0.1% |
| `concurrent_get_16` | `loopback_unix` | 81041 | 81750 | +0.9% |
| `concurrent_get_16_distinct` | `loopback_unix` | 76000 | 77083 | +1.4% |
| `concurrent_put_16` | `loopback_unix` | 79250 | 80709 | +1.8% |

The results are effectively flat. The striped counters reduce the chance that
metric atomics become a scalability ceiling, but current local benchmark
latency is not dominated by metrics contention.

## Interpretation

Metric aggregation is now structurally safer under concurrency without changing
the public metrics surface. The measured blocker remains transport scheduling,
task handoff, socket reads/writes, and response writes rather than global metric
atomics.

The next milestone should focus on syscall and write-path reduction. If future
profiles show scrape aggregation cost, the next step is caching a scrape
snapshot or aggregating per worker, but that is not justified by these numbers.
