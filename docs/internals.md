# Internals And Performance

This page explains the current Cachebox implementation in enough detail to
reason about behavior, resource use, and benchmark results. It is user-facing:
it describes how the shipped server works now.

## Process Surfaces

The cache data plane is the native socket protocol over TCP and, on Unix
platforms, Unix domain sockets. Admin HTTP exposes `/healthz` and `/metrics`.

The default local listeners are:

```text
admin HTTP:  127.0.0.1:7400
native TCP:  127.0.0.1:7401
native Unix: disabled unless --native-unix is set
```

## Engine Layout

The server owns a `ShardedEngine`. By default it has 16 shards. Each shard is a
`Mutex<Engine>`, so unrelated keys can be served by different shard locks while
the engine remains simple and allocation-conscious.

Shard selection hashes the namespace and key. Inside one shard, the engine has:

| Structure | Purpose |
| --- | --- |
| `entries: HashMap<EntryId, Entry>` | Stored byte values and metadata |
| `leases: HashMap<EntryId, Lease>` | Active refresh leases by key |
| `tag_index: HashMap<TagId, HashSet<EntryId>>` | Exact per-shard tag membership |
| `expiry_index: BTreeSet<ExpiryKey>` | Ordered expiry cleanup candidates |
| `memory_used_bytes` | Approximate bytes charged to the shard |
| `cost_score_total` | Sum of current entry cost hints |
| `next_access` | Monotonic access counter for approximate LRU |

The sharded wrapper also owns:

```text
tag_directory: Mutex<HashMap<TagId, HashSet<usize>>>
```

That directory maps each `(namespace, tag)` pair to shard indices that currently
contain matching entries.

An entry stores the value bytes, tags, optional TTL, optional stale TTL, optional
cost, charged memory size, and last-access counter. Cachebox accounts memory as
value bytes plus key/tag/metadata overhead. The accounting is intentionally
approximate; it is used to bound cache growth, not to reproduce allocator RSS.

## Expiration

Fresh TTL and stale TTL are separate. A value can be:

- `Hit`: fresh TTL has not elapsed.
- `Stale`: fresh TTL elapsed, but stale TTL has not elapsed.
- `Miss`: no entry exists, or both fresh and stale windows elapsed.

The `expiry_index` stores the time when an entry becomes removable, which is the
end of the stale window when stale TTL exists, otherwise the end of the fresh
TTL window.

Expiration is cleaned in three places:

- a background worker runs every `--cleanup-interval-ms` milliseconds;
- the worker removes at most `--cleanup-max-entries-per-tick` entries per tick;
- write paths can reclaim expired entries before evicting live entries.

Metrics and length/accounting reads are observational. They report current
accounting and do not perform cleanup work.

## Eviction And Approximate LRU

When a write would exceed the shard memory budget, Cachebox evicts live entries
until the incoming value fits. Eviction uses bounded-sample approximate LRU:
the shard inspects up to 16 entries and removes the one with the smallest
`last_access` counter.

This is deliberately cheaper than maintaining a global ordered LRU list. A true
global LRU needs mutation on every hit, cross-shard coordination, or a second
shared data structure. Cachebox instead records access inside the key's shard
and keeps eviction local to the shard doing the write. The result is predictable
memory bounding with a small hot-path cost.

## Tag Invalidation

Tags are designed for fast group invalidation without forcing every get or
untagged write through a global tag lock.

The per-shard `tag_index` is the source of truth for exact membership. The
global `tag_directory` is a routing table that answers: "which shards might
contain this tag?"

Writes with tags register routes in the directory after the shard write
succeeds. Deletes, replacements, lease completions, expiry cleanup, and
evictions remove per-shard tag membership. When a shard no longer contains a
tag, the route for that shard is removed.

Invalidation is then narrow:

1. Remove the `(namespace, tag)` route from the directory.
2. Lock only the routed shards.
3. In each routed shard, remove exactly the entries listed in its tag index.
4. Update memory, cost, expiry, lease, and tag accounting.

If a stale route exists because an entry expired or was evicted between route
maintenance points, invalidation may lock one extra shard and remove zero
entries there. That is safe: stale routes can add a little work, but they cannot
hide matching entries.

This is the main reason empty tag invalidation is cheap in the benchmarks.

