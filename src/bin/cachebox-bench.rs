use std::net::TcpListener as StdTcpListener;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cachebox::api::Ttl;
use cachebox::config::Config;
use cachebox::engine::{Engine, GetOutcome, PutCommand};
use cachebox::protocol::{
    BatchItem, Command, HEADER_LEN, Metadata, RequestFrame, RequestPayload, ResponsePayload,
    decode_response_frame, encode_request_frame,
};
use cachebox::server;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::runtime::Runtime;

const WARMUP_ITERS: usize = 100;
const MEASURE_DURATION: Duration = Duration::from_secs(1);

macro_rules! warmup_async {
    ($body:block) => {
        for _ in 0..WARMUP_ITERS {
            $body
        }
    };
}

macro_rules! measure_native_async {
    ($client:expr, $name:expr, $notes:expr, $body:block) => {{
        let mut samples = Vec::new();
        let started = Instant::now();
        while started.elapsed() < MEASURE_DURATION {
            let sample_started = Instant::now();
            $body
            samples.push(sample_started.elapsed());
        }
        summarize(
            $name,
            $client.transport,
            $notes,
            samples,
            started.elapsed(),
            0,
        )
    }};
}

fn main() {
    let runtime = Runtime::new().expect("benchmark tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    let native_server = NativeLoopbackServer::start(Config::default());
    let mut native_client = NativeClient::connect_tcp(&native_server.addr).await;

    let mut scenarios = vec![
        bench_engine_get(),
        bench_engine_put(),
        bench_engine_tag_invalidate_8(),
        bench_native_single_key_get(&mut native_client).await,
        bench_native_single_key_put(&mut native_client).await,
        bench_native_batch_get(&mut native_client).await,
        bench_native_lease_contention(&mut native_client).await,
        bench_native_tag_invalidate_empty(&mut native_client).await,
        bench_native_tag_invalidate_8(&mut native_client).await,
        bench_native_tag_invalidation(&mut native_client).await,
        bench_native_ttl_heavy_writes(&mut native_client).await,
    ];

    #[cfg(unix)]
    {
        let native_unix_server = NativeUnixLoopbackServer::start(Config::default());
        let mut native_unix_client = NativeClient::connect_unix(&native_unix_server.path).await;
        scenarios.extend([
            bench_native_single_key_get(&mut native_unix_client).await,
            bench_native_single_key_put(&mut native_unix_client).await,
            bench_native_batch_get(&mut native_unix_client).await,
            bench_native_lease_contention(&mut native_unix_client).await,
            bench_native_tag_invalidate_empty(&mut native_unix_client).await,
            bench_native_tag_invalidate_8(&mut native_unix_client).await,
            bench_native_tag_invalidation(&mut native_unix_client).await,
            bench_native_ttl_heavy_writes(&mut native_unix_client).await,
        ]);
        native_unix_server.cleanup();
    }

    println!(
        "scenario transport iterations p50_ns p95_ns p99_ns throughput_ops_s memory_used_bytes cost_score_total notes"
    );
    for scenario in scenarios {
        println!(
            "{} {} {} {} {} {} {:.2} {} {} {}",
            scenario.name,
            scenario.transport,
            scenario.iterations,
            scenario.p50_ns,
            scenario.p95_ns,
            scenario.p99_ns,
            scenario.throughput_ops_s,
            scenario.memory_used_bytes,
            scenario.cost_score_total,
            scenario.notes
        );
    }
}

fn bench_engine_get() -> BenchResult {
    let mut engine = Engine::new();
    engine
        .put(put_command("bench", b"get-key", b"value"))
        .expect("engine put should fit");
    warmup_sync(|| {
        assert_eq!(
            engine.get("bench", b"get-key"),
            GetOutcome::Hit(b"value".to_vec())
        );
    });
    let result = measure_sync("engine_get", "engine_cached_hit", || {
        assert_eq!(
            engine.get("bench", b"get-key"),
            GetOutcome::Hit(b"value".to_vec())
        );
    });
    with_memory(result, engine.memory_used_bytes())
}

fn bench_engine_put() -> BenchResult {
    let mut engine = Engine::new();
    let mut index = 0usize;
    warmup_sync(|| {
        let key = format!("put-warmup-{index}");
        index += 1;
        engine
            .put(put_command("bench", key.as_bytes(), b"value"))
            .expect("engine put should fit");
    });
    let result = measure_sync("engine_put", "engine_unique_keys", || {
        let key = format!("put-{index}");
        index += 1;
        engine
            .put(put_command("bench", key.as_bytes(), b"value"))
            .expect("engine put should fit");
    });
    with_memory(result, engine.memory_used_bytes())
}

fn bench_engine_tag_invalidate_8() -> BenchResult {
    let mut engine = Engine::new();
    let mut index = 0usize;
    warmup_sync(|| {
        let tag = engine_put_tagged_values(&mut engine, index);
        assert_eq!(engine.invalidate_tag("bench", &tag), 8);
        index += 1;
    });
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let tag = engine_put_tagged_values(&mut engine, index);
        index += 1;
        let sample_started = Instant::now();
        assert_eq!(engine.invalidate_tag("bench", &tag), 8);
        samples.push(sample_started.elapsed());
    }
    let measured_elapsed = total_duration(&samples);
    let result = summarize(
        "engine_tag_invalidate_8",
        "engine",
        "remove_8_tagged_keys",
        samples,
        measured_elapsed,
        0,
    );
    with_memory(result, engine.memory_used_bytes())
}

