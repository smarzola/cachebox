use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
#[cfg(unix)]
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use cachebox::api::{ContentType, Ttl};
use cachebox::client::{ClientError, NativeClient};
use cachebox::protocol::{
    BatchItem, Command as NativeCommand, Metadata, RequestPayload, ResponsePayload,
};

struct ServerProcess {
    child: Child,
    addr: String,
    native_addr: String,
    #[cfg(unix)]
    native_unix_socket: PathBuf,
}

impl Drop for ServerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        #[cfg(unix)]
        let _ = std::fs::remove_file(&self.native_unix_socket);
    }
}

#[test]
#[ignore = "spawns the cachebox binary and binds localhost native TCP"]
fn spawned_binary_supports_native_tcp_client_workflow() {
    let server = spawn_server();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let client = NativeClient::connect_tcp(&server.native_addr)
            .await
            .expect("native tcp connect");
        native_client_workflow(client).await;
    });
}

#[cfg(unix)]
#[test]
#[ignore = "spawns the cachebox binary and binds a native Unix socket"]
fn spawned_binary_supports_native_unix_client_workflow() {
    let server = spawn_server();
    let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
    runtime.block_on(async {
        let client = NativeClient::connect_unix(&server.native_unix_socket)
            .await
            .expect("native unix connect");
        native_client_workflow(client).await;
    });
}

async fn native_client_workflow(mut client: NativeClient) {
    let metadata = Metadata {
        ttl: Some(Ttl {
            milliseconds: 60_000,
        }),
        stale_ttl: Some(Ttl {
            milliseconds: 60_000,
        }),
        cost: Some(7),
        tags: vec!["group".to_string(), "blob".to_string()],
        content_type: ContentType::OctetStream,
    };
    let value = vec![0, 255, b'v', b'a', b'l'];
    assert_eq!(
        client
            .put("default", b"blob".to_vec(), metadata, value.clone())
            .await
            .expect("native put"),
        0
    );

    assert_eq!(
        client
            .get("default", b"blob".to_vec())
            .await
            .expect("native get"),
        ResponsePayload::Hit(value)
    );

    assert_eq!(
        client
            .batch_get("default", vec![b"blob".to_vec(), b"missing".to_vec()])
            .await
            .expect("native batch"),
        vec![
            BatchItem::Hit(vec![0, 255, b'v', b'a', b'l']),
            BatchItem::Miss
        ]
    );

    assert_eq!(
        client
            .request_pipelined(vec![
                (
                    NativeCommand::Get,
                    RequestPayload::Get {
                        namespace: "default".to_string(),
                        key: b"blob".to_vec(),
                    },
                ),
                (
                    NativeCommand::Get,
                    RequestPayload::Get {
                        namespace: "default".to_string(),
                        key: b"missing".to_vec(),
                    },
                ),
            ])
            .await
            .expect("native pipelined get"),
        vec![
            ResponsePayload::Hit(vec![0, 255, b'v', b'a', b'l']),
            ResponsePayload::Miss,
        ]
    );

    let lease = client
        .start_lease("default", b"leased".to_vec(), 10_000, None)
        .await
        .expect("native lease start");
    let token = match lease {
        ResponsePayload::LeaseGranted { lease_token, .. } => lease_token,
        other => panic!("expected lease grant, got {other:?}"),
    };
    assert_eq!(
        client
            .complete_lease(
                "default",
                b"leased".to_vec(),
                token,
                Metadata::default(),
                b"leased-value".to_vec(),
            )
            .await
            .expect("native lease complete"),
        0
    );
    assert_eq!(
        client
            .get("default", b"leased".to_vec())
            .await
            .expect("native leased get"),
        ResponsePayload::Hit(b"leased-value".to_vec())
    );

    assert_eq!(
        client
            .invalidate_tag("default", "group")
            .await
            .expect("native invalidate"),
        1
    );
    assert_eq!(
        client
            .get("default", b"blob".to_vec())
            .await
            .expect("native miss"),
        ResponsePayload::Miss
    );

    assert!(
        client
            .delete("default", b"leased".to_vec())
            .await
            .expect("native delete")
    );
    let error = client
        .put(
            "default",
            b"too-large".to_vec(),
            Metadata::default(),
            vec![b'x'; 2 * 1024 * 1024],
        )
        .await
        .expect_err("oversized native value should fail");
    assert!(matches!(
        error,
        ClientError::Server {
            code: cachebox::protocol::ErrorCode::ValueTooLarge,
            ..
        }
    ));

    let error = client
        .request_pipelined(vec![
            (
                NativeCommand::Get,
                RequestPayload::Get {
                    namespace: "bad namespace!".to_string(),
                    key: b"k".to_vec(),
                },
            ),
            (
                NativeCommand::Get,
                RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"missing".to_vec(),
                },
            ),
        ])
        .await
        .expect_err("pipelined server error should fail");
    assert!(matches!(
        error,
        ClientError::Server {
            code: cachebox::protocol::ErrorCode::InvalidNamespace,
            ..
        }
    ));
}