## Leases

Leases coordinate refresh for expensive values. A lease is stored in the same
shard as its key. `LeaseStart` returns:

- `Hit` if fresh bytes are available;
- `Stale` if another client already owns a lease and stale bytes are readable;
- `LeaseGranted` if this client should recompute;
- `LeaseDenied` if another client owns the lease and no stale value is
  available.

`LeaseComplete` validates the token and stores the fresh value through the same
write path as `Put`, including tag registration, memory accounting, expiry
indexing, and eviction if needed. Lease state is in-memory and process-local.

## Native Connection Hot Path

The native protocol has a fixed 24-byte header plus a length-prefixed payload.
The server keeps a read buffer per connection and identifies complete frame
ranges inside that buffer.

For non-pipelined connections, execution is inline: the server decodes the
frame, executes the operation, encodes into a reusable response buffer, and
writes it directly. This avoids a Tokio task spawn and response channel for the
common one-request-at-a-time client flow.

When the server observes multiple complete frames in the connection buffer, it
switches that connection to pipelined mode:

- frames are copied into owned buffers so worker tasks can outlive the read
  buffer;
- at most 128 requests are in flight per connection;
- worker tasks execute independent requests concurrently;
- one writer task serializes responses back to the socket;
- the writer coalesces up to 32 response frames or 64 KiB into one write.

Pipelined responses can be written out of request order. The protocol makes
that safe by echoing `request_id`, and the official client reorders results for
the caller.

## Codec Optimizations

Hot request shapes avoid unnecessary ownership work. The server first tries a
borrowed decode path for `Get`, `Delete`, `TagInvalidate`, and `LeaseStart`.
For those commands, namespace, key, and tag fields can point into the connection
frame while the operation executes. Owned decode is still used for payloads that
must own values or collections, such as `Put`, `BatchGet`, and
`LeaseComplete`.

Cached get responses also use borrowed response encoding for hit and stale
values. The encoder writes the response frame from the engine-held value slice
into the response buffer, avoiding an intermediate `Vec<u8>` response payload.

## Metrics Hot Path

Request metrics are striped. The server chooses a metrics shard from connection
id and request id, which reduces counter contention under concurrent clients.
Metrics reads aggregate the stripes for admin output.

The important design point is that metrics are side-effect-free. Scraping
`/metrics` does not force expiration cleanup, alter LRU metadata, or mutate the
cache.

## Performance Shape

The benchmark harness separates the engine, codec, scheduler, and socket costs.
On the current local baseline:

| Scenario | p50 |
| --- | ---: |
| Engine cached get | `417 ns` |
| Decode prebuilt GET frame | `375 ns` |
| Encode borrowed HIT response | `167 ns` |
| Engine get plus borrowed encode | `542 ns` |
| Sharded get, access update, borrowed encode | `833 ns` |
| Spawn empty Tokio task and join | `6958 ns` |
| Spawn task plus response channel | `9125 ns` |
| Native Unix cached get | `13834 ns` |
| Native TCP cached get | `25084 ns` |
| Native Unix pipelined 32 GETs | `4826 ns/request` |
| Official client Unix pipelined 32 GETs | `5679 ns/request` |
| Native Unix empty tag invalidation | `14292 ns` |
| Native Unix eight-key tag invalidation | `29958 ns` |

The table shows why the transport optimizations matter. The core cache and
codec paths are in the nanosecond to low-microsecond range. Single-request
loopback work is dominated by socket read/write, scheduler, and async
connection overhead. Pipelining reduces that cost by keeping the connection busy
and amortizing writes.

## Resource Profile

A local optimized build on this machine produced:

```text
target/release/cachebox: 2.1 MB
idle RSS with admin TCP, native TCP, and native Unix listeners: about 2.6 MiB
```

Default runtime limits are:

```text
max cache memory:     64 MiB
max value bytes:       8 MiB
max frame payload:     8 MiB
cleanup interval:    250 ms
cleanup tick budget: 128 expired entries
```

Those defaults make Cachebox usable in small services, sidecars, local tools,
and resource-constrained environments. Increase `--max-memory-bytes` when you
want a larger working set; keep `--max-value-bytes` and `--max-body-bytes`
close to the largest value you actually intend to cache.