async fn bench_native_single_key_get(client: &mut NativeClient) -> BenchResult {
    assert_eq!(
        client
            .request(
                Command::Put,
                native_put_payload(b"get-key", Metadata::default(), b"value")
            )
            .await,
        ResponsePayload::Stored { evicted: 0 }
    );
    warmup_async!({
        assert_eq!(
            client
                .request(Command::Get, native_get_payload(b"get-key"))
                .await,
            ResponsePayload::Hit(b"value".to_vec())
        );
    });
    measure_native_async!(client, "single_key_get", "cached_hit", {
        assert_eq!(
            client
                .request(Command::Get, native_get_payload(b"get-key"))
                .await,
            ResponsePayload::Hit(b"value".to_vec())
        );
    })
}

async fn bench_native_single_key_put(client: &mut NativeClient) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let key = format!("native-put-warmup-{index}");
        index += 1;
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), Metadata::default(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    });
    measure_native_async!(client, "single_key_put", "unique_keys", {
        let key = format!("native-put-{index}");
        index += 1;
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), Metadata::default(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    })
}

async fn bench_native_batch_get(client: &mut NativeClient) -> BenchResult {
    for index in 0..32 {
        let key = format!("native-batch-{index}");
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), Metadata::default(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    }
    let keys: Vec<Vec<u8>> = (0..32)
        .map(|index| format!("native-batch-{index}").into_bytes())
        .collect();
    warmup_async!({
        assert_native_batch_hits(client, &keys).await;
    });
    measure_native_async!(client, "batch_get_32", "32_keys", {
        assert_native_batch_hits(client, &keys).await;
    })
}

async fn bench_native_lease_contention(client: &mut NativeClient) -> BenchResult {
    assert!(matches!(
        client
            .request(
                Command::LeaseStart,
                RequestPayload::LeaseStart {
                    namespace: "bench".to_string(),
                    key: b"native-contention".to_vec(),
                    lease_ttl_ms: 60_000,
                    allow_stale_ms: None,
                },
            )
            .await,
        ResponsePayload::LeaseGranted { .. }
    ));
    warmup_async!({
        assert_eq!(
            client
                .request(
                    Command::LeaseStart,
                    RequestPayload::LeaseStart {
                        namespace: "bench".to_string(),
                        key: b"native-contention".to_vec(),
                        lease_ttl_ms: 60_000,
                        allow_stale_ms: None,
                    },
                )
                .await,
            ResponsePayload::LeaseDenied
        );
    });
    measure_native_async!(client, "lease_contention", "same_missing_key", {
        assert_eq!(
            client
                .request(
                    Command::LeaseStart,
                    RequestPayload::LeaseStart {
                        namespace: "bench".to_string(),
                        key: b"native-contention".to_vec(),
                        lease_ttl_ms: 60_000,
                        allow_stale_ms: None,
                    },
                )
                .await,
            ResponsePayload::LeaseDenied
        );
    })
}

