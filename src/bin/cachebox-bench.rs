use std::net::TcpListener as StdTcpListener;
#[cfg(unix)]
use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, Instant};

use cachebox::config::Config;
use cachebox::engine::{Engine, GetOutcome, GetOutcomeRef, PutCommand, ShardedEngine};
use cachebox::protocol::{
    BatchItem, Command, HEADER_LEN, Metadata, RequestFrame, RequestPayload, ResponsePayload,
    ResponsePayloadView, Ttl, decode_request_frame, decode_response_frame,
    encode_request_frame_into, encode_response_payload_view_frame_into,
};
use cachebox::server;
use cachebox_client::NativeClient as OfficialNativeClient;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
#[cfg(unix)]
use tokio::net::UnixStream;
use tokio::runtime::Runtime;
use tokio::sync::mpsc;

const WARMUP_ITERS: usize = 100;
const MEASURE_DURATION: Duration = Duration::from_secs(1);
const CONCURRENT_CLIENTS: usize = 16;
const PIPELINE_DEPTH: usize = 32;

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
    let mut official_client = OfficialNativeClient::connect_tcp(&native_server.addr)
        .await
        .expect("official native tcp client");

    let mut scenarios = vec![
        bench_engine_get(),
        bench_engine_put(),
        bench_engine_tag_invalidate_8(),
        bench_protocol_decode_get(),
        bench_protocol_encode_hit(),
        bench_engine_get_ref_encode(),
        bench_sharded_get_ref_encode(),
        bench_sharded_get_ref_no_access_encode(),
        bench_tokio_spawn_ready().await,
        bench_tokio_spawn_mpsc_response().await,
        bench_native_single_key_get(&mut native_client).await,
        bench_native_single_key_put(&mut native_client).await,
        bench_native_batch_get(&mut native_client).await,
        bench_native_lease_contention(&mut native_client).await,
        bench_native_tag_invalidate_empty(&mut native_client).await,
        bench_native_tag_invalidate_8(&mut native_client).await,
        bench_native_tag_invalidation(&mut native_client).await,
        bench_native_ttl_heavy_writes(&mut native_client).await,
        bench_native_pipelined_get(&mut native_client).await,
        bench_official_sequential_get_32(&mut official_client, "loopback_tcp").await,
        bench_official_pipelined_get_32(&mut official_client, "loopback_tcp").await,
        bench_native_concurrent_get_tcp(&native_server.addr).await,
        bench_native_concurrent_get_distinct_tcp(&native_server.addr).await,
        bench_native_concurrent_put_tcp(&native_server.addr).await,
        bench_native_short_connection_get_tcp(&native_server.addr).await,
    ];

    #[cfg(unix)]
    {
        let native_unix_server = NativeUnixLoopbackServer::start(Config::default());
        let mut native_unix_client = NativeClient::connect_unix(&native_unix_server.path).await;
        let mut official_unix_client = OfficialNativeClient::connect_unix(&native_unix_server.path)
            .await
            .expect("official native unix client");
        scenarios.extend([
            bench_native_single_key_get(&mut native_unix_client).await,
            bench_native_single_key_put(&mut native_unix_client).await,
            bench_native_batch_get(&mut native_unix_client).await,
            bench_native_lease_contention(&mut native_unix_client).await,
            bench_native_tag_invalidate_empty(&mut native_unix_client).await,
            bench_native_tag_invalidate_8(&mut native_unix_client).await,
            bench_native_tag_invalidation(&mut native_unix_client).await,
            bench_native_ttl_heavy_writes(&mut native_unix_client).await,
            bench_native_pipelined_get(&mut native_unix_client).await,
            bench_official_sequential_get_32(&mut official_unix_client, "loopback_unix").await,
            bench_official_pipelined_get_32(&mut official_unix_client, "loopback_unix").await,
            bench_native_concurrent_get_unix(&native_unix_server.path).await,
            bench_native_concurrent_get_distinct_unix(&native_unix_server.path).await,
            bench_native_concurrent_put_unix(&native_unix_server.path).await,
            bench_native_short_connection_get_unix(&native_unix_server.path).await,
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

fn bench_protocol_decode_get() -> BenchResult {
    let mut frame = Vec::new();
    encode_request_frame_into(
        &RequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: native_get_payload(b"decode-key"),
        },
        &mut frame,
    );
    warmup_sync(|| {
        let decoded = decode_request_frame(&frame, usize::MAX).expect("get frame should decode");
        assert_eq!(decoded.command, Command::Get);
    });
    measure_process_sync("protocol_decode_get", "decode_prebuilt_get_frame", || {
        let decoded = decode_request_frame(&frame, usize::MAX).expect("get frame should decode");
        assert_eq!(decoded.command, Command::Get);
    })
}

fn bench_protocol_encode_hit() -> BenchResult {
    let value = b"value";
    let mut out = Vec::new();
    warmup_sync(|| {
        encode_response_payload_view_frame_into(
            1,
            Command::Get,
            ResponsePayloadView::Hit(value),
            &mut out,
        );
    });
    measure_process_sync(
        "protocol_encode_hit",
        "encode_borrowed_hit_response",
        || {
            encode_response_payload_view_frame_into(
                1,
                Command::Get,
                ResponsePayloadView::Hit(value),
                &mut out,
            );
        },
    )
}

fn bench_engine_get_ref_encode() -> BenchResult {
    let mut engine = Engine::new();
    engine
        .put(put_command("bench", b"get-ref-encode", b"value"))
        .expect("engine put should fit");
    let mut out = Vec::new();
    warmup_sync(|| {
        engine.get_ref("bench", b"get-ref-encode", |outcome| match outcome {
            cachebox::engine::GetOutcomeRef::Hit(value) => encode_response_payload_view_frame_into(
                1,
                Command::Get,
                ResponsePayloadView::Hit(value),
                &mut out,
            ),
            other => panic!("expected hit, got {other:?}"),
        });
    });
    let result = measure_process_sync(
        "engine_get_ref_encode",
        "engine_get_ref_plus_borrowed_encode",
        || {
            engine.get_ref("bench", b"get-ref-encode", |outcome| match outcome {
                cachebox::engine::GetOutcomeRef::Hit(value) => {
                    encode_response_payload_view_frame_into(
                        1,
                        Command::Get,
                        ResponsePayloadView::Hit(value),
                        &mut out,
                    );
                }
                other => panic!("expected hit, got {other:?}"),
            });
        },
    );
    with_memory(result, engine.memory_used_bytes())
}

fn bench_sharded_get_ref_encode() -> BenchResult {
    let engine = ShardedEngine::with_limits(Default::default());
    engine
        .put(put_command("bench", b"sharded-get-ref-encode", b"value"))
        .expect("engine put should fit");
    let mut out = Vec::new();
    warmup_sync(|| {
        encode_sharded_get_ref_response(&engine, b"sharded-get-ref-encode", true, &mut out);
    });
    let result = measure_process_sync(
        "sharded_get_ref_encode",
        "shard_lock_get_ref_access_update_encode",
        || {
            encode_sharded_get_ref_response(&engine, b"sharded-get-ref-encode", true, &mut out);
        },
    );
    with_memory(result, engine.memory_used_bytes())
}

fn bench_sharded_get_ref_no_access_encode() -> BenchResult {
    let engine = ShardedEngine::with_limits(Default::default());
    engine
        .put(put_command(
            "bench",
            b"sharded-get-ref-no-access-encode",
            b"value",
        ))
        .expect("engine put should fit");
    let mut out = Vec::new();
    warmup_sync(|| {
        encode_sharded_get_ref_response(
            &engine,
            b"sharded-get-ref-no-access-encode",
            false,
            &mut out,
        );
    });
    let result = measure_process_sync(
        "sharded_get_ref_no_access_encode",
        "shard_lock_get_ref_without_access_update_encode",
        || {
            encode_sharded_get_ref_response(
                &engine,
                b"sharded-get-ref-no-access-encode",
                false,
                &mut out,
            );
        },
    );
    with_memory(result, engine.memory_used_bytes())
}

fn encode_sharded_get_ref_response(
    engine: &ShardedEngine,
    key: &[u8],
    update_access: bool,
    out: &mut Vec<u8>,
) {
    if update_access {
        engine.get_ref("bench", key, |outcome| match outcome {
            GetOutcomeRef::Hit(value) => encode_response_payload_view_frame_into(
                1,
                Command::Get,
                ResponsePayloadView::Hit(value),
                out,
            ),
            other => panic!("expected hit, got {other:?}"),
        });
    } else {
        engine.get_ref_without_access_update("bench", key, |outcome| match outcome {
            GetOutcomeRef::Hit(value) => encode_response_payload_view_frame_into(
                1,
                Command::Get,
                ResponsePayloadView::Hit(value),
                out,
            ),
            other => panic!("expected hit, got {other:?}"),
        });
    }
}

async fn bench_tokio_spawn_ready() -> BenchResult {
    for _ in 0..WARMUP_ITERS {
        tokio::spawn(async {}).await.expect("spawn should join");
    }
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        tokio::spawn(async {}).await.expect("spawn should join");
        samples.push(sample_started.elapsed());
    }
    summarize(
        "tokio_spawn_ready",
        "process",
        "spawn_empty_task_and_join",
        samples,
        started.elapsed(),
        0,
    )
}

async fn bench_tokio_spawn_mpsc_response() -> BenchResult {
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(1024);
    for _ in 0..WARMUP_ITERS {
        let tx = tx.clone();
        tokio::spawn(async move {
            tx.send(Vec::new()).await.expect("warmup send");
        });
        rx.recv().await.expect("warmup recv");
    }
    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let tx = tx.clone();
        let sample_started = Instant::now();
        tokio::spawn(async move {
            tx.send(Vec::new()).await.expect("benchmark send");
        });
        rx.recv().await.expect("benchmark recv");
        samples.push(sample_started.elapsed());
    }
    summarize(
        "tokio_spawn_mpsc_response",
        "process",
        "spawn_task_send_response_vec",
        samples,
        started.elapsed(),
        0,
    )
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

async fn bench_native_pipelined_get(client: &mut NativeClient) -> BenchResult {
    assert_eq!(
        client
            .request(
                Command::Put,
                native_put_payload(b"pipeline-get-key", Metadata::default(), b"value")
            )
            .await,
        ResponsePayload::Stored { evicted: 0 }
    );
    warmup_async!({
        native_pipelined_get_round(client).await;
    });

    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        native_pipelined_get_round(client).await;
        let per_request = sample_started.elapsed() / PIPELINE_DEPTH as u32;
        samples.extend(std::iter::repeat_n(per_request, PIPELINE_DEPTH));
    }
    summarize(
        "pipelined_get_32",
        client.transport,
        "one_connection_32_outstanding_gets",
        samples,
        started.elapsed(),
        0,
    )
}

