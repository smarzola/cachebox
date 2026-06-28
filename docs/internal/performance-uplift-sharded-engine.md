# Performance Uplift Sharded Engine

Milestone 4 replaces the server's single global engine mutex with fixed
sharded engine ownership.

## Change

- `ShardedEngine` owns 16 independent `Engine` shards.
- Ordinary key operations select a shard by hashing namespace and key.
- Batch get performs per-key shard selection.
- Tag invalidation visits every shard because tag indexes are shard-local.
- The expiration worker keeps one global per-tick cleanup budget and spends it
  across shards.
- `/metrics` remains observational and aggregates shard stats, memory, and cost.

Memory pressure is enforced per shard by dividing the configured memory limit
across shards. This avoids a coordinated global memory lock on the hot path, but
it means one overloaded shard can evict or reject writes while other shards have
free space.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `concurrent_get_16` | `loopback_tcp` | 111500 | 108750 | -2.5% |
| `concurrent_get_16_distinct` | `loopback_tcp` | n/a | 109375 | new |
| `concurrent_put_16` | `loopback_tcp` | 111500 | 114625 | +2.8% |
| `concurrent_get_16` | `loopback_unix` | 81625 | 81166 | -0.6% |
| `concurrent_get_16_distinct` | `loopback_unix` | n/a | 76459 | new |
| `concurrent_put_16` | `loopback_unix` | 83000 | 80167 | -3.4% |
| `tag_invalidate_8` | `loopback_unix` | 29750 | 37250 | +25.2% |

The single hot-key concurrent get benchmark still targets one shard, so it is
not expected to scale from sharding. The distinct-key benchmark shows the
distributed-key read shape explicitly. Unix concurrent puts improved, while TCP
put latency was roughly flat. Tag invalidation got slower because it now scans
all shard-local tag indexes.

## Interpretation

This removes avoidable global-lock contention from the server's key-addressed
operations without changing the transport protocol or cache semantics. The
remaining concurrent latency is now dominated by per-request task scheduling,
socket work, response encoding, and atomic metrics more than by one global
engine mutex.

The next work should measure metrics overhead and consider per-shard or
per-worker counters. Tag invalidation also needs a better global tag directory
or a sharded tag routing strategy if it must reach the original sub-20 us
target.
