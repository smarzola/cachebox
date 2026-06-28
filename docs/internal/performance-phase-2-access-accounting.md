# Performance Phase 2 Access Accounting

Milestone 3 measures approximate LRU access-update cost on hot cached gets.

## Change

- Added a measurement-only `get_ref_without_access_update` path on the engine.
- Added benchmark rows for sharded get plus borrowed response encoding:
  - `sharded_get_ref_encode`
  - `sharded_get_ref_no_access_encode`
- Left production native get behavior unchanged: hot gets still update
  approximate LRU access metadata.

The no-access path exists to isolate cost. It does not refresh LRU state and is
covered by an eviction-focused test.

## Measurement

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | p50 ns | Notes |
| --- | --- | ---: | --- |
| `engine_get_ref_encode` | `process` | 542 | Inner engine get plus borrowed response encoding. |
| `sharded_get_ref_encode` | `process` | 833 | Shard mutex, get, access update, and borrowed response encoding. |
| `sharded_get_ref_no_access_encode` | `process` | 875 | Same path without access update. |
| `single_key_get` | `loopback_unix` | 14458 | Full native Unix cached get. |
| `single_key_get` | `loopback_tcp` | 24834 | Full native TCP cached get. |

The access-update experiment did not show a win. In this run, removing the LRU
metadata update was slightly slower than the normal sharded get path. The
combined shard lock, engine get, access update, and borrowed encode path is
still under `1 us`, while native Unix and TCP cached gets are still an order of
magnitude higher.

## Decision

Do not change production eviction behavior in this milestone. Per-get access
updates are not the measured blocker yet, and weakening approximate LRU would
trade correctness expectations for no demonstrated latency gain.

The next performance work should focus on tag invalidation routing, because
Unix `tag_invalidate_8` remains around `32.8 us` and the engine still scans all
shards for tag invalidation.
