use std::net::TcpListener as StdTcpListener;
use std::thread;
use std::time::{Duration, Instant};

use bytes::Bytes;
use cachebox::api::Ttl;
use cachebox::config::Config;
use cachebox::engine::{Engine, GetOutcome, PutCommand};
use cachebox::server;
use http::Request;
use tokio::net::TcpStream;
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

macro_rules! measure_async {
    ($name:expr, $notes:expr, $body:block) => {{
        let mut samples = Vec::new();
        let started = Instant::now();
        while started.elapsed() < MEASURE_DURATION {
            let sample_started = Instant::now();
            $body
            samples.push(sample_started.elapsed());
        }
        summarize($name, "loopback_h2", $notes, samples, started.elapsed(), 0)
    }};
}

fn main() {
    let runtime = Runtime::new().expect("benchmark tokio runtime");
    runtime.block_on(async_main());
}

async fn async_main() {
    let server = LoopbackServer::start(Config::default());
    let eviction_server = LoopbackServer::start(Config {
        max_memory_bytes: 64 * 1024,
        max_value_bytes: 1024,
        ..Config::default()
    });
    let mut client = H2Client::connect(&server.addr).await;
    let mut eviction_client = H2Client::connect(&eviction_server.addr).await;

    let scenarios = [
        bench_engine_get(),
        bench_engine_put(),
        bench_engine_tag_invalidate_8(),
        bench_single_key_get(&mut client).await,
        bench_single_key_put(&mut client).await,
        bench_batch_get(&mut client).await,
        bench_lease_contention(&mut client).await,
        bench_tag_invalidate_empty(&mut client).await,
        bench_tag_invalidate_8(&mut client).await,
        bench_tag_invalidation(&mut client).await,
        bench_ttl_heavy_writes(&mut client).await,
        bench_eviction_pressure(&mut eviction_client).await,
        bench_cost_shaped_writes(&mut client).await,
    ];

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

async fn bench_single_key_get(client: &mut H2Client) -> BenchResult {
    assert_status(
        client
            .request("PUT", "/v1/namespaces/bench/keys/get-key", &[], b"value")
            .await,
        201,
    );
    warmup_async!({
        assert_status(
            client
                .request("GET", "/v1/namespaces/bench/keys/get-key", &[], &[])
                .await,
            200,
        );
    });
    let result = measure_async!("single_key_get", "cached_hit", {
        let response = client
            .request("GET", "/v1/namespaces/bench/keys/get-key", &[], &[])
            .await;
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"value");
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_single_key_put(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let path = format!("/v1/namespaces/bench/keys/put-warmup-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"value").await, 201);
    });
    let result = measure_async!("single_key_put", "unique_keys", {
        let path = format!("/v1/namespaces/bench/keys/put-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"value").await, 201);
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_batch_get(client: &mut H2Client) -> BenchResult {
    for index in 0..32 {
        let path = format!("/v1/namespaces/bench/keys/batch-{index}");
        assert_status(client.request("PUT", &path, &[], b"value").await, 201);
    }
    let body = br#"{"keys":["batch-0","batch-1","batch-2","batch-3","batch-4","batch-5","batch-6","batch-7","batch-8","batch-9","batch-10","batch-11","batch-12","batch-13","batch-14","batch-15","batch-16","batch-17","batch-18","batch-19","batch-20","batch-21","batch-22","batch-23","batch-24","batch-25","batch-26","batch-27","batch-28","batch-29","batch-30","batch-31"]}"#;
    warmup_async!({
        assert_status(
            client
                .request("POST", "/v1/namespaces/bench/batch/get", &[], body)
                .await,
            200,
        );
    });
    let result = measure_async!("batch_get_32", "32_keys", {
        let response = client
            .request("POST", "/v1/namespaces/bench/batch/get", &[], body)
            .await;
        assert_eq!(response.status, 200);
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_lease_contention(client: &mut H2Client) -> BenchResult {
    let body = br#"{"lease_ttl_ms":60000}"#;
    assert_status(
        client
            .request("POST", "/v1/namespaces/bench/leases/contention", &[], body)
            .await,
        200,
    );
    warmup_async!({
        assert_status(
            client
                .request("POST", "/v1/namespaces/bench/leases/contention", &[], body)
                .await,
            200,
        );
    });
    let result = measure_async!("lease_contention", "same_missing_key", {
        let response = client
            .request("POST", "/v1/namespaces/bench/leases/contention", &[], body)
            .await;
        assert_eq!(response.status, 200);
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_tag_invalidation(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        tag_invalidation_round(client, index).await;
        index += 1;
    });
    let result = measure_async!("tag_workflow_put8_invalidate", "8_puts_plus_invalidate", {
        tag_invalidation_round(client, index).await;
        index += 1;
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_tag_invalidate_empty(client: &mut H2Client) -> BenchResult {
    warmup_async!({
        let response = client
            .request(
                "POST",
                "/v1/namespaces/bench/tags/empty-tag/invalidate",
                &[],
                &[],
            )
            .await;
        assert_eq!(response.status, 200);
    });
    let result = measure_async!("tag_invalidate_empty", "single_empty_invalidate", {
        let response = client
            .request(
                "POST",
                "/v1/namespaces/bench/tags/empty-tag/invalidate",
                &[],
                &[],
            )
            .await;
        assert_eq!(response.status, 200);
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_tag_invalidate_8(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let tag = put_tagged_values(client, index).await;
        invalidate_tag(client, &tag).await;
        index += 1;
    });
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let tag = put_tagged_values(client, index).await;
        index += 1;
        let sample_started = Instant::now();
        invalidate_tag(client, &tag).await;
        samples.push(sample_started.elapsed());
    }
    let measured_elapsed = total_duration(&samples);
    let result = summarize(
        "tag_invalidate_8",
        "loopback_h2",
        "single_invalidate_8_tagged_keys",
        samples,
        measured_elapsed,
        0,
    );
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_ttl_heavy_writes(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let path = format!("/v1/namespaces/bench/keys/ttl-warmup-{index}");
        index += 1;
        assert_status(
            client
                .request(
                    "PUT",
                    &path,
                    &[("Cachebox-TTL", "60s"), ("Cachebox-Stale-TTL", "60s")],
                    b"value",
                )
                .await,
            201,
        );
    });
    let result = measure_async!("ttl_heavy_writes", "ttl_and_stale_ttl", {
        let path = format!("/v1/namespaces/bench/keys/ttl-{index}");
        index += 1;
        assert_status(
            client
                .request(
                    "PUT",
                    &path,
                    &[("Cachebox-TTL", "60s"), ("Cachebox-Stale-TTL", "60s")],
                    b"value",
                )
                .await,
            201,
        );
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_eviction_pressure(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        let path = format!("/v1/namespaces/bench/keys/evict-warmup-{index}");
        index += 1;
        assert_status(
            client.request("PUT", &path, &[], b"0123456789abcdef").await,
            201,
        );
    });
    let result = measure_async!("eviction_pressure", "64KiB_cap", {
        let path = format!("/v1/namespaces/bench/keys/evict-{index}");
        index += 1;
        assert_status(
            client.request("PUT", &path, &[], b"0123456789abcdef").await,
            201,
        );
    });
    with_memory(result, client.memory_used_bytes().await)
}

async fn bench_cost_shaped_writes(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup_async!({
        cost_shaped_round(client, index).await;
        index += 1;
    });
    let result = measure_async!(
        "cost_shaped_writes",
        "cheap_large_expensive_small_mixed_ttl",
        {
            cost_shaped_round(client, index).await;
            index += 1;
        }
    );
    with_metrics(result, client).await
}

async fn tag_invalidation_round(client: &mut H2Client, index: usize) {
    let tag = put_tagged_values(client, index).await;
    invalidate_tag(client, &tag).await;
}

async fn put_tagged_values(client: &mut H2Client, index: usize) -> String {
    let tag = format!("tag-{index}");
    for item in 0..8 {
        let path = format!("/v1/namespaces/bench/keys/tag-{index}-{item}");
        assert_status(
            client
                .request("PUT", &path, &[("Cachebox-Tags", &tag)], b"value")
                .await,
            201,
        );
    }
    tag
}

async fn invalidate_tag(client: &mut H2Client, tag: &str) {
    assert_status(
        client
            .request(
                "POST",
                &format!("/v1/namespaces/bench/tags/{tag}/invalidate"),
                &[],
                &[],
            )
            .await,
        200,
    );
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

async fn cost_shaped_round(client: &mut H2Client, index: usize) {
    let large_value = [b'x'; 512];
    let writes = [
        (
            format!("/v1/namespaces/bench/keys/cost-large-{index}"),
            vec![("Cachebox-Cost", "1")],
            large_value.as_slice(),
        ),
        (
            format!("/v1/namespaces/bench/keys/cost-small-{index}"),
            vec![("Cachebox-Cost", "1000")],
            b"x".as_slice(),
        ),
        (
            format!("/v1/namespaces/bench/keys/cost-ttl-{index}"),
            vec![("Cachebox-Cost", "500"), ("Cachebox-TTL", "60s")],
            b"ttl".as_slice(),
        ),
    ];
    for (path, headers, body) in writes {
        assert_status(client.request("PUT", &path, &headers, body).await, 201);
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

async fn with_metrics(mut result: BenchResult, client: &mut H2Client) -> BenchResult {
    result.memory_used_bytes = client.memory_used_bytes().await;
    result.cost_score_total = client.cost_score_total().await;
    result
}

fn assert_status(response: Response, expected: u16) {
    assert_eq!(
        response.status, expected,
        "response body: {:?}",
        response.body
    );
}

struct LoopbackServer {
    addr: String,
}

impl LoopbackServer {
    fn start(config: Config) -> Self {
        let std_listener = StdTcpListener::bind("127.0.0.1:0").expect("loopback bind");
        std_listener
            .set_nonblocking(true)
            .expect("nonblocking listener");
        let addr = std_listener.local_addr().expect("local addr").to_string();

        thread::spawn(move || {
            let runtime = tokio::runtime::Builder::new_multi_thread()
                .enable_io()
                .build()
                .expect("tokio runtime");
            runtime.block_on(async move {
                let listener =
                    tokio::net::TcpListener::from_std(std_listener).expect("tokio listener");
                axum::serve(listener, server::app(&config))
                    .await
                    .expect("loopback server");
            });
        });

        Self { addr }
    }
}

struct H2Client {
    authority: String,
    sender: h2::client::SendRequest<Bytes>,
}

impl H2Client {
    async fn connect(addr: &str) -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            match Self::try_connect(addr).await {
                Ok((sender, authority)) => {
                    let mut client = Self { authority, sender };
                    wait_for_health(&mut client).await;
                    return client;
                }
                Err(_) if Instant::now() < deadline => {
                    tokio::time::sleep(Duration::from_millis(20)).await;
                }
                Err(error) => panic!("benchmark h2 client did not connect: {error}"),
            }
        }
    }

    async fn try_connect(
        addr: &str,
    ) -> Result<(h2::client::SendRequest<Bytes>, String), Box<dyn std::error::Error + Send + Sync>>
    {
        let stream = TcpStream::connect(addr).await?;
        let (sender, connection) = h2::client::handshake(stream).await?;
        tokio::spawn(async move {
            if let Err(error) = connection.await {
                eprintln!("benchmark h2 connection error: {error}");
            }
        });
        Ok((sender, addr.to_string()))
    }

    async fn request(
        &mut self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Response {
        h2_request(
            &mut self.sender,
            &self.authority,
            method,
            path,
            headers,
            body,
        )
        .await
        .expect("h2 request should succeed")
    }

    async fn memory_used_bytes(&mut self) -> usize {
        let response = self.request("GET", "/metrics", &[], &[]).await;
        assert_eq!(response.status, 200);
        let body = String::from_utf8(response.body).expect("metrics utf-8");
        metric_value(&body, "cachebox_memory_used_bytes")
    }

    async fn cost_score_total(&mut self) -> usize {
        let response = self.request("GET", "/metrics", &[], &[]).await;
        assert_eq!(response.status, 200);
        let body = String::from_utf8(response.body).expect("metrics utf-8");
        metric_value(&body, "cachebox_cost_score_total")
    }
}

async fn h2_request(
    sender: &mut h2::client::SendRequest<Bytes>,
    authority: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: &[u8],
) -> Result<Response, Box<dyn std::error::Error + Send + Sync>> {
    let mut ready = sender.clone().ready().await?;
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .header("host", authority);
    for (name, value) in headers {
        builder = builder.header(*name, *value);
    }
    let request = builder.body(())?;
    let (response_future, mut stream) = ready.send_request(request, body.is_empty())?;
    if !body.is_empty() {
        stream.send_data(Bytes::copy_from_slice(body), true)?;
    }

    let response = response_future.await?;
    let status = response.status().as_u16();
    let mut body = Vec::new();
    let mut recv = response.into_body();
    while let Some(chunk) = recv.data().await {
        body.extend_from_slice(&chunk?);
    }
    Ok(Response { status, body })
}

async fn wait_for_health(client: &mut H2Client) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if client.request("GET", "/healthz", &[], &[]).await.status == 200 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("benchmark loopback server did not become healthy");
}

fn metric_value(body: &str, name: &str) -> usize {
    body.lines()
        .find_map(|line| {
            let (metric, value) = line.split_once(' ')?;
            if metric == name {
                value.parse::<usize>().ok()
            } else {
                None
            }
        })
        .expect("metric should exist")
}

#[derive(Debug)]
struct Response {
    status: u16,
    body: Vec<u8>,
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