async fn native_pipelined_get_round(client: &mut NativeClient) {
    let mut expected_ids = Vec::with_capacity(PIPELINE_DEPTH);
    for _ in 0..PIPELINE_DEPTH {
        let request_id = client
            .send_request(Command::Get, native_get_payload(b"pipeline-get-key"))
            .await;
        expected_ids.push(request_id);
    }

    let mut seen = vec![false; PIPELINE_DEPTH];
    for _ in 0..PIPELINE_DEPTH {
        let response = client.read_response().await;
        let index = expected_ids
            .iter()
            .position(|request_id| *request_id == response.request_id)
            .expect("pipelined response id should match a request");
        assert!(!seen[index], "duplicate pipelined response id");
        seen[index] = true;
        assert_eq!(response.command, Command::Get);
        assert_eq!(response.payload, ResponsePayload::Hit(b"value".to_vec()));
    }
    assert!(seen.into_iter().all(|seen| seen));
}

async fn bench_official_sequential_get_32(
    client: &mut OfficialNativeClient,
    transport: &'static str,
) -> BenchResult {
    client
        .put(
            "default",
            b"official-sequential-get-key".to_vec(),
            Metadata::default(),
            b"value".to_vec(),
        )
        .await
        .expect("official put");
    warmup_async!({
        official_sequential_get_32_round(client, b"official-sequential-get-key").await;
    });

    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        official_sequential_get_32_round(client, b"official-sequential-get-key").await;
        let per_request = sample_started.elapsed() / PIPELINE_DEPTH as u32;
        samples.extend(std::iter::repeat_n(per_request, PIPELINE_DEPTH));
    }
    summarize(
        "client_sequential_get_32",
        transport,
        "official_client_32_sequential_gets",
        samples,
        started.elapsed(),
        0,
    )
}

