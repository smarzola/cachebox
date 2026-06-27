# Quickstart

## Build

```sh
cargo build
```

## Run

```sh
cargo run --bin cachebox
```

Default listeners:

```text
admin HTTP: 127.0.0.1:7400
native TCP: 127.0.0.1:7401
```

Useful startup options:

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --native-bind 127.0.0.1:7401 \
  --native-unix /tmp/cachebox.sock \
  --max-body-bytes 8388608 \
  --max-memory-bytes 67108864 \
  --max-value-bytes 8388608 \
  --cleanup-interval-ms 250 \
  --cleanup-max-entries-per-tick 128
```

Show all options:

```sh
cargo run --bin cachebox -- --help
```

## Store, Read, And Delete

Use the native Rust client over TCP:

```rust
use cachebox::api::Ttl;
use cachebox::client::NativeClient;
use cachebox::protocol::{Metadata, ResponsePayload};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = NativeClient::connect_tcp("127.0.0.1:7401").await?;

    client
        .put(
            "default",
            b"user:123".to_vec(),
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

    let value = client.get("default", b"user:123".to_vec()).await?;
    assert_eq!(value, ResponsePayload::Hit(b"cached bytes".to_vec()));

    assert!(client.delete("default", b"user:123".to_vec()).await?);

    Ok(())
}
```

Use a Unix socket instead:

```rust
let mut client = NativeClient::connect_unix("/tmp/cachebox.sock").await?;
```

## Check Health And Metrics

Admin HTTP is not the cache data plane. It exposes only health and metrics:

```sh
curl 'http://127.0.0.1:7400/healthz'
curl 'http://127.0.0.1:7400/metrics'
```

## Run Tests

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

## Run Benchmarks

```sh
cargo run --bin cachebox-bench
```