async fn bench_native_tag_invalidate_empty(client: &mut NativeClient) -> BenchResult {
    warmup_async!({
        assert_eq!(
            client
                .request(
                    Command::TagInvalidate,
                    native_tag_invalidate_payload("empty-tag")
                )
                .await,
            ResponsePayload::Invalidated { removed: 0 }
        );
    });
    measure_native_async!(client, "tag_invalidate_empty", "single_empty_invalidate", {
        assert_eq!(
            client
                .request(
                    Command::TagInvalidate,
                    native_tag_invalidate_payload("empty-tag")
                )
                .await,
            ResponsePayload::Invalidated { removed: 0 }
        );
    })
}

async fn bench_native_tag_invalidate_8(client: &mut NativeClient) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let tag = native_put_tagged_values(client, index).await;
        native_invalidate_tag(client, &tag, 8).await;
        index += 1;
    });
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let tag = native_put_tagged_values(client, index).await;
        index += 1;
        let sample_started = Instant::now();
        native_invalidate_tag(client, &tag, 8).await;
        samples.push(sample_started.elapsed());
    }
    let measured_elapsed = total_duration(&samples);
    summarize(
        "tag_invalidate_8",
        client.transport,
        "single_invalidate_8_tagged_keys",
        samples,
        measured_elapsed,
        0,
    )
}

async fn bench_native_tag_invalidation(client: &mut NativeClient) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        native_tag_invalidation_round(client, index).await;
        index += 1;
    });
    measure_native_async!(
        client,
        "tag_workflow_put8_invalidate",
        "8_puts_plus_invalidate",
        {
            native_tag_invalidation_round(client, index).await;
            index += 1;
        }
    )
}

async fn bench_native_ttl_heavy_writes(client: &mut NativeClient) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let key = format!("native-ttl-warmup-{index}");
        index += 1;
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), ttl_metadata(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    });
    measure_native_async!(client, "ttl_heavy_writes", "ttl_and_stale_ttl", {
        let key = format!("native-ttl-{index}");
        index += 1;
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), ttl_metadata(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    })
}

async fn assert_native_batch_hits(client: &mut NativeClient, keys: &[Vec<u8>]) {
    let response = client
        .request(
            Command::BatchGet,
            RequestPayload::BatchGet {
                namespace: "bench".to_string(),
                keys: keys.to_vec(),
            },
        )
        .await;
    assert_eq!(
        response,
        ResponsePayload::BatchGet {
            items: vec![BatchItem::Hit(b"value".to_vec()); keys.len()]
        }
    );
}

async fn native_tag_invalidation_round(client: &mut NativeClient, index: usize) {
    let tag = native_put_tagged_values(client, index).await;
    native_invalidate_tag(client, &tag, 8).await;
}

fn engine_put_tagged_values(engine: &mut Engine, index: usize) -> String {
    let tag = format!("tag-{index}");
    for item in 0..8 {
        let key = format!("tag-{index}-{item}");
        let mut command = put_command("bench", key.as_bytes(), b"value");
        command.tags = vec![tag.clone()];
        engine.put(command).expect("engine put should fit");
    }
    tag
}

