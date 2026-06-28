# Native Sockets

Cachebox can expose the binary native data plane over TCP and, on Unix
platforms, Unix domain sockets. Cache operations use this native protocol:
clients keep a connection open, send length-prefixed request frames, and read
length-prefixed response frames. Admin HTTP exposes health and metrics.

Use TCP when you need a normal loopback or network socket. Use a Unix socket
when the client runs on the same host and you want lower local transport
overhead plus filesystem permissions around the socket path.

## Start TCP

```sh
cargo run --bin cachebox -- \
  --native-bind 127.0.0.1:7401
```

`127.0.0.1:7401` is the default native TCP listener, so `cargo run --bin
cachebox` is enough for local TCP use. Accepted native TCP connections use
`TCP_NODELAY`.

## Start Unix Socket

```sh
cargo run --bin cachebox -- \
  --native-unix /tmp/cachebox.sock
```

If `/tmp/cachebox.sock` is a stale socket file, Cachebox removes it before
binding. If another process is actively listening on that socket, startup fails.
Non-socket files are never removed.

## Rust Client Example

This example uses the native client API over a Unix socket. Use
`NativeClient::connect_tcp("127.0.0.1:7401")` for TCP.

```rust
use cachebox::protocol::{BatchItem, Metadata, ResponsePayload, Ttl};
use cachebox_client::NativeClient;

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

    let pipelined = client
        .request_pipelined(vec![
            (
                cachebox::protocol::Command::Get,
                cachebox::protocol::RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"user:123".to_vec(),
                },
            ),
            (
                cachebox::protocol::Command::Get,
                cachebox::protocol::RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"missing".to_vec(),
                },
            ),
        ])
        .await?;
    assert_eq!(
        pipelined,
        vec![
            ResponsePayload::Hit(b"cached bytes".to_vec()),
            ResponsePayload::Miss,
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

## Connection Behavior

Native connections are persistent. A client can send one request and wait for
one response, or it can pipeline several requests before reading responses.

Each request carries a `request_id`. The server echoes that id in the matching
response. When a connection is pipelined, responses can arrive in a different
order than requests because independent work may execute concurrently. The Rust
client handles that by matching `request_id` values and returning pipelined
payloads in submission order.

The server closes a native connection if the buffered frame payload exceeds
`--max-body-bytes`. Valid protocol errors, such as an invalid namespace or
unknown command, are returned as structured error response frames when the
server can decode enough of the frame to reply.

See [protocol.md](protocol.md) for frame layout, command ids, payload fields,
response status ids, error codes, and ordering rules.