async fn official_sequential_get_32_round(client: &mut OfficialNativeClient, key: &[u8]) {
    for _ in 0..PIPELINE_DEPTH {
        assert_eq!(
            client
                .get("default", key.to_vec())
                .await
                .expect("official sequential get"),
            ResponsePayload::Hit(b"value".to_vec())
        );
    }
}

async fn bench_official_pipelined_get_32(
    client: &mut OfficialNativeClient,
    transport: &'static str,
) -> BenchResult {
    client
        .put(
            "default",
            b"official-pipelined-get-key".to_vec(),
            Metadata::default(),
            b"value".to_vec(),
        )
        .await
        .expect("official put");
    warmup_async!({
        official_pipelined_get_32_round(client, b"official-pipelined-get-key").await;
    });

    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        official_pipelined_get_32_round(client, b"official-pipelined-get-key").await;
        let per_request = sample_started.elapsed() / PIPELINE_DEPTH as u32;
        samples.extend(std::iter::repeat_n(per_request, PIPELINE_DEPTH));
    }
    summarize(
        "client_pipelined_get_32",
        transport,
        "official_client_32_pipelined_gets",
        samples,
        started.elapsed(),
        0,
    )
}

async fn official_pipelined_get_32_round(client: &mut OfficialNativeClient, key: &[u8]) {
    let requests = (0..PIPELINE_DEPTH)
        .map(|_| {
            (
                Command::Get,
                RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: key.to_vec(),
                },
            )
        })
        .collect();
    let responses = client
        .request_pipelined(requests)
        .await
        .expect("official pipelined get");
    assert_eq!(responses.len(), PIPELINE_DEPTH);
    for response in responses {
        assert_eq!(response, ResponsePayload::Hit(b"value".to_vec()));
    }
}