async fn native_put_tagged_values(client: &mut NativeClient, index: usize) -> String {
    let tag = format!("native-tag-{index}");
    for item in 0..8 {
        let key = format!("native-tag-{index}-{item}");
        let metadata = Metadata {
            tags: vec![tag.clone()],
            ..Metadata::default()
        };
        assert_eq!(
            client
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), metadata, b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    }
    tag
}

async fn native_invalidate_tag(client: &mut NativeClient, tag: &str, expected_removed: u32) {
    assert_eq!(
        client
            .request(Command::TagInvalidate, native_tag_invalidate_payload(tag))
            .await,
        ResponsePayload::Invalidated {
            removed: expected_removed
        }
    );
}

fn native_get_payload(key: &[u8]) -> RequestPayload {
    RequestPayload::Get {
        namespace: "bench".to_string(),
        key: key.to_vec(),
    }
}

fn native_put_payload(key: &[u8], metadata: Metadata, value: &[u8]) -> RequestPayload {
    RequestPayload::Put {
        namespace: "bench".to_string(),
        key: key.to_vec(),
        metadata,
        value: value.to_vec(),
    }
}

fn native_tag_invalidate_payload(tag: &str) -> RequestPayload {
    RequestPayload::TagInvalidate {
        namespace: "bench".to_string(),
        tag: tag.to_string(),
    }
}

fn ttl_metadata() -> Metadata {
    Metadata {
        ttl: Some(Ttl {
            milliseconds: 60_000,
        }),
        stale_ttl: Some(Ttl {
            milliseconds: 60_000,
        }),
        ..Metadata::default()
    }
}

fn warmup_sync(mut operation: impl FnMut()) {
    for _ in 0..WARMUP_ITERS {
        operation();
    }
}

fn measure_sync(
    name: &'static str,
    notes: &'static str,
    mut operation: impl FnMut(),
) -> BenchResult {
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        operation();
        samples.push(sample_started.elapsed());
    }
    summarize(name, "engine", notes, samples, started.elapsed(), 0)
}

fn put_command(namespace: &str, key: &[u8], value: &[u8]) -> PutCommand {
    PutCommand {
        namespace: namespace.to_string(),
        key: key.to_vec(),
        value: value.to_vec(),
        ttl: Some(Ttl {
            milliseconds: 60_000,
        }),
        stale_ttl: None,
        tags: Vec::new(),
        cost: None,
    }
}

fn summarize(
    name: &'static str,
    transport: &'static str,
    notes: &'static str,
    mut samples: Vec<Duration>,
    elapsed: Duration,
    memory_used_bytes: usize,
) -> BenchResult {
    samples.sort_unstable();
    let iterations = samples.len();
    BenchResult {
        name,
        transport,
        iterations,
        p50_ns: percentile_ns(&samples, 50),
        p95_ns: percentile_ns(&samples, 95),
        p99_ns: percentile_ns(&samples, 99),
        throughput_ops_s: iterations as f64 / elapsed.as_secs_f64(),
        memory_used_bytes,
        cost_score_total: 0,
        notes,
    }
}

fn percentile_ns(samples: &[Duration], percentile: usize) -> u128 {
    let index = (samples.len() * percentile / 100).min(samples.len().saturating_sub(1));
    samples[index].as_nanos()
}

fn total_duration(samples: &[Duration]) -> Duration {
    samples
        .iter()
        .copied()
        .fold(Duration::ZERO, |total, sample| total + sample)
}

fn with_memory(mut result: BenchResult, memory_used_bytes: usize) -> BenchResult {
    result.memory_used_bytes = memory_used_bytes;
    result
}

struct NativeLoopbackServer {
    addr: String,
}

impl NativeLoopbackServer {
    fn start(config: Config) -> Self {
        let std_listener = StdTcpListener::bind("127.0.0.1:0").expect("native loopback bind");
        std_listener
            .set_nonblocking(true)
            .expect("native nonblocking listener");
        let addr = std_listener
            .local_addr()
            .expect("native local addr")
            .to_string();

        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_io()
                .build()
                .expect("native tokio runtime");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(std_listener).expect("native listener");
                server::serve_native_tcp(listener, &config)
                    .await
                    .expect("native loopback server");
            });
        });

        Self { addr }
    }
}