#[test]
#[ignore = "spawns the cachebox binary and binds localhost native TCP"]
fn spawned_binary_grants_one_lease_under_client_contention() {
    const CLIENTS: usize = 32;

    let server = spawn_server();
    let addr = Arc::new(server.native_addr.clone());
    let barrier = Arc::new(Barrier::new(CLIENTS));
    let mut handles = Vec::new();

    for _ in 0..CLIENTS {
        let addr = Arc::clone(&addr);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let runtime = tokio::runtime::Runtime::new().expect("tokio runtime");
            runtime.block_on(async move {
                let mut client = NativeClient::connect_tcp(addr.as_str())
                    .await
                    .expect("native tcp connect");
                match client
                    .start_lease("default", b"hot-key".to_vec(), 60_000, None)
                    .await
                    .expect("native lease start")
                {
                    ResponsePayload::LeaseGranted { .. } => "lease_granted".to_string(),
                    ResponsePayload::LeaseDenied => "lease_denied".to_string(),
                    other => panic!("unexpected lease response: {other:?}"),
                }
            })
        }));
    }

    let states: Vec<String> = handles
        .into_iter()
        .map(|handle| handle.join().expect("client thread should finish"))
        .collect();
    let grants = states
        .iter()
        .filter(|state| *state == "lease_granted")
        .count();
    let denials = states
        .iter()
        .filter(|state| *state == "lease_denied")
        .count();

    assert_eq!(grants, 1);
    assert_eq!(denials, CLIENTS - 1);
}

fn spawn_server() -> ServerProcess {
    let addr = unused_local_addr();
    let native_addr = unused_local_addr();
    #[cfg(unix)]
    let native_unix_socket = native_unix_socket_path();
    let mut command = Command::new(env!("CARGO_BIN_EXE_cachebox"));
    command
        .arg("--bind")
        .arg(&addr)
        .arg("--native-bind")
        .arg(&native_addr)
        .arg("--max-memory-bytes")
        .arg("1048576")
        .arg("--max-value-bytes")
        .arg("1048576")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(unix)]
    command.arg("--native-unix").arg(&native_unix_socket);
    let child = command.spawn().expect("cachebox binary should spawn");

    let server = ServerProcess {
        child,
        addr,
        native_addr,
        #[cfg(unix)]
        native_unix_socket,
    };
    wait_for_health(&server.addr);
    server
}

fn unused_local_addr() -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("local port should bind");
    listener.local_addr().expect("local addr").to_string()
}

#[cfg(unix)]
fn native_unix_socket_path() -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "cachebox-spawned-{}-{unique}.sock",
        std::process::id()
    ))
}

fn wait_for_health(addr: &str) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if request_fallible("GET", addr, "/healthz", &[], &[])
            .map(|response| response.status == 200)
            .unwrap_or(false)
        {
            return;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    panic!("server did not become healthy");
}

fn request_fallible(
    method: &str,
    addr: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> std::io::Result<Response> {
    let mut stream = TcpStream::connect(addr)?;
    stream.set_read_timeout(Some(Duration::from_secs(2)))?;
    stream.set_write_timeout(Some(Duration::from_secs(2)))?;

    write!(
        stream,
        "{method} {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\nContent-Length: {}\r\n",
        body.len()
    )?;
    for (name, value) in headers {
        write!(stream, "{name}: {value}\r\n")?;
    }
    stream.write_all(b"\r\n")?;
    stream.write_all(body)?;

    let mut bytes = Vec::new();
    stream.read_to_end(&mut bytes)?;
    Ok(parse_response(&bytes))
}

fn parse_response(bytes: &[u8]) -> Response {
    let separator = b"\r\n\r\n";
    let split = bytes
        .windows(separator.len())
        .position(|window| window == separator)
        .expect("response headers");
    let headers = std::str::from_utf8(&bytes[..split]).expect("headers utf-8");
    let status = headers
        .lines()
        .next()
        .and_then(|line| line.split_whitespace().nth(1))
        .and_then(|status| status.parse::<u16>().ok())
        .expect("status code");
    Response { status }
}

#[derive(Debug)]
struct Response {
    status: u16,
}