async fn bench_native_concurrent_get_tcp(addr: &str) -> BenchResult {
    let mut setup = NativeClient::connect_tcp(addr).await;
    let addr = addr.to_string();
    bench_native_concurrent_get(
        "loopback_tcp",
        move || {
            let addr = addr.clone();
            async move { NativeClient::connect_tcp(&addr).await }
        },
        &mut setup,
    )
    .await
}

async fn bench_native_concurrent_get_distinct_tcp(addr: &str) -> BenchResult {
    let mut setup = NativeClient::connect_tcp(addr).await;
    let addr = addr.to_string();
    bench_native_concurrent_get_distinct(
        "loopback_tcp",
        move || {
            let addr = addr.clone();
            async move { NativeClient::connect_tcp(&addr).await }
        },
        &mut setup,
    )
    .await
}

#[cfg(unix)]
async fn bench_native_concurrent_get_unix(path: &Path) -> BenchResult {
    let mut setup = NativeClient::connect_unix(path).await;
    let path = path.to_path_buf();
    bench_native_concurrent_get(
        "loopback_unix",
        move || {
            let path = path.clone();
            async move { NativeClient::connect_unix(&path).await }
        },
        &mut setup,
    )
    .await
}

#[cfg(unix)]
async fn bench_native_concurrent_get_distinct_unix(path: &Path) -> BenchResult {
    let mut setup = NativeClient::connect_unix(path).await;
    let path = path.to_path_buf();
    bench_native_concurrent_get_distinct(
        "loopback_unix",
        move || {
            let path = path.clone();
            async move { NativeClient::connect_unix(&path).await }
        },
        &mut setup,
    )
    .await
}

async fn bench_native_concurrent_put_tcp(addr: &str) -> BenchResult {
    let addr = addr.to_string();
    bench_native_concurrent_put("loopback_tcp", move || {
        let addr = addr.clone();
        async move { NativeClient::connect_tcp(&addr).await }
    })
    .await
}

#[cfg(unix)]
async fn bench_native_concurrent_put_unix(path: &Path) -> BenchResult {
    let path = path.to_path_buf();
    bench_native_concurrent_put("loopback_unix", move || {
        let path = path.clone();
        async move { NativeClient::connect_unix(&path).await }
    })
    .await
}

