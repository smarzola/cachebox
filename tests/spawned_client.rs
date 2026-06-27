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
use cachebox::protocol::{BatchItem, Metadata, ResponsePayload};

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
#[ignore = "spawns the cachebox binary and binds a localhost TCP port"]
fn spawned_binary_supports_cache_client_workflow() {
    let server = spawn_server();

    assert_eq!(
        request("GET", &server.addr, "/healthz", &[], &[]).status,
        200
    );

    let value = vec![0, 255, b'v', b'a', b'l'];
    let put = request(
        "PUT",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[
            ("Cachebox-TTL", "60s"),
            ("Cachebox-Tags", "group,blob"),
            ("Content-Type", "application/octet-stream"),
        ],
        &value,
    );
    assert_eq!(put.status, 201);

    let get = request(
        "GET",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[],
        &[],
    );
    assert_eq!(get.status, 200);
    assert_eq!(get.body, value);

    let batch = request(
        "POST",
        &server.addr,
        "/v1/namespaces/default/batch/get",
        &[],
        br#"{"keys":["blob","missing"]}"#,
    );
    assert_eq!(batch.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&batch.body).expect("batch json");
    assert_eq!(body["results"][0]["status"], "hit");
    assert_eq!(body["results"][1]["status"], "miss");

    let lease = request(
        "POST",
        &server.addr,
        "/v1/namespaces/default/leases/leased",
        &[],
        br#"{"lease_ttl_ms":10000}"#,
    );
    assert_eq!(lease.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&lease.body).expect("lease json");
    assert_eq!(body["state"], "lease_granted");
    let token = body["lease_token"].as_str().expect("lease token");

    let complete = request(
        "PUT",
        &server.addr,
        "/v1/namespaces/default/leases/leased/complete",
        &[("Cachebox-Lease-Token", token), ("Cachebox-TTL", "60s")],
        b"leased-value",
    );
    assert_eq!(complete.status, 201);

    let leased_get = request(
        "GET",
        &server.addr,
        "/v1/namespaces/default/keys/leased",
        &[],
        &[],
    );
    assert_eq!(leased_get.status, 200);
    assert_eq!(leased_get.body, b"leased-value");

    let invalidate = request(
        "POST",
        &server.addr,
        "/v1/namespaces/default/tags/group/invalidate",
        &[],
        &[],
    );
    assert_eq!(invalidate.status, 200);
    let body: serde_json::Value =
        serde_json::from_slice(&invalidate.body).expect("invalidate json");
    assert_eq!(body["removed"], 1);

    let deleted_get = request(
        "GET",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[],
        &[],
    );
    assert_eq!(deleted_get.status, 404);

    let delete = request(
        "DELETE",
        &server.addr,
        "/v1/namespaces/default/keys/leased",
        &[],
        &[],
    );
    assert_eq!(delete.status, 204);

    let unsupported = request("GET", &server.addr, "/unsupported", &[], &[]);
    assert_eq!(unsupported.status, 400);
    let body: serde_json::Value =
        serde_json::from_slice(&unsupported.body).expect("unsupported json");
    assert_eq!(body["code"], "invalid_path");
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
}