#[cfg(unix)]
struct NativeUnixLoopbackServer {
    path: PathBuf,
}

#[cfg(unix)]
impl NativeUnixLoopbackServer {
    fn start(config: Config) -> Self {
        let path = native_unix_socket_path("bench");
        let listener = tokio::net::UnixListener::bind(&path).expect("native unix listener");
        let server_path = path.clone();

        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_io()
                .build()
                .expect("native unix tokio runtime");
            runtime.block_on(async move {
                server::serve_native_unix(listener, &config)
                    .await
                    .expect("native unix loopback server");
            });
        });

        Self { path: server_path }
    }

    fn cleanup(self) {
        let _ = std::fs::remove_file(self.path);
    }
}

#[cfg(unix)]
fn native_unix_socket_path(name: &str) -> PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock")
        .as_nanos();
    std::env::temp_dir().join(format!(
        "cachebox-{name}-{}-{unique}.sock",
        std::process::id()
    ))
}

struct NativeClient {
    stream: NativeClientStream,
    transport: &'static str,
    next_request_id: u64,
}

enum NativeClientStream {
    Tcp(TcpStream),
    #[cfg(unix)]
    Unix(UnixStream),
}

impl NativeClient {
    async fn connect_tcp(addr: &str) -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match TcpStream::connect(addr).await {
                Ok(stream) => {
                    return Self {
                        stream: NativeClientStream::Tcp(stream),
                        transport: "loopback_tcp",
                        next_request_id: 1,
                    };
                }
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => panic!("benchmark native client did not connect: {error}"),
            }
        }
    }

    #[cfg(unix)]
    async fn connect_unix(path: &Path) -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match UnixStream::connect(path).await {
                Ok(stream) => {
                    return Self {
                        stream: NativeClientStream::Unix(stream),
                        transport: "loopback_unix",
                        next_request_id: 1,
                    };
                }
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => panic!("benchmark native unix client did not connect: {error}"),
            }
        }
    }

    async fn request(&mut self, command: Command, payload: RequestPayload) -> ResponsePayload {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let frame = RequestFrame {
            request_id,
            command,
            payload,
        };
        self.write_all(&encode_request_frame(&frame)).await;

        let mut header = [0u8; HEADER_LEN];
        self.read_exact(&mut header).await;
        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("header payload length")) as usize;
        let mut response = Vec::with_capacity(HEADER_LEN + payload_len);
        response.extend_from_slice(&header);
        let start = response.len();
        response.resize(HEADER_LEN + payload_len, 0);
        self.read_exact(&mut response[start..]).await;

        let response =
            decode_response_frame(&response, usize::MAX).expect("native response should decode");
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.command, command);
        response.payload
    }

    async fn write_all(&mut self, bytes: &[u8]) {
        match &mut self.stream {
            NativeClientStream::Tcp(stream) => stream.write_all(bytes).await,
            #[cfg(unix)]
            NativeClientStream::Unix(stream) => stream.write_all(bytes).await,
        }
        .expect("native write");
    }

    async fn read_exact(&mut self, bytes: &mut [u8]) {
        match &mut self.stream {
            NativeClientStream::Tcp(stream) => stream.read_exact(bytes).await,
            #[cfg(unix)]
            NativeClientStream::Unix(stream) => stream.read_exact(bytes).await,
        }
        .expect("native read");
    }
}

#[derive(Debug, Clone)]
struct BenchResult {
    name: &'static str,
    transport: &'static str,
    iterations: usize,
    p50_ns: u128,
    p95_ns: u128,
    p99_ns: u128,
    throughput_ops_s: f64,
    memory_used_bytes: usize,
    cost_score_total: usize,
    notes: &'static str,
}