async fn bench_native_short_connection_get_tcp(addr: &str) -> BenchResult {
    let mut setup = NativeClient::connect_tcp(addr).await;
    let addr = addr.to_string();
    bench_native_short_connection_get(
        "loopback_tcp",
        move || {
            let addr = addr.clone();
            async move { NativeClient::connect_tcp(&addr).await }
        },
        &mut setup,
    )
    .await
}

#[cfg(unix)]
async fn bench_native_short_connection_get_unix(path: &Path) -> BenchResult {
    let mut setup = NativeClient::connect_unix(path).await;
    let path = path.to_path_buf();
    bench_native_short_connection_get(
        "loopback_unix",
        move || {
            let path = path.clone();
            async move { NativeClient::connect_unix(&path).await }
        },
        &mut setup,
    )
    .await
}

async fn bench_native_concurrent_get<F, Fut>(
    transport: &'static str,
    connect: F,
    setup: &mut NativeClient,
) -> BenchResult
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = NativeClient> + Send + 'static,
{
    assert_eq!(
        setup
            .request(
                Command::Put,
                native_put_payload(b"concurrent-get-key", Metadata::default(), b"value")
            )
            .await,
        ResponsePayload::Stored { evicted: 0 }
    );

    let started = Instant::now();
    let deadline = started + MEASURE_DURATION;
    let mut handles = Vec::with_capacity(CONCURRENT_CLIENTS);
    for _ in 0..CONCURRENT_CLIENTS {
        let connect = connect.clone();
        handles.push(tokio::spawn(async move {
            let mut client = connect().await;
            let mut samples = Vec::new();
            while Instant::now() < deadline {
                let sample_started = Instant::now();
                assert_eq!(
                    client
                        .request(Command::Get, native_get_payload(b"concurrent-get-key"))
                        .await,
                    ResponsePayload::Hit(b"value".to_vec())
                );
                samples.push(sample_started.elapsed());
            }
            samples
        }));
    }
    summarize_joined_samples(
        "concurrent_get_16",
        transport,
        "16_clients_cached_hit",
        started,
        handles,
    )
    .await
}

async fn bench_native_concurrent_get_distinct<F, Fut>(
    transport: &'static str,
    connect: F,
    setup: &mut NativeClient,
) -> BenchResult
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = NativeClient> + Send + 'static,
{
    for client_index in 0..CONCURRENT_CLIENTS {
        let key = format!("concurrent-get-distinct-{client_index}");
        assert_eq!(
            setup
                .request(
                    Command::Put,
                    native_put_payload(key.as_bytes(), Metadata::default(), b"value")
                )
                .await,
            ResponsePayload::Stored { evicted: 0 }
        );
    }

    let started = Instant::now();
    let deadline = started + MEASURE_DURATION;
    let mut handles = Vec::with_capacity(CONCURRENT_CLIENTS);
    for client_index in 0..CONCURRENT_CLIENTS {
        let connect = connect.clone();
        handles.push(tokio::spawn(async move {
            let mut client = connect().await;
            let key = format!("concurrent-get-distinct-{client_index}");
            let mut samples = Vec::new();
            while Instant::now() < deadline {
                let sample_started = Instant::now();
                assert_eq!(
                    client
                        .request(Command::Get, native_get_payload(key.as_bytes()))
                        .await,
                    ResponsePayload::Hit(b"value".to_vec())
                );
                samples.push(sample_started.elapsed());
            }
            samples
        }));
    }
    summarize_joined_samples(
        "concurrent_get_16_distinct",
        transport,
        "16_clients_distinct_cached_hits",
        started,
        handles,
    )
    .await
}