#[test]
#[ignore = "spawns the cachebox binary and binds a localhost TCP port"]
fn spawned_binary_grants_one_lease_under_client_contention() {
    const CLIENTS: usize = 32;

    let server = spawn_server();
    let addr = Arc::new(server.addr.clone());
    let barrier = Arc::new(Barrier::new(CLIENTS));
    let mut handles = Vec::new();

    for _ in 0..CLIENTS {
        let addr = Arc::clone(&addr);
        let barrier = Arc::clone(&barrier);
        handles.push(thread::spawn(move || {
            barrier.wait();
            let response = request(
                "POST",
                &addr,
                "/v1/namespaces/default/leases/hot-key",
                &[],
                br#"{"lease_ttl_ms":60000}"#,
            );
            assert_eq!(response.status, 200);
            let body: serde_json::Value =
                serde_json::from_slice(&response.body).expect("lease json");
            body["state"].as_str().expect("state").to_string()
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

#[test]
#[ignore = "spawns the cachebox binary, requires curl with HTTP/2 support, and binds localhost"]
fn spawned_binary_supports_http2_prior_knowledge_with_curl() {
    let server = spawn_server();

    let health = curl_h2("GET", &server.addr, "/healthz", &[], &[]);
    assert_eq!(health.status, 200);
    assert_eq!(health.body, b"ok");

    let put = curl_h2(
        "PUT",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[("Cachebox-Tags", "group")],
        b"value",
    );
    assert_eq!(put.status, 201);

    let get = curl_h2(
        "GET",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[],
        &[],
    );
    assert_eq!(get.status, 200);
    assert_eq!(get.body, b"value");

    let batch = curl_h2(
        "POST",
        &server.addr,
        "/v1/namespaces/default/batch/get",
        &[],
        br#"{"keys":["blob","missing"]}"#,
    );
    assert_eq!(batch.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&batch.body).expect("batch json");
    assert_eq!(body["results"][0]["status"], "hit");
    assert_eq!(body["results"][1]["status"], "miss");

    let lease = curl_h2(
        "POST",
        &server.addr,
        "/v1/namespaces/default/leases/leased",
        &[],
        br#"{"lease_ttl_ms":10000}"#,
    );
    assert_eq!(lease.status, 200);
    let body: serde_json::Value = serde_json::from_slice(&lease.body).expect("lease json");
    assert_eq!(body["state"], "lease_granted");
    let token = body["lease_token"].as_str().expect("lease token");

    let complete = curl_h2(
        "PUT",
        &server.addr,
        "/v1/namespaces/default/leases/leased/complete",
        &[("Cachebox-Lease-Token", token)],
        b"fresh",
    );
    assert_eq!(complete.status, 201);

    let delete = curl_h2(
        "DELETE",
        &server.addr,
        "/v1/namespaces/default/keys/leased",
        &[],
        &[],
    );
    assert_eq!(delete.status, 204);

    let invalidate = curl_h2(
        "POST",
        &server.addr,
        "/v1/namespaces/default/tags/group/invalidate",
        &[],
        &[],
    );
    assert_eq!(invalidate.status, 200);
    let body: serde_json::Value =
        serde_json::from_slice(&invalidate.body).expect("invalidate json");
    assert_eq!(body["removed"], 1);

    let miss = curl_h2(
        "GET",
        &server.addr,
        "/v1/namespaces/default/keys/blob",
        &[],
        &[],
    );
    assert_eq!(miss.status, 404);
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

fn request(
    method: &str,
    addr: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    request_fallible(method, addr, path, headers, body).expect("request should succeed")
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

fn curl_h2(
    method: &str,
    addr: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Response {
    let mut command = Command::new("curl");
    command
        .arg("--http2-prior-knowledge")
        .arg("-sS")
        .arg("-X")
        .arg(method)
        .arg("-w")
        .arg("\n%{http_version} %{http_code}");

    for (name, value) in headers {
        command.arg("-H").arg(format!("{name}: {value}"));
    }
    if !body.is_empty() {
        command
            .arg("--data-binary")
            .arg(String::from_utf8_lossy(body).to_string());
    }
    command.arg(format!("http://{addr}{path}"));

    let output = command.output().expect("curl should run");
    assert!(
        output.status.success(),
        "curl stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );

    let split = output
        .stdout
        .iter()
        .rposition(|byte| *byte == b'\n')
        .expect("curl write-out separator");
    let trailer = std::str::from_utf8(&output.stdout[split + 1..]).expect("curl write-out utf-8");
    let mut parts = trailer.split_whitespace();
    let version = parts.next().expect("http version");
    let status = parts
        .next()
        .and_then(|status| status.parse::<u16>().ok())
        .expect("status");
    assert_eq!(version, "2");

    Response {
        status,
        body: output.stdout[..split].to_vec(),
    }
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
    Response {
        status,
        body: bytes[split + separator.len()..].to_vec(),
    }
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: Vec<u8>,
}
