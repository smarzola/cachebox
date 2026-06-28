# Quickstart

This guide gets one Cachebox server running locally and stores a raw byte value
through the native Rust client.

## Build And Run

Build the binary:

```sh
cargo build
```

Start Cachebox with default listeners:

```sh
cargo run --bin cachebox
```

The default process opens two local surfaces:

```text
admin HTTP: 127.0.0.1:7400
native TCP: 127.0.0.1:7401
```

Use the native TCP listener for cache operations. Use the admin HTTP listener
for health and metrics only.

## First Cache Entry

This example stores `cached bytes` under a byte key. It gives the entry a fresh
TTL, a stale window, two tags, and an optional cost score.

```rust
use cachebox::protocol::{Metadata, Ttl};
use cachebox_client::{GetResult, NativeClient};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = NativeClient::connect_tcp("127.0.0.1:7401").await?;

    client
        .put(
            "default",
            b"user:123/profile".to_vec(),
            Metadata {
                ttl: Some(Ttl {
                    milliseconds: 300_000,
                }),
                stale_ttl: Some(Ttl {
                    milliseconds: 60_000,
                }),
                tags: vec!["user:123".to_string(), "org:9".to_string()],
                cost: Some(42),
                ..Metadata::default()
            },
            b"cached bytes".to_vec(),
        )
        .await?;

    let value = client.get("default", b"user:123/profile".to_vec()).await?;
    assert_eq!(value, GetResult::Hit(b"cached bytes".to_vec()));

    let removed = client.invalidate_tag("default", "user:123").await?;
    assert_eq!(removed, 1);

    let value = client.get("default", b"user:123/profile".to_vec()).await?;
    assert_eq!(value, GetResult::Miss);

    Ok(())
}
```

The important pieces:

- Namespace `default` keeps this key separate from other tenants or feature
  areas.
- The key is bytes, so it can contain any application-owned encoding.
- TTL controls the fresh lifetime.
- Stale TTL lets clients serve stale bytes while another client refreshes.
- Tags let you invalidate a group without knowing every key.
- Cost is recorded for metrics and future policy decisions; it does not change
  eviction behavior today.

## Unix Socket Option

For same-host clients, a Unix domain socket avoids TCP loopback setup:

```sh
cargo run --bin cachebox -- --native-unix /tmp/cachebox.sock
```

```rust
let mut client = NativeClient::connect_unix("/tmp/cachebox.sock").await?;
```

## Memory And Cleanup Defaults

The default configuration is intentionally bounded:

```text
max frame payload: 8 MiB
max value size:    8 MiB
max cache memory:  64 MiB
cleanup interval: 250 ms
cleanup budget:   128 expired entries per tick
```

Override limits when starting the server:

```sh
cargo run --bin cachebox -- \
  --max-body-bytes 8388608 \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

Use `--cleanup-interval-ms 0` to disable the background expiration worker.
Metrics remain observational; reading metrics does not reclaim expired entries.

## Health, Metrics, And Benchmarks

Health:

```sh
curl 'http://127.0.0.1:7400/healthz'
```

Metrics:

```sh
curl 'http://127.0.0.1:7400/metrics'
```

Local benchmark harness:

```sh
cargo run --bin cachebox-bench
```

The benchmark output is for comparing changes on the same machine. See
[benchmarks.md](benchmarks.md) for the current table and scenario descriptions.
