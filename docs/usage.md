# Usage Guide

Cachebox stores raw bytes under byte keys, scoped by namespaces. The server
does not need to understand your application objects. Your application chooses
stable keys, writes bytes, and attaches the cache metadata that describes how
those bytes should age and be invalidated.

Use this guide to choose the right operation and understand the behavior you
get from each feature.

All Rust examples use:

```rust
use cachebox::client::NativeClient;
use cachebox::protocol::{BatchItem, Command, Metadata, RequestPayload, ResponsePayload, Ttl};
```

Connect over TCP:

```rust
let mut client = NativeClient::connect_tcp("127.0.0.1:7401").await?;
```

Connect over a Unix socket:

```rust
let mut client = NativeClient::connect_unix("/tmp/cachebox.sock").await?;
```

## Keys And Namespaces

Use namespaces to separate tenants, products, environments, or broad feature
areas. Namespaces are ASCII strings containing letters, numbers, `-`, and `_`.
Tags use the same readable style and also allow `:`, `.`, and `/`, which is
useful for names like `user:123`, `org.9`, or `prompt/template/v2`.

Keys are raw bytes. That means you can use a readable string key:

```rust
b"user:123/profile".to_vec()
```

or a binary digest:

```rust
vec![0x9f, 0x86, 0xd0, 0x81]
```

The native protocol carries bytes directly, so keys do not need percent
encoding.

## Put: Store Raw Bytes

Use `put` when your application has a value ready to cache:

```rust
let evicted = client
    .put(
        "default",
        b"dashboard:42".to_vec(),
        Metadata::default(),
        std::fs::read("dashboard.bin")?,
    )
    .await?;
```

The return value is the number of entries evicted to make room for the write.
For most normal writes it is `0`.

Use metadata to describe freshness and invalidation:

```rust
client
    .put(
        "default",
        b"user:123/dashboard".to_vec(),
        Metadata {
            ttl: Some(Ttl {
                milliseconds: 300_000,
            }),
            stale_ttl: Some(Ttl {
                milliseconds: 60_000,
            }),
            tags: vec!["user:123".to_string(), "dashboard".to_string()],
            cost: Some(1200),
            ..Metadata::default()
        },
        b"rendered dashboard".to_vec(),
    )
    .await?;
```

Choose metadata deliberately:

- Use `ttl` when a value is trustworthy for a fixed fresh lifetime.
- Use `stale_ttl` when serving old bytes is better than forcing every caller to
  recompute at once.
- Use `tags` when you will need group invalidation.
- Use `cost` to record the approximate recomputation cost for metrics and
  future policy decisions.

## Get: Read Fresh, Stale, Or Missing State

Use `get` when you want one key:

```rust
let response = client.get("default", b"dashboard:42".to_vec()).await?;

match response {
    ResponsePayload::Hit(bytes) => {
        // Fresh value.
        let _ = bytes;
    }
    ResponsePayload::Stale(bytes) => {
        // Still readable, but a refresh should be considered.
        let _ = bytes;
    }
    ResponsePayload::Miss => {
        // Nothing readable exists.
    }
    other => panic!("unexpected get response: {other:?}"),
}
```

Cachebox keeps stale as a first-class state because many real workloads prefer
bounded staleness over a thundering herd. A stale response is not a failure; it
is a signal that the caller can serve old bytes while arranging refresh.

## Delete: Remove One Key

Use `delete` for direct key removal:

```rust
let removed = client.delete("default", b"dashboard:42".to_vec()).await?;
```

Delete is idempotent. `removed` is `true` only if a stored entry existed.

## Batch Get: One Command, Many Keys

Use `batch_get` when you have many keys and want one logical request:

```rust
let items = client
    .batch_get(
        "default",
        vec![
            b"fragment:a".to_vec(),
            b"fragment:b".to_vec(),
            b"fragment:c".to_vec(),
        ],
    )
    .await?;

for item in items {
    match item {
        BatchItem::Hit(bytes) => {
            let _ = bytes;
        }
        BatchItem::Stale(bytes) => {
            let _ = bytes;
        }
        BatchItem::Miss => {}
    }
}
```

Batch get is best when every item is the same command shape. Response order
matches the key order you sent.

## Pipelined Requests: Many Commands, One Connection Round

Use `request_pipelined` when you have independent requests and want to send
them before waiting for individual responses. This keeps server semantics the
same, but lets the connection stay busy.

```rust
let responses = client
    .request_pipelined(vec![
        (
            Command::Get,
            RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"fragment:a".to_vec(),
            },
        ),
        (
            Command::Get,
            RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"fragment:b".to_vec(),
            },
        ),
        (
            Command::Delete,
            RequestPayload::Delete {
                namespace: "default".to_string(),
                key: b"old-fragment".to_vec(),
            },
        ),
    ])
    .await?;
```

