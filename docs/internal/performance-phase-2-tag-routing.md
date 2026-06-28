# Performance Phase 2 Tag Routing

Milestone 4 avoids blindly scanning every shard-local tag index for ordinary
tag invalidation.

## Change

- `ShardedEngine` now keeps a tag directory mapping `(namespace, tag)` to the
  shard indices that currently contain entries for that tag.
- Tagged `put` and lease completion register the destination shard for each
  tag.
- Replacing a key with different tags removes stale routes when the shard no
  longer contains the old tag.
- Deleting a tagged entry removes stale routes when the shard no longer
  contains that tag.
- `invalidate_tag` removes the tag route and locks only the routed shards.
- Untagged `get`, untagged `put`, and ordinary cache reads do not take the tag
  directory lock.

The directory can still hold stale routes after expiration or eviction until a
future delete, replacement, or invalidation observes them. That is acceptable
for correctness: stale routes can cause an extra empty shard invalidation, but
they cannot hide a tagged entry.

## Before And After

Local benchmark command:

```sh
cargo run --bin cachebox-bench
```

Selected p50 results:

| Scenario | Transport | Before ns | After ns | Change |
| --- | --- | ---: | ---: | ---: |
| `tag_invalidate_empty` | `loopback_tcp` | 30292 | 24667 | -18.6% |
| `tag_invalidate_8` | `loopback_tcp` | 42333 | 39584 | -6.5% |
| `tag_workflow_put8_invalidate` | `loopback_tcp` | 275084 | 287167 | +4.4% |
| `tag_invalidate_empty` | `loopback_unix` | 21416 | 14625 | -31.7% |
| `tag_invalidate_8` | `loopback_unix` | 32833 | 29500 | -10.2% |
| `tag_workflow_put8_invalidate` | `loopback_unix` | 177250 | 188667 | +6.4% |

Empty invalidation improved substantially because it no longer scans all
shards. Populated eight-key invalidation improved, but not enough to reach the
`20 us` Unix target.

## Remaining Bottleneck

Unix `tag_invalidate_8` is still around `29.5 us`. The remaining cost is no
longer a blind all-shard scan; it is routed shard locking plus removing eight
entries from shard-local entry, tag, expiry, memory, and cost indexes.

The full put-eight-then-invalidate workflow regressed slightly because tagged
writes now maintain the routing directory. That tradeoff is acceptable for this
checkpoint because ordinary untagged get and put paths avoid the directory, and
empty/tag invalidation requests are materially cheaper.