async fn bench_native_concurrent_put<F, Fut>(transport: &'static str, connect: F) -> BenchResult
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = NativeClient> + Send + 'static,
{
    let started = Instant::now();
    let deadline = started + MEASURE_DURATION;
    let mut handles = Vec::with_capacity(CONCURRENT_CLIENTS);
    for client_index in 0..CONCURRENT_CLIENTS {
        let connect = connect.clone();
        handles.push(tokio::spawn(async move {
            let mut client = connect().await;
            let mut samples = Vec::new();
            let mut index = 0usize;
            while Instant::now() < deadline {
                let key = format!("concurrent-put-{client_index}-{index}");
                index += 1;
                let sample_started = Instant::now();
                assert_eq!(
                    client
                        .request(
                            Command::Put,
                            native_put_payload(key.as_bytes(), Metadata::default(), b"value")
                        )
                        .await,
                    ResponsePayload::Stored { evicted: 0 }
                );
                samples.push(sample_started.elapsed());
            }
            samples
        }));
    }
    summarize_joined_samples(
        "concurrent_put_16",
        transport,
        "16_clients_unique_keys",
        started,
        handles,
    )
    .await
}

async fn bench_native_short_connection_get<F, Fut>(
    transport: &'static str,
    connect: F,
    setup: &mut NativeClient,
) -> BenchResult
where
    F: Fn() -> Fut + Clone + Send + Sync + 'static,
    Fut: std::future::Future<Output = NativeClient> + Send + 'static,
{
    assert_eq!(
        setup
            .request(
                Command::Put,
                native_put_payload(b"short-connection-key", Metadata::default(), b"value")
            )
            .await,
        ResponsePayload::Stored { evicted: 0 }
    );
    warmup_async!({
        let mut client = connect().await;
        assert_eq!(
            client
                .request(Command::Get, native_get_payload(b"short-connection-key"))
                .await,
            ResponsePayload::Hit(b"value".to_vec())
        );
    });

    let mut samples = Vec::new();
    let started = Instant::now();
    while started.elapsed() < MEASURE_DURATION {
        let sample_started = Instant::now();
        let mut client = connect().await;
        assert_eq!(
            client
                .request(Command::Get, native_get_payload(b"short-connection-key"))
                .await,
            ResponsePayload::Hit(b"value".to_vec())
        );
        samples.push(sample_started.elapsed());
    }
    summarize(
        "short_connection_get",
        transport,
        "connect_get_close",
        samples,
        started.elapsed(),
        0,
    )
}

async fn summarize_joined_samples(
    name: &'static str,
    transport: &'static str,
    notes: &'static str,
    started: Instant,
    handles: Vec<tokio::task::JoinHandle<Vec<Duration>>>,
) -> BenchResult {
    let mut samples = Vec::new();
    for handle in handles {
        samples.extend(handle.await.expect("benchmark worker should finish"));
    }
    summarize(name, transport, notes, samples, started.elapsed(), 0)
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

fn measure_process_sync(
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
    summarize(name, "process", notes, samples, started.elapsed(), 0)
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
    request_buffer: Vec<u8>,
    response_buffer: Vec<u8>,
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
                        request_buffer: Vec::new(),
                        response_buffer: Vec::new(),
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
                        request_buffer: Vec::new(),
                        response_buffer: Vec::new(),
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
        let request_id = self.send_request(command, payload).await;
        let response = self.read_response().await;
        assert_eq!(response.request_id, request_id);
        assert_eq!(response.command, command);
        response.payload
    }

    async fn send_request(&mut self, command: Command, payload: RequestPayload) -> u64 {
        let request_id = self.next_request_id;
        self.next_request_id += 1;
        let frame = RequestFrame {
            request_id,
            command,
            payload,
        };
        encode_request_frame_into(&frame, &mut self.request_buffer);
        self.write_all_request_buffer().await;
        request_id
    }

    async fn read_response(&mut self) -> cachebox::protocol::ResponseFrame {
        let mut header = [0u8; HEADER_LEN];
        self.read_exact(&mut header).await;
        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("header payload length")) as usize;
        self.response_buffer.clear();
        self.response_buffer.extend_from_slice(&header);
        let start = self.response_buffer.len();
        self.response_buffer.resize(HEADER_LEN + payload_len, 0);
        self.read_response_payload(start).await;

        decode_response_frame(&self.response_buffer, usize::MAX)
            .expect("native response should decode")
    }

    async fn write_all_request_buffer(&mut self) {
        match &mut self.stream {
            NativeClientStream::Tcp(stream) => stream.write_all(&self.request_buffer).await,
            #[cfg(unix)]
            NativeClientStream::Unix(stream) => stream.write_all(&self.request_buffer).await,
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

    async fn read_response_payload(&mut self, start: usize) {
        match &mut self.stream {
            NativeClientStream::Tcp(stream) => {
                stream.read_exact(&mut self.response_buffer[start..]).await
            }
            #[cfg(unix)]
            NativeClientStream::Unix(stream) => {
                stream.read_exact(&mut self.response_buffer[start..]).await
            }
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