The server may execute pipelined work concurrently and may respond out of
order. The Rust client matches by request id and returns payloads in the same
order as the submitted requests.

If any request returns a structured server error, the helper returns
`ClientError::Server` after reading the response batch, so the connection stays
aligned for later use.

## Tags: Invalidate Groups Without Knowing Every Key

Use tags when many keys share an invalidation reason:

```rust
client
    .put(
        "default",
        b"user:123/dashboard".to_vec(),
        Metadata {
            tags: vec![
                "user:123".to_string(),
                "workspace:abc".to_string(),
                "prompt-template:v2".to_string(),
            ],
            ..Metadata::default()
        },
        b"dashboard bytes".to_vec(),
    )
    .await?;
```

Invalidate one tag in one namespace:

```rust
let removed = client.invalidate_tag("default", "user:123").await?;
```

Internally, Cachebox maintains a tag directory that routes each
`(namespace, tag)` to the shards that currently contain matching entries. That
makes empty or narrow invalidations avoid scanning every shard. Ordinary gets
and untagged puts do not take the tag directory lock.

## Leases: Coordinate Expensive Refresh

Use leases when recomputing a miss or stale value is expensive. A lease lets one
client become the refresher while other clients avoid repeating the same work.

```rust
let response = client
    .start_lease("default", b"prompt:abc".to_vec(), 10_000, None)
    .await?;
```

Handle the possible states:

```rust
match response {
    ResponsePayload::Hit(bytes) => {
        // Fresh value already exists.
        let _ = bytes;
    }
    ResponsePayload::Stale(bytes) => {
        // Another client owns the refresh lease; serve stale bytes if acceptable.
        let _ = bytes;
    }
    ResponsePayload::LeaseGranted {
        lease_token,
        stale_value,
    } => {
        // This client should recompute.
        let _ = stale_value;
        let generated = b"fresh generated bytes".to_vec();

        client
            .complete_lease(
                "default",
                b"prompt:abc".to_vec(),
                lease_token,
                Metadata {
                    ttl: Some(Ttl {
                        milliseconds: 300_000,
                    }),
                    stale_ttl: Some(Ttl {
                        milliseconds: 1_800_000,
                    }),
                    ..Metadata::default()
                },
                generated,
            )
            .await?;
    }
    ResponsePayload::LeaseDenied => {
        // Another client owns an active lease and no stale value is available.
    }
    other => panic!("unexpected lease response: {other:?}"),
}
```

Lease state is in-memory and process-local. If the process exits, active leases
are gone with the cache.

## Memory Limits, Expiration, And Eviction

Cachebox is memory bounded. Start it with explicit limits:

```sh
cargo run --bin cachebox -- \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --max-body-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

The limits mean:

- `--max-body-bytes`: largest accepted native frame payload.
- `--max-value-bytes`: largest single cached value.
- `--max-memory-bytes`: approximate cache memory budget.
- `--cleanup-interval-ms`: background expiration worker interval.
- `--cleanup-max-entries-per-tick`: expiration work budget per tick.

Expiration and eviction are separate:

- Expiration removes entries whose TTL and stale TTL have both passed.
- Eviction removes live entries when a write needs memory.
- Eviction uses bounded-sample approximate LRU, so hot entries are likely to
  stay without requiring a global ordered LRU structure.
- Metrics and accounting accessors are observational; they do not trigger
  cleanup side effects.

## Metrics

Admin HTTP exposes health and metrics:

```sh
curl 'http://127.0.0.1:7400/healthz'
curl 'http://127.0.0.1:7400/metrics'
```

Important metrics:

```text
cachebox_requests_total
cachebox_cache_hits_total
cachebox_cache_misses_total
cachebox_cache_stale_total
cachebox_lease_grants_total
cachebox_lease_denials_total
cachebox_errors_total
cachebox_expirations_total
cachebox_evictions_total
cachebox_memory_used_bytes
cachebox_memory_limit_bytes
cachebox_cost_score_total
```

`cachebox_cost_score_total` is the sum of currently accounted entry cost
metadata. It is useful for observability and planning, but it does not change
eviction behavior in the current engine.

## Errors

The native client returns structured server errors as `ClientError::Server`:

```rust
match client.get("bad namespace!", b"k".to_vec()).await {
    Err(cachebox::client::ClientError::Server { code, message }) => {
        let _ = (code, message);
    }
    other => panic!("unexpected result: {other:?}"),
}
```

Branch on the error code rather than the diagnostic message. See
[protocol.md](protocol.md) for the full error-code table.
