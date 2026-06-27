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

## Rust Unix Socket Example

This example stores and reads raw bytes over the native Unix socket using the
same codec exposed by the `cachebox` crate.

```rust
use cachebox::protocol::{
    Command, HEADER_LEN, Metadata, RequestFrame, RequestPayload, ResponsePayload,
    decode_response_frame, encode_request_frame,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn roundtrip(
    stream: &mut UnixStream,
    request: RequestFrame,
) -> std::io::Result<ResponsePayload> {
    stream.write_all(&encode_request_frame(&request)).await?;

    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let payload_len =
        u32::from_be_bytes(header[16..20].try_into().expect("header payload length")) as usize;
    let mut frame = Vec::with_capacity(HEADER_LEN + payload_len);
    frame.extend_from_slice(&header);
    frame.resize(HEADER_LEN + payload_len, 0);
    stream.read_exact(&mut frame[HEADER_LEN..]).await?;

    Ok(decode_response_frame(&frame, usize::MAX)
        .expect("native response should decode")
        .payload)
}

#[tokio::main]
async fn main() -> std::io::Result<()> {
    let mut stream = UnixStream::connect("/tmp/cachebox.sock").await?;

    let stored = roundtrip(
        &mut stream,
        RequestFrame {
            request_id: 1,
            command: Command::Put,
            payload: RequestPayload::Put {
                namespace: "default".to_string(),
                key: b"user:123".to_vec(),
                metadata: Metadata::default(),
                value: b"cached bytes".to_vec(),
            },
        },
    )
    .await?;
    assert_eq!(stored, ResponsePayload::Stored { evicted: 0 });

    let hit = roundtrip(
        &mut stream,
        RequestFrame {
            request_id: 2,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"user:123".to_vec(),
            },
        },
    )
    .await?;
    assert_eq!(hit, ResponsePayload::Hit(b"cached bytes".to_vec()));

    Ok(())
}
```

See [the native protocol specification](internal/native-socket-protocol.md) for
frame layout, command IDs, payload fields, and error codes.
