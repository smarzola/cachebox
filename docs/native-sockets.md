# Native Sockets

Cachebox can expose the binary native data plane over TCP and, on Unix
platforms, Unix domain sockets. The HTTP/2 API is still available during the
transport migration.

## Start TCP

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --native-bind 127.0.0.1:7401
```

## Start Unix Socket

```sh
cargo run --bin cachebox -- \
  --bind 127.0.0.1:7400 \
  --native-unix /tmp/cachebox.sock
```

If `/tmp/cachebox.sock` is a stale socket file, Cachebox removes it before
binding. If another process is actively listening on that socket, startup fails.
Non-socket files are never removed.

## Rust Client Example

This example uses the native client API over a Unix socket. Use
`NativeClient::connect_tcp("127.0.0.1:7401")` for TCP.

```rust
use cachebox::api::Ttl;
use cachebox::client::NativeClient;
use cachebox::protocol::{BatchItem, Metadata, ResponsePayload};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut client = NativeClient::connect_unix("/tmp/cachebox.sock").await?;

    client
        .put(
            "default",
            b"user:123".to_vec(),
            Metadata {
                ttl: Some(Ttl {
                    milliseconds: 60_000,
                }),
                stale_ttl: Some(Ttl {
                    milliseconds: 60_000,
                }),
                tags: vec!["user:123".to_string()],
                ..Metadata::default()
            },
            b"cached bytes".to_vec(),
        )
        .await?;

    let hit = client.get("default", b"user:123".to_vec()).await?;
    assert_eq!(hit, ResponsePayload::Hit(b"cached bytes".to_vec()));

    let batch = client
        .batch_get(
            "default",
            vec![b"user:123".to_vec(), b"missing".to_vec()],
        )
        .await?;
    assert_eq!(
        batch,
        vec![
            BatchItem::Hit(b"cached bytes".to_vec()),
            BatchItem::Miss,
        ]
    );

    let lease = client
        .start_lease("default", b"expensive".to_vec(), 10_000, None)
        .await?;
    let token = match lease {
        ResponsePayload::LeaseGranted { lease_token, .. } => lease_token,
        other => panic!("expected lease grant, got {other:?}"),
    };

    client
        .complete_lease(
            "default",
            b"expensive".to_vec(),
            token,
            Metadata::default(),
            b"fresh bytes".to_vec(),
        )
        .await?;

    let removed = client.invalidate_tag("default", "user:123").await?;
    assert_eq!(removed, 1);

    let deleted = client.delete("default", b"expensive".to_vec()).await?;
    assert!(deleted);

    Ok(())
}
```

The low-level codec remains available if a client wants direct frame control:

```rust
use cachebox::protocol::{
    Command, Metadata, RequestFrame, RequestPayload, encode_request_frame,
};

let bytes = encode_request_frame(&RequestFrame {
    request_id: 1,
    command: Command::Put,
    payload: RequestPayload::Put {
        namespace: "default".to_string(),
        key: b"user:123".to_vec(),
        metadata: Metadata::default(),
        value: b"cached bytes".to_vec(),
    },
});
```

See [the native protocol specification](internal/native-socket-protocol.md) for
frame layout, command IDs, payload fields, and error codes.
