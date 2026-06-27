use std::net::TcpListener as StdTcpListener;
use std::thread;
use std::time::{Duration, Instant};

use bytes::Bytes;
use cachebox::config::Config;
use cachebox::server;
use http::Request;
use tokio::net::TcpStream;
use tokio::runtime::Runtime;

const WARMUP_ITERS: usize = 100;
const MEASURE_DURATION: Duration = Duration::from_secs(1);

fn main() {
    let server = LoopbackServer::start(Config::default());
    let eviction_server = LoopbackServer::start(Config {
        max_memory_bytes: 64 * 1024,
        max_value_bytes: 1024,
        ..Config::default()
    });
    let mut client = H2Client::connect(&server.addr);
    let mut eviction_client = H2Client::connect(&eviction_server.addr);

    let scenarios = [
        bench_single_key_get(&mut client),
        bench_single_key_put(&mut client),
        bench_batch_get(&mut client),
        bench_lease_contention(&mut client),
        bench_tag_invalidation(&mut client),
        bench_ttl_heavy_writes(&mut client),
        bench_eviction_pressure(&mut eviction_client),
        bench_cost_shaped_writes(&mut client),
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

fn bench_single_key_get(client: &mut H2Client) -> BenchResult {
    assert_status(
        client.request("PUT", "/v1/namespaces/bench/keys/get-key", &[], b"value"),
        201,
    );
    warmup(|| {
        assert_status(
            client.request("GET", "/v1/namespaces/bench/keys/get-key", &[], &[]),
            200,
        );
    });
    let result = measure("single_key_get", "cached_hit", || {
        let response = client.request("GET", "/v1/namespaces/bench/keys/get-key", &[], &[]);
        assert_eq!(response.status, 200);
        assert_eq!(response.body, b"value");
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_single_key_put(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup(|| {
        let path = format!("/v1/namespaces/bench/keys/put-warmup-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"value"), 201);
    });
    let result = measure("single_key_put", "unique_keys", || {
        let path = format!("/v1/namespaces/bench/keys/put-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"value"), 201);
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_batch_get(client: &mut H2Client) -> BenchResult {
    for index in 0..32 {
        let path = format!("/v1/namespaces/bench/keys/batch-{index}");
        assert_status(client.request("PUT", &path, &[], b"value"), 201);
    }
    let body = br#"{"keys":["batch-0","batch-1","batch-2","batch-3","batch-4","batch-5","batch-6","batch-7","batch-8","batch-9","batch-10","batch-11","batch-12","batch-13","batch-14","batch-15","batch-16","batch-17","batch-18","batch-19","batch-20","batch-21","batch-22","batch-23","batch-24","batch-25","batch-26","batch-27","batch-28","batch-29","batch-30","batch-31"]}"#;
    warmup(|| {
        assert_status(
            client.request("POST", "/v1/namespaces/bench/batch/get", &[], body),
            200,
        );
    });
    let result = measure("batch_get_32", "32_keys", || {
        let response = client.request("POST", "/v1/namespaces/bench/batch/get", &[], body);
        assert_eq!(response.status, 200);
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_lease_contention(client: &mut H2Client) -> BenchResult {
    let body = br#"{"lease_ttl_ms":60000}"#;
    assert_status(
        client.request("POST", "/v1/namespaces/bench/leases/contention", &[], body),
        200,
    );
    warmup(|| {
        assert_status(
            client.request("POST", "/v1/namespaces/bench/leases/contention", &[], body),
            200,
        );
    });
    let result = measure("lease_contention", "same_missing_key", || {
        let response = client.request("POST", "/v1/namespaces/bench/leases/contention", &[], body);
        assert_eq!(response.status, 200);
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_tag_invalidation(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup(|| {
        tag_invalidation_round(client, index);
        index += 1;
    });
    let result = measure("tag_invalidation_8", "put_then_invalidate", || {
        tag_invalidation_round(client, index);
        index += 1;
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_ttl_heavy_writes(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup(|| {
        let path = format!("/v1/namespaces/bench/keys/ttl-warmup-{index}");
        index += 1;
        assert_status(
            client.request(
                "PUT",
                &path,
                &[("Cachebox-TTL", "60s"), ("Cachebox-Stale-TTL", "60s")],
                b"value",
            ),
            201,
        );
    });
    let result = measure("ttl_heavy_writes", "ttl_and_stale_ttl", || {
        let path = format!("/v1/namespaces/bench/keys/ttl-{index}");
        index += 1;
        assert_status(
            client.request(
                "PUT",
                &path,
                &[("Cachebox-TTL", "60s"), ("Cachebox-Stale-TTL", "60s")],
                b"value",
            ),
            201,
        );
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_eviction_pressure(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup(|| {
        let path = format!("/v1/namespaces/bench/keys/evict-warmup-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"0123456789abcdef"), 201);
    });
    let result = measure("eviction_pressure", "64KiB_cap", || {
        let path = format!("/v1/namespaces/bench/keys/evict-{index}");
        index += 1;
        assert_status(client.request("PUT", &path, &[], b"0123456789abcdef"), 201);
    });
    with_memory(result, client.memory_used_bytes())
}

fn bench_cost_shaped_writes(client: &mut H2Client) -> BenchResult {
    let mut index = 0usize;
    warmup(|| {
        cost_shaped_round(client, index);
        index += 1;
    });
    let result = measure(
        "cost_shaped_writes",
        "cheap_large_expensive_small_mixed_ttl",
        || {
            cost_shaped_round(client, index);
            index += 1;
        },
    );
    with_metrics(result, client)
}

fn tag_invalidation_round(client: &mut H2Client, index: usize) {
    let tag = format!("tag-{index}");
    for item in 0..8 {
        let path = format!("/v1/namespaces/bench/keys/tag-{index}-{item}");
        assert_status(
            client.request("PUT", &path, &[("Cachebox-Tags", &tag)], b"value"),
            201,
        );
    }
    assert_status(
        client.request(
            "POST",
            &format!("/v1/namespaces/bench/tags/{tag}/invalidate"),
            &[],
            &[],
        ),
        200,
    );
}

fn cost_shaped_round(client: &mut H2Client, index: usize) {
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
        assert_status(client.request("PUT", &path, &headers, body), 201);
    }
}

fn warmup(mut operation: impl FnMut()) {
    for _ in 0..WARMUP_ITERS {
        operation();
    }
}

fn measure(name: &'static str, notes: &'static str, mut operation: impl FnMut()) -> BenchResult {
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        operation();
        samples.push(sample_started.elapsed());
    }
    summarize(name, notes, samples, started.elapsed(), 0)
}

fn summarize(
    name: &'static str,
    notes: &'static str,
    mut samples: Vec<Duration>,
    elapsed: Duration,
    memory_used_bytes: usize,
) -> BenchResult {
    samples.sort_unstable();
    let iterations = samples.len();
    BenchResult {
        name,
        transport: "loopback_h2",
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

fn with_memory(mut result: BenchResult, memory_used_bytes: usize) -> BenchResult {
    result.memory_used_bytes = memory_used_bytes;
    result
}

fn with_metrics(mut result: BenchResult, client: &mut H2Client) -> BenchResult {
    result.memory_used_bytes = client.memory_used_bytes();
    result.cost_score_total = client.cost_score_total();
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
    runtime: Runtime,
    authority: String,
    sender: h2::client::SendRequest<Bytes>,
}

impl H2Client {
    fn connect(addr: &str) -> Self {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let runtime = Runtime::new().expect("benchmark h2 runtime");
            match runtime.block_on(Self::try_connect(addr)) {
                Ok((sender, authority)) => {
                    let mut client = Self {
                        runtime,
                        authority,
                        sender,
                    };
                    wait_for_health(&mut client);
                    return client;
                }
                Err(_) if Instant::now() < deadline => thread::sleep(Duration::from_millis(20)),
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

    fn request(
        &mut self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
    ) -> Response {
        let authority = self.authority.clone();
        let sender = &mut self.sender;
        self.runtime
            .block_on(h2_request(sender, &authority, method, path, headers, body))
            .expect("h2 request should succeed")
    }

    fn memory_used_bytes(&mut self) -> usize {
        let response = self.request("GET", "/metrics", &[], &[]);
        assert_eq!(response.status, 200);
        let body = String::from_utf8(response.body).expect("metrics utf-8");
        metric_value(&body, "cachebox_memory_used_bytes")
    }

    fn cost_score_total(&mut self) -> usize {
        let response = self.request("GET", "/metrics", &[], &[]);
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

fn wait_for_health(client: &mut H2Client) {
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if client.request("GET", "/healthz", &[], &[]).status == 200 {
            return;
        }
        thread::sleep(Duration::from_millis(20));
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
