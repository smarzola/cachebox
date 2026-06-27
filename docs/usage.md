# Usage Guide

Cachebox stores raw-byte values under byte keys scoped by an ASCII namespace.
Cache operations use the native socket protocol over TCP or Unix sockets.

All examples use:

```rust
use cachebox::api::Ttl;
use cachebox::client::NativeClient;
use cachebox::protocol::{BatchItem, Metadata, ResponsePayload};
```

Connect over the default native TCP listener:

```rust
let mut client = NativeClient::connect_tcp("127.0.0.1:7401").await?;
```

Connect over a Unix socket when the server is started with `--native-unix`:

```rust
let mut client = NativeClient::connect_unix("/tmp/cachebox.sock").await?;
```

## Namespaces And Keys

Namespaces may contain ASCII letters, numbers, `-`, and `_`.

Keys are bytes. The native protocol does not require percent encoding:

```rust
client
    .put(
        "default",
        b"user:123/profile".to_vec(),
        Metadata::default(),
        b"profile bytes".to_vec(),
    )
    .await?;
```

## Put

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

`put` stores the value exactly as raw bytes. It returns the number of entries
evicted to make room for the write.

## Get

```rust
let response = client.get("default", b"dashboard:42".to_vec()).await?;

match response {
    ResponsePayload::Hit(bytes) => {
        assert_eq!(bytes, b"dashboard bytes".to_vec());
    }
    ResponsePayload::Stale(bytes) => {
        let _ = bytes;
    }
    ResponsePayload::Miss => {}
    other => panic!("unexpected get response: {other:?}"),
}
```

Get outcomes:

- `Hit(bytes)` for a fresh value.
- `Stale(bytes)` for a value inside its stale window.
- `Miss` when no readable value exists.

## Delete

```rust
let removed = client.delete("default", b"dashboard:42".to_vec()).await?;
```

Delete is idempotent. `removed` is `true` only when a stored entry existed.

## TTL

Set the fresh lifetime with `Metadata::ttl`:

```rust
client
    .put(
        "default",
        b"session:abc".to_vec(),
        Metadata {
            ttl: Some(Ttl {
                milliseconds: 30_000,
            }),
            ..Metadata::default()
        },
        b"session bytes".to_vec(),
    )
    .await?;
```

## Stale TTL

Set `stale_ttl` with `ttl` to keep serving an expired fresh value during a stale
window:

```rust
client
    .put(
        "default",
        b"report:weekly".to_vec(),
        Metadata {
            ttl: Some(Ttl {
                milliseconds: 60_000,
            }),
            stale_ttl: Some(Ttl {
                milliseconds: 300_000,
            }),
            ..Metadata::default()
        },
        b"rendered report".to_vec(),
    )
    .await?;
```

After the fresh TTL expires, reads can return `ResponsePayload::Stale(bytes)`.
After the stale TTL expires, the key is treated as a miss.

## Batch Get

Batch get reads multiple byte keys in one request:

```rust
let items = client
    .batch_get(
        "default",
        vec![
            b"a".to_vec(),
            b"user:123".to_vec(),
            b"bin\0\xff".to_vec(),
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

Each item reports hit, stale, or miss state.

## Tags

Attach tags on write:

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

Invalidate all keys in a namespace with a tag:

```rust
let removed = client.invalidate_tag("default", "user:123").await?;
```

The return value is the number of removed entries.

## Cost Metadata

`Metadata::cost` stores a user-provided unsigned integer score. It is useful for
measuring recomputation cost before enabling future cost-aware policy
experiments:

```rust
client
    .put(
        "default",
        b"llm:answer".to_vec(),
        Metadata {
            cost: Some(1200),
            ttl: Some(Ttl {
                milliseconds: 600_000,
            }),
            ..Metadata::default()
        },
        b"model output".to_vec(),
    )
    .await?;
```

The currently accounted aggregate is exposed by `/metrics`. Metrics reads are
observational and do not reclaim expired entries:

```text
cachebox_cost_score_total 1200
```

Cost metadata is observational. It does not change the current approximate LRU
eviction policy.

## Leases

Leases coordinate expensive recomputation so one client refreshes a missing or
stale key while other clients avoid duplicating the same work.

Start a lease:

```rust
let response = client
    .start_lease("default", b"prompt:abc".to_vec(), 10_000, None)
    .await?;
```

Possible states:

```rust
match response {
    ResponsePayload::Hit(bytes) => {
        let _ = bytes;
    }
    ResponsePayload::Stale(bytes) => {
        let _ = bytes;
    }
    ResponsePayload::LeaseGranted {
        lease_token,
        stale_value,
    } => {
        let generated = b"fresh generated bytes".to_vec();
        let _ = stale_value;
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
        // Another client owns the active lease.
    }
    other => panic!("unexpected lease response: {other:?}"),
}
```

Lease state is in-memory and process-local.

## Memory Limits And Eviction

Start Cachebox with memory and value limits:

```sh
cargo run --bin cachebox -- \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --max-body-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

Behavior:

- Native frames with payloads over `--max-body-bytes` are rejected.
- Single values over `--max-value-bytes` are rejected.
- `--cleanup-interval-ms` controls the background expiration interval. Use `0`
  to disable the background cleanup worker.
- `--cleanup-max-entries-per-tick` limits how many expired entries the
  background worker can reclaim in one tick.
- Writes evict bounded-sample approximate least-recently-used entries when
  memory is tight.
- Expired entries are tracked in an expiry index and reclaimed by cache access
  paths, the bounded background worker, or before live entries are evicted.

## Health And Metrics

Admin HTTP is intentionally separate from the native cache data plane.

Health:

```sh
curl 'http://127.0.0.1:7400/healthz'
```

Metrics:

```sh
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

## Errors

The native client returns `ClientError::Server` for structured server errors:

```rust
match client.get("bad namespace!", b"k".to_vec()).await {
    Err(cachebox::client::ClientError::Server { code, message }) => {
        let _ = (code, message);
    }
    other => panic!("unexpected result: {other:?}"),
}
```

Protocol error codes are defined in `cachebox::protocol::ErrorCode`.
