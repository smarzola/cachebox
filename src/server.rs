//! HTTP server startup and handlers.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::Bytes;
use axum::extract::{OriginalUri, State};
use axum::http::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use axum::http::{Method as HttpMethod, StatusCode as HttpStatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::any};
use serde::Serialize;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::api::{ErrorEnvelope, Method, StatusCode};
use crate::config::Config;
use crate::engine::{
    CompleteLeaseCommand, CompleteLeaseError, Engine, EngineLimits, GetOutcome, PutCommand,
    PutError, StartLeaseOutcome,
};
use crate::operation::{Operation, OperationError, RequestParts, parse_operation};
use crate::protocol::{
    BatchItem, Command, DecodeError, ErrorCode, HEADER_LEN, Metadata as NativeMetadata,
    RequestFrame, RequestPayload, ResponseFrame, ResponsePayload, decode_request_frame,
    encode_response_frame,
};

#[derive(Debug, Clone)]
pub struct StartupReport {
    pub bind_addr: String,
    pub native_bind_addr: Option<String>,
    pub max_body_bytes: usize,
    pub max_memory_bytes: usize,
    pub max_value_bytes: usize,
    pub cleanup_interval_ms: u64,
    pub cleanup_max_entries_per_tick: usize,
}

#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<Engine>>,
    metrics: Arc<Metrics>,
    max_body_bytes: usize,
}

impl AppState {
    fn from_config(config: &Config) -> Self {
        Self {
            engine: Arc::new(Mutex::new(Engine::with_limits(EngineLimits {
                max_memory_bytes: config.max_memory_bytes,
                max_value_bytes: config.max_value_bytes,
            }))),
            metrics: Arc::new(Metrics::default()),
            max_body_bytes: config.max_body_bytes,
        }
    }
}

#[derive(Debug, Default)]
struct Metrics {
    requests_total: AtomicU64,
    health_requests: AtomicU64,
    get_requests: AtomicU64,
    put_requests: AtomicU64,
    delete_requests: AtomicU64,
    batch_get_requests: AtomicU64,
    tag_invalidate_requests: AtomicU64,
    hits_total: AtomicU64,
    misses_total: AtomicU64,
    stale_total: AtomicU64,
    lease_grants: AtomicU64,
    lease_denials: AtomicU64,
    errors_total: AtomicU64,
}

impl Metrics {
    fn snapshot(&self) -> MetricsSnapshot {
        MetricsSnapshot {
            requests_total: self.requests_total.load(Ordering::Relaxed),
            health_requests: self.health_requests.load(Ordering::Relaxed),
            get_requests: self.get_requests.load(Ordering::Relaxed),
            put_requests: self.put_requests.load(Ordering::Relaxed),
            delete_requests: self.delete_requests.load(Ordering::Relaxed),
            batch_get_requests: self.batch_get_requests.load(Ordering::Relaxed),
            tag_invalidate_requests: self.tag_invalidate_requests.load(Ordering::Relaxed),
            hits_total: self.hits_total.load(Ordering::Relaxed),
            misses_total: self.misses_total.load(Ordering::Relaxed),
            stale_total: self.stale_total.load(Ordering::Relaxed),
            lease_grants: self.lease_grants.load(Ordering::Relaxed),
            lease_denials: self.lease_denials.load(Ordering::Relaxed),
            errors_total: self.errors_total.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct MetricsSnapshot {
    requests_total: u64,
    health_requests: u64,
    get_requests: u64,
    put_requests: u64,
    delete_requests: u64,
    batch_get_requests: u64,
    tag_invalidate_requests: u64,
    hits_total: u64,
    misses_total: u64,
    stale_total: u64,
    lease_grants: u64,
    lease_denials: u64,
    errors_total: u64,
}

pub fn startup_report(config: &Config) -> StartupReport {
    StartupReport {
        bind_addr: config.bind_addr.to_string(),
        native_bind_addr: config.native_bind_addr.map(|addr| addr.to_string()),
        max_body_bytes: config.max_body_bytes,
        max_memory_bytes: config.max_memory_bytes,
        max_value_bytes: config.max_value_bytes,
        cleanup_interval_ms: config.cleanup_interval_ms,
        cleanup_max_entries_per_tick: config.cleanup_max_entries_per_tick,
    }
}

pub fn app(config: &Config) -> Router {
    app_with_state(AppState::from_config(config))
}

pub async fn serve_native_tcp(listener: TcpListener, config: &Config) -> std::io::Result<()> {
    run_native_tcp_listener(
        listener,
        AppState::from_config(config),
        config.max_body_bytes,
    )
    .await
}

fn app_with_state(state: AppState) -> Router {
    Router::new()
        .fallback(any(handle_request))
        .with_state(state)
}

pub async fn run(config: Config) -> std::io::Result<()> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    let local_addr = listener.local_addr()?;
    println!(
        "event=server_start bind_addr={local_addr} native_bind_addr={} max_body_bytes={} max_memory_bytes={} max_value_bytes={} cleanup_interval_ms={} cleanup_max_entries_per_tick={}",
        config
            .native_bind_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "disabled".to_string()),
        config.max_body_bytes,
        config.max_memory_bytes,
        config.max_value_bytes,
        config.cleanup_interval_ms,
        config.cleanup_max_entries_per_tick
    );

    let state = AppState::from_config(&config);
    let native_listener = match config.native_bind_addr {
        Some(addr) => Some(TcpListener::bind(addr).await?),
        None => None,
    };
    let native_task = native_listener.map(|listener| {
        tokio::spawn(run_native_tcp_listener(
            listener,
            state.clone(),
            config.max_body_bytes,
        ))
    });
    let cleanup_task = spawn_expiration_worker(
        Arc::clone(&state.engine),
        config.cleanup_interval_ms,
        config.cleanup_max_entries_per_tick,
    );

    let result = axum::serve(listener, app_with_state(state))
        .with_graceful_shutdown(shutdown_signal())
        .await;

    if let Some(cleanup_task) = cleanup_task {
        cleanup_task.abort();
    }
    if let Some(native_task) = native_task {
        native_task.abort();
    }

    result
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("event=server_shutdown signal=ctrl_c");
}

fn spawn_expiration_worker(
    engine: Arc<Mutex<Engine>>,
    interval_ms: u64,
    max_entries_per_tick: usize,
) -> Option<JoinHandle<()>> {
    if interval_ms == 0 {
        return None;
    }

    Some(tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_millis(interval_ms));
        interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            engine
                .lock()
                .expect("engine mutex poisoned")
                .reclaim_expired_budget(max_entries_per_tick);
        }
    }))
}

async fn run_native_tcp_listener(
    listener: TcpListener,
    state: AppState,
    max_payload_len: usize,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            let _ = handle_native_tcp_connection(stream, state, max_payload_len).await;
        });
    }
}

async fn handle_native_tcp_connection(
    mut stream: TcpStream,
    state: AppState,
    max_payload_len: usize,
) -> std::io::Result<()> {
    loop {
        let mut header = [0; HEADER_LEN];
        match stream.read_exact(&mut header).await {
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(()),
            Err(error) => return Err(error),
        }

        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("slice length")) as usize;
        if payload_len > max_payload_len {
            return Ok(());
        }

        let mut frame = Vec::with_capacity(HEADER_LEN + payload_len);
        frame.extend_from_slice(&header);
        let start = frame.len();
        frame.resize(HEADER_LEN + payload_len, 0);
        stream.read_exact(&mut frame[start..]).await?;

        let request = match decode_request_frame(&frame, max_payload_len) {
            Ok(request) => request,
            Err(error) => {
                if let Some(response) = native_decode_error_response(&frame, error) {
                    stream.write_all(&encode_response_frame(&response)).await?;
                }
                return Ok(());
            }
        };
        let response = execute_native_request(&state, request);
        stream.write_all(&encode_response_frame(&response)).await?;
    }
}

async fn handle_request(
    State(state): State<AppState>,
    method: HttpMethod,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let path = uri.path();
    if method == HttpMethod::GET && path == crate::api::METRICS_ROUTE {
        return metrics_response(&state);
    }

    state.metrics.requests_total.fetch_add(1, Ordering::Relaxed);

    if method == HttpMethod::GET && path == crate::api::HEALTH_ROUTE {
        state
            .metrics
            .health_requests
            .fetch_add(1, Ordering::Relaxed);
        return (HttpStatusCode::OK, "ok").into_response();
    }

    if body.len() > state.max_body_bytes {
        state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
        return json_error(
            HttpStatusCode::PAYLOAD_TOO_LARGE,
            ErrorEnvelope {
                code: "body_too_large",
                message: format!("request body exceeds {} byte limit", state.max_body_bytes),
            },
        );
    }

    let Some(method) = convert_method(&method) else {
        state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
        return json_error(
            HttpStatusCode::METHOD_NOT_ALLOWED,
            ErrorEnvelope {
                code: "method_not_allowed",
                message: format!("{} is not supported", method.as_str()),
            },
        );
    };

    let header_pairs = header_pairs(&headers);
    let request = RequestParts {
        method,
        path,
        headers: header_pairs
            .iter()
            .map(|(name, value)| (name.as_str(), value.as_str()))
            .collect(),
        body: body.to_vec(),
    };

    match parse_operation(request) {
        Ok(operation) => execute_operation(state, operation),
        Err(error) => {
            state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
            operation_error_response(error)
        }
    }
}

fn execute_operation(state: AppState, operation: Operation) -> Response {
    match operation {
        Operation::Get { namespace, key } => {
            state.metrics.get_requests.fetch_add(1, Ordering::Relaxed);
            let outcome = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .get(&namespace, &key);
            match &outcome {
                GetOutcome::Hit(_) => state.metrics.hits_total.fetch_add(1, Ordering::Relaxed),
                GetOutcome::Stale(_) => state.metrics.stale_total.fetch_add(1, Ordering::Relaxed),
                GetOutcome::Miss => state.metrics.misses_total.fetch_add(1, Ordering::Relaxed),
            };
            get_response(outcome)
        }
        Operation::Put {
            namespace,
            key,
            value,
            metadata,
        } => {
            state.metrics.put_requests.fetch_add(1, Ordering::Relaxed);
            let result = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .put(PutCommand {
                    namespace,
                    key,
                    value,
                    ttl: metadata.ttl,
                    stale_ttl: metadata.stale_ttl,
                    tags: metadata.tags,
                    cost: metadata.cost,
                });
            match result {
                Ok(_) => HttpStatusCode::CREATED.into_response(),
                Err(error) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    put_error_response(error)
                }
            }
        }
        Operation::Delete { namespace, key } => {
            state
                .metrics
                .delete_requests
                .fetch_add(1, Ordering::Relaxed);
            state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .delete(&namespace, &key);
            HttpStatusCode::NO_CONTENT.into_response()
        }
        Operation::BatchGet { namespace, keys } => {
            state
                .metrics
                .batch_get_requests
                .fetch_add(1, Ordering::Relaxed);
            let outcomes = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .batch_get(&namespace, &keys);
            for outcome in &outcomes {
                match outcome {
                    GetOutcome::Hit(_) => state.metrics.hits_total.fetch_add(1, Ordering::Relaxed),
                    GetOutcome::Stale(_) => {
                        state.metrics.stale_total.fetch_add(1, Ordering::Relaxed)
                    }
                    GetOutcome::Miss => state.metrics.misses_total.fetch_add(1, Ordering::Relaxed),
                };
            }
            Json(BatchGetResponse::from_outcomes(outcomes)).into_response()
        }
        Operation::InvalidateTag { namespace, tag } => {
            state
                .metrics
                .tag_invalidate_requests
                .fetch_add(1, Ordering::Relaxed);
            let Ok(tag) = String::from_utf8(tag) else {
                state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                return json_error(
                    HttpStatusCode::BAD_REQUEST,
                    ErrorEnvelope {
                        code: "invalid_tag",
                        message: "tag invalidation requires a UTF-8 tag".to_string(),
                    },
                );
            };
            let removed = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .invalidate_tag(&namespace, &tag);
            Json(InvalidateTagResponse { removed }).into_response()
        }
        Operation::StartLease {
            namespace,
            key,
            lease_ttl_ms,
            allow_stale_ms: _,
        } => {
            let outcome = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .start_lease(&namespace, &key, lease_ttl_ms);
            match &outcome {
                StartLeaseOutcome::Hit(_) => {
                    state.metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                }
                StartLeaseOutcome::Stale { .. } => {
                    state.metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                }
                StartLeaseOutcome::LeaseGranted { .. } => {
                    state.metrics.lease_grants.fetch_add(1, Ordering::Relaxed);
                }
                StartLeaseOutcome::LeaseDenied => {
                    state.metrics.lease_denials.fetch_add(1, Ordering::Relaxed);
                }
            }
            Json(StartLeaseResponse::from(outcome)).into_response()
        }
        Operation::CompleteLease {
            namespace,
            key,
            lease_token,
            value,
            metadata,
        } => {
            let result = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .complete_lease(CompleteLeaseCommand {
                    namespace,
                    key,
                    lease_token,
                    value,
                    ttl: metadata.ttl,
                    stale_ttl: metadata.stale_ttl,
                    tags: metadata.tags,
                    cost: metadata.cost,
                });
            match result {
                Ok(_) => HttpStatusCode::CREATED.into_response(),
                Err(CompleteLeaseError::InvalidLeaseToken) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    json_error(
                        HttpStatusCode::CONFLICT,
                        ErrorEnvelope {
                            code: "invalid_lease_token",
                            message: "lease token is missing, expired, or no longer active"
                                .to_string(),
                        },
                    )
                }
                Err(CompleteLeaseError::Put(error)) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    put_error_response(error)
                }
            }
        }
    }
}

fn execute_native_request(state: &AppState, frame: RequestFrame) -> ResponseFrame {
    let payload = match frame.payload {
        RequestPayload::Get { namespace, key } => {
            state.metrics.get_requests.fetch_add(1, Ordering::Relaxed);
            match state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .get(&namespace, &key)
            {
                GetOutcome::Hit(value) => {
                    state.metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Hit(value)
                }
                GetOutcome::Stale(value) => {
                    state.metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Stale(value)
                }
                GetOutcome::Miss => {
                    state.metrics.misses_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Miss
                }
            }
        }
        RequestPayload::Put {
            namespace,
            key,
            metadata,
            value,
        } => {
            state.metrics.put_requests.fetch_add(1, Ordering::Relaxed);
            match state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .put(native_put_command(namespace, key, metadata, value))
            {
                Ok(outcome) => ResponsePayload::Stored {
                    evicted: outcome.evicted.min(u32::MAX as usize) as u32,
                },
                Err(error) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    native_put_error(error)
                }
            }
        }
        RequestPayload::Delete { namespace, key } => {
            state
                .metrics
                .delete_requests
                .fetch_add(1, Ordering::Relaxed);
            let removed = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .delete(&namespace, &key);
            ResponsePayload::Deleted { removed }
        }
        RequestPayload::BatchGet { namespace, keys } => {
            state
                .metrics
                .batch_get_requests
                .fetch_add(1, Ordering::Relaxed);
            let outcomes = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .batch_get(&namespace, &keys);
            let mut items = Vec::with_capacity(outcomes.len());
            for outcome in outcomes {
                match outcome {
                    GetOutcome::Hit(value) => {
                        state.metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Hit(value));
                    }
                    GetOutcome::Stale(value) => {
                        state.metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Stale(value));
                    }
                    GetOutcome::Miss => {
                        state.metrics.misses_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Miss);
                    }
                }
            }
            ResponsePayload::BatchGet { items }
        }
        RequestPayload::TagInvalidate { namespace, tag } => {
            state
                .metrics
                .tag_invalidate_requests
                .fetch_add(1, Ordering::Relaxed);
            let removed = state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .invalidate_tag(&namespace, &tag);
            ResponsePayload::Invalidated {
                removed: removed.min(u32::MAX as usize) as u32,
            }
        }
        RequestPayload::LeaseStart {
            namespace,
            key,
            lease_ttl_ms,
            allow_stale_ms: _,
        } => {
            match state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .start_lease(&namespace, &key, lease_ttl_ms)
            {
                StartLeaseOutcome::Hit(value) => {
                    state.metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Hit(value)
                }
                StartLeaseOutcome::Stale { value } => {
                    state.metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Stale(value)
                }
                StartLeaseOutcome::LeaseGranted { token, stale_value } => {
                    state.metrics.lease_grants.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::LeaseGranted {
                        lease_token: token,
                        stale_value,
                    }
                }
                StartLeaseOutcome::LeaseDenied => {
                    state.metrics.lease_denials.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::LeaseDenied
                }
            }
        }
        RequestPayload::LeaseComplete {
            namespace,
            key,
            lease_token,
            metadata,
            value,
        } => {
            let command = CompleteLeaseCommand {
                namespace,
                key,
                lease_token,
                value,
                ttl: metadata.ttl,
                stale_ttl: metadata.stale_ttl,
                tags: metadata.tags,
                cost: metadata.cost,
            };
            match state
                .engine
                .lock()
                .expect("engine mutex poisoned")
                .complete_lease(command)
            {
                Ok(outcome) => ResponsePayload::Stored {
                    evicted: outcome.evicted.min(u32::MAX as usize) as u32,
                },
                Err(CompleteLeaseError::InvalidLeaseToken) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Error {
                        code: ErrorCode::InvalidLeaseToken,
                        message: "lease token is missing, expired, or no longer active".to_string(),
                    }
                }
                Err(CompleteLeaseError::Put(error)) => {
                    state.metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    native_put_error(error)
                }
            }
        }
    };

    ResponseFrame {
        request_id: frame.request_id,
        command: frame.command,
        payload,
    }
}

fn native_put_command(
    namespace: String,
    key: Vec<u8>,
    metadata: NativeMetadata,
    value: Vec<u8>,
) -> PutCommand {
    PutCommand {
        namespace,
        key,
        value,
        ttl: metadata.ttl,
        stale_ttl: metadata.stale_ttl,
        tags: metadata.tags,
        cost: metadata.cost,
    }
}

fn native_put_error(error: PutError) -> ResponsePayload {
    match error {
        PutError::ValueTooLarge { .. } => ResponsePayload::Error {
            code: ErrorCode::ValueTooLarge,
            message: "value exceeds configured value limit".to_string(),
        },
        PutError::ValueTooLargeForMemory { .. } => ResponsePayload::Error {
            code: ErrorCode::EntryTooLarge,
            message: "entry cannot fit configured memory limit".to_string(),
        },
        PutError::InsufficientMemory { .. } => ResponsePayload::Error {
            code: ErrorCode::InsufficientMemory,
            message: "entry could not fit after cleanup and eviction".to_string(),
        },
    }
}

fn native_decode_error_response(frame: &[u8], error: DecodeError) -> Option<ResponseFrame> {
    if frame.len() < HEADER_LEN || frame[0..4] != crate::protocol::MAGIC {
        return None;
    }
    let command = Command::from_id(frame[6]).unwrap_or(Command::Get);
    let request_id = u64::from_be_bytes(frame[8..16].try_into().ok()?);
    Some(ResponseFrame {
        request_id,
        command,
        payload: ResponsePayload::Error {
            code: native_decode_error_code(&error),
            message: format!("{error:?}"),
        },
    })
}

fn native_decode_error_code(error: &DecodeError) -> ErrorCode {
    match error {
        DecodeError::UnsupportedVersion(_) => ErrorCode::UnsupportedVersion,
        DecodeError::UnknownCommand(_) => ErrorCode::UnknownCommand,
        DecodeError::FrameTooLarge { .. } => ErrorCode::FrameTooLarge,
        DecodeError::InvalidNamespace => ErrorCode::InvalidNamespace,
        DecodeError::InvalidTag => ErrorCode::InvalidTag,
        _ => ErrorCode::BadFrame,
    }
}

fn metrics_response(state: &AppState) -> Response {
    let snapshot = state.metrics.snapshot();
    let engine = state.engine.lock().expect("engine mutex poisoned");
    let engine_stats = engine.stats();
    let memory_used_bytes = engine.memory_used_bytes();
    let cost_score_total = engine.cost_score_total();
    let limits = engine.limits();

    let body = format!(
        "\
# HELP cachebox_requests_total Total HTTP requests handled.
# TYPE cachebox_requests_total counter
cachebox_requests_total {}
cachebox_requests_health_total {}
cachebox_requests_get_total {}
cachebox_requests_put_total {}
cachebox_requests_delete_total {}
cachebox_requests_batch_get_total {}
cachebox_requests_tag_invalidate_total {}
# HELP cachebox_cache_hits_total Cache hit outcomes.
# TYPE cachebox_cache_hits_total counter
cachebox_cache_hits_total {}
cachebox_cache_misses_total {}
cachebox_cache_stale_total {}
cachebox_lease_grants_total {}
cachebox_lease_denials_total {}
cachebox_errors_total {}
cachebox_expirations_total {}
cachebox_evictions_total {}
cachebox_memory_used_bytes {}
cachebox_memory_limit_bytes {}
cachebox_cost_score_total {}
cachebox_connections_current 0
",
        snapshot.requests_total,
        snapshot.health_requests,
        snapshot.get_requests,
        snapshot.put_requests,
        snapshot.delete_requests,
        snapshot.batch_get_requests,
        snapshot.tag_invalidate_requests,
        snapshot.hits_total,
        snapshot.misses_total,
        snapshot.stale_total,
        snapshot.lease_grants,
        snapshot.lease_denials,
        snapshot.errors_total,
        engine_stats.expirations,
        engine_stats.evictions,
        memory_used_bytes,
        limits.max_memory_bytes,
        cost_score_total
    );

    (
        HttpStatusCode::OK,
        [(CONTENT_TYPE, "text/plain; version=0.0.4")],
        body,
    )
        .into_response()
}

fn put_error_response(error: PutError) -> Response {
    match error {
        PutError::ValueTooLarge {
            value_bytes,
            max_value_bytes,
        } => json_error(
            HttpStatusCode::PAYLOAD_TOO_LARGE,
            ErrorEnvelope {
                code: "value_too_large",
                message: format!(
                    "value is {value_bytes} bytes, exceeding {max_value_bytes} byte limit"
                ),
            },
        ),
        PutError::ValueTooLargeForMemory {
            entry_bytes,
            max_memory_bytes,
        } => json_error(
            HttpStatusCode::PAYLOAD_TOO_LARGE,
            ErrorEnvelope {
                code: "value_too_large_for_memory",
                message: format!(
                    "entry needs {entry_bytes} bytes, exceeding {max_memory_bytes} byte memory limit"
                ),
            },
        ),
        PutError::InsufficientMemory {
            required_bytes,
            memory_used_bytes,
            max_memory_bytes,
        } => json_error(
            HttpStatusCode::INSUFFICIENT_STORAGE,
            ErrorEnvelope {
                code: "insufficient_memory",
                message: format!(
                    "entry needs {required_bytes} bytes with {memory_used_bytes} of {max_memory_bytes} bytes already used"
                ),
            },
        ),
    }
}

fn get_response(outcome: GetOutcome) -> Response {
    match outcome {
        GetOutcome::Hit(value) => (
            HttpStatusCode::OK,
            [(CONTENT_TYPE, "application/octet-stream")],
            value,
        )
            .into_response(),
        GetOutcome::Stale(value) => {
            let mut response = (
                HttpStatusCode::OK,
                [(CONTENT_TYPE, "application/octet-stream")],
                value,
            )
                .into_response();
            response.headers_mut().insert(
                HeaderName::from_static("cachebox-status"),
                HeaderValue::from_static("stale"),
            );
            response
        }
        GetOutcome::Miss => json_error(
            HttpStatusCode::NOT_FOUND,
            ErrorEnvelope {
                code: "cache_miss",
                message: "cache key was not found".to_string(),
            },
        ),
    }
}

fn operation_error_response(error: OperationError) -> Response {
    json_error(convert_status(error.status_code()), error.envelope())
}

fn json_error(status: HttpStatusCode, envelope: ErrorEnvelope) -> Response {
    (status, Json(envelope)).into_response()
}

fn convert_status(status: StatusCode) -> HttpStatusCode {
    match status {
        StatusCode::Ok => HttpStatusCode::OK,
        StatusCode::Created => HttpStatusCode::CREATED,
        StatusCode::NoContent => HttpStatusCode::NO_CONTENT,
        StatusCode::BadRequest => HttpStatusCode::BAD_REQUEST,
        StatusCode::NotFound => HttpStatusCode::NOT_FOUND,
        StatusCode::MethodNotAllowed => HttpStatusCode::METHOD_NOT_ALLOWED,
    }
}

fn convert_method(method: &HttpMethod) -> Option<Method> {
    match *method {
        HttpMethod::GET => Some(Method::Get),
        HttpMethod::PUT => Some(Method::Put),
        HttpMethod::DELETE => Some(Method::Delete),
        HttpMethod::POST => Some(Method::Post),
        _ => None,
    }
}

fn header_pairs(headers: &HeaderMap) -> Vec<(String, String)> {
    headers
        .iter()
        .filter_map(|(name, value)| {
            value
                .to_str()
                .ok()
                .map(|value| (name.as_str().to_string(), value.to_string()))
        })
        .collect()
}

#[derive(Debug, Clone, Serialize)]
struct BatchGetResponse {
    results: Vec<BatchGetItem>,
}

impl BatchGetResponse {
    fn from_outcomes(outcomes: Vec<GetOutcome>) -> Self {
        Self {
            results: outcomes.into_iter().map(BatchGetItem::from).collect(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct BatchGetItem {
    status: &'static str,
    value: Option<Vec<u8>>,
}

#[derive(Debug, Clone, Serialize)]
struct StartLeaseResponse {
    state: &'static str,
    value: Option<Vec<u8>>,
    lease_token: Option<String>,
    stale_value: Option<Vec<u8>>,
}

impl From<StartLeaseOutcome> for StartLeaseResponse {
    fn from(outcome: StartLeaseOutcome) -> Self {
        match outcome {
            StartLeaseOutcome::Hit(value) => Self {
                state: "hit",
                value: Some(value),
                lease_token: None,
                stale_value: None,
            },
            StartLeaseOutcome::Stale { value } => Self {
                state: "stale",
                value: Some(value),
                lease_token: None,
                stale_value: None,
            },
            StartLeaseOutcome::LeaseGranted { token, stale_value } => Self {
                state: "lease_granted",
                value: None,
                lease_token: Some(token),
                stale_value,
            },
            StartLeaseOutcome::LeaseDenied => Self {
                state: "lease_denied",
                value: None,
                lease_token: None,
                stale_value: None,
            },
        }
    }
}

impl From<GetOutcome> for BatchGetItem {
    fn from(outcome: GetOutcome) -> Self {
        match outcome {
            GetOutcome::Hit(value) => Self {
                status: "hit",
                value: Some(value),
            },
            GetOutcome::Stale(value) => Self {
                status: "stale",
                value: Some(value),
            },
            GetOutcome::Miss => Self {
                status: "miss",
                value: None,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize)]
struct InvalidateTagResponse {
    removed: usize,
}

#[allow(dead_code)]
fn _assert_socket_addr(_: SocketAddr) {}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tower::ServiceExt;

    use super::*;
    use crate::protocol::{
        Metadata, RequestFrame as NativeRequestFrame, ResponseFrame as NativeResponseFrame,
        VERSION, decode_response_frame, encode_request_frame,
    };

    async fn response_bytes(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes()
            .to_vec()
    }

    async fn native_test_client() -> (TcpStream, JoinHandle<std::io::Result<()>>) {
        let state = AppState::from_config(&Config::default());
        let listener = TcpListener::bind("127.0.0.1:0")
            .await
            .expect("native test listener");
        let addr = listener.local_addr().expect("native test listener addr");
        let task = tokio::spawn(run_native_tcp_listener(listener, state, 8192));
        let stream = TcpStream::connect(addr)
            .await
            .expect("native test connection");
        (stream, task)
    }

    async fn native_roundtrip(
        stream: &mut TcpStream,
        request: NativeRequestFrame,
    ) -> NativeResponseFrame {
        stream
            .write_all(&encode_request_frame(&request))
            .await
            .expect("native request write");
        let mut header = [0; HEADER_LEN];
        stream
            .read_exact(&mut header)
            .await
            .expect("native response header");
        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("slice length")) as usize;
        let mut frame = Vec::with_capacity(HEADER_LEN + payload_len);
        frame.extend_from_slice(&header);
        let start = frame.len();
        frame.resize(HEADER_LEN + payload_len, 0);
        stream
            .read_exact(&mut frame[start..])
            .await
            .expect("native response payload");
        decode_response_frame(&frame, 8192).expect("native response decode")
    }

    #[tokio::test]
    async fn healthz_responds_ok() {
        let response = app(&Config::default())
            .oneshot(
                Request::builder()
                    .uri("/healthz")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response_bytes(response).await, b"ok".to_vec());
    }

    #[tokio::test]
    async fn expiration_worker_is_disabled_by_zero_interval() {
        let state = AppState::from_config(&Config::default());

        assert!(spawn_expiration_worker(Arc::clone(&state.engine), 0, 1).is_none());
    }

    #[tokio::test]
    async fn expiration_worker_reclaims_expired_entries_with_budget() {
        let state = AppState::from_config(&Config::default());
        let cleanup_task = spawn_expiration_worker(Arc::clone(&state.engine), 5, 1)
            .expect("cleanup task should start");

        {
            let mut engine = state.engine.lock().expect("engine mutex poisoned");
            for key in [b"a", b"b", b"c"] {
                engine
                    .put(PutCommand {
                        namespace: "default".to_string(),
                        key: key.to_vec(),
                        value: b"value".to_vec(),
                        ttl: Some(crate::api::Ttl { milliseconds: 1 }),
                        stale_ttl: None,
                        tags: Vec::new(),
                        cost: None,
                    })
                    .expect("value should fit");
            }
        }

        tokio::time::sleep(Duration::from_millis(30)).await;
        cleanup_task.abort();

        let engine = state.engine.lock().expect("engine mutex poisoned");
        assert_eq!(engine.len(), 0);
        assert_eq!(engine.stats().expirations, 3);
    }

    #[tokio::test]
    async fn native_tcp_supports_cache_workflow() {
        let (mut stream, task) = native_test_client().await;

        let put = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 1,
                command: Command::Put,
                payload: RequestPayload::Put {
                    namespace: "default".to_string(),
                    key: b"k".to_vec(),
                    metadata: Metadata {
                        ttl: Some(crate::api::Ttl {
                            milliseconds: 60_000,
                        }),
                        stale_ttl: None,
                        cost: Some(7),
                        tags: vec!["group".to_string()],
                        content_type: crate::api::ContentType::OctetStream,
                    },
                    value: b"value".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(put.payload, ResponsePayload::Stored { evicted: 0 });

        let get = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 2,
                command: Command::Get,
                payload: RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"k".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(get.payload, ResponsePayload::Hit(b"value".to_vec()));

        let batch = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 3,
                command: Command::BatchGet,
                payload: RequestPayload::BatchGet {
                    namespace: "default".to_string(),
                    keys: vec![b"k".to_vec(), b"missing".to_vec()],
                },
            },
        )
        .await;
        assert_eq!(
            batch.payload,
            ResponsePayload::BatchGet {
                items: vec![BatchItem::Hit(b"value".to_vec()), BatchItem::Miss]
            }
        );

        let delete_put = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 4,
                command: Command::Put,
                payload: RequestPayload::Put {
                    namespace: "default".to_string(),
                    key: b"delete-me".to_vec(),
                    metadata: Metadata::default(),
                    value: b"temporary".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(delete_put.payload, ResponsePayload::Stored { evicted: 0 });

        let delete = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 5,
                command: Command::Delete,
                payload: RequestPayload::Delete {
                    namespace: "default".to_string(),
                    key: b"delete-me".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(delete.payload, ResponsePayload::Deleted { removed: true });

        let deleted_miss = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 6,
                command: Command::Get,
                payload: RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"delete-me".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(deleted_miss.payload, ResponsePayload::Miss);

        let invalidate = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 7,
                command: Command::TagInvalidate,
                payload: RequestPayload::TagInvalidate {
                    namespace: "default".to_string(),
                    tag: "group".to_string(),
                },
            },
        )
        .await;
        assert_eq!(
            invalidate.payload,
            ResponsePayload::Invalidated { removed: 1 }
        );

        let miss = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 8,
                command: Command::Get,
                payload: RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"k".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(miss.payload, ResponsePayload::Miss);

        task.abort();
    }

    #[tokio::test]
    async fn native_tcp_returns_protocol_error_for_invalid_frame() {
        let (mut stream, task) = native_test_client().await;
        let mut frame = Vec::new();
        frame.extend_from_slice(&crate::protocol::MAGIC);
        frame.push(VERSION);
        frame.push(0);
        frame.push(0xff);
        frame.push(0);
        frame.extend_from_slice(&42u64.to_be_bytes());
        frame.extend_from_slice(&0u32.to_be_bytes());
        frame.extend_from_slice(&0u32.to_be_bytes());

        stream.write_all(&frame).await.expect("invalid frame write");
        let mut header = [0; HEADER_LEN];
        stream
            .read_exact(&mut header)
            .await
            .expect("native error response header");
        let payload_len =
            u32::from_be_bytes(header[16..20].try_into().expect("slice length")) as usize;
        let mut response = Vec::with_capacity(HEADER_LEN + payload_len);
        response.extend_from_slice(&header);
        let start = response.len();
        response.resize(HEADER_LEN + payload_len, 0);
        stream
            .read_exact(&mut response[start..])
            .await
            .expect("native error response payload");

        let response = decode_response_frame(&response, 8192).expect("native error decode");
        assert_eq!(response.request_id, 42);
        assert_eq!(response.command, Command::Get);
        assert!(matches!(
            response.payload,
            ResponsePayload::Error {
                code: ErrorCode::UnknownCommand,
                ..
            }
        ));

        task.abort();
    }

    #[tokio::test]
    async fn native_tcp_supports_lease_workflow() {
        let (mut stream, task) = native_test_client().await;

        let lease = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 1,
                command: Command::LeaseStart,
                payload: RequestPayload::LeaseStart {
                    namespace: "default".to_string(),
                    key: b"lease-key".to_vec(),
                    lease_ttl_ms: 60_000,
                    allow_stale_ms: None,
                },
            },
        )
        .await;
        let token = match lease.payload {
            ResponsePayload::LeaseGranted {
                lease_token,
                stale_value,
            } => {
                assert_eq!(stale_value, None);
                lease_token
            }
            other => panic!("expected lease grant, got {other:?}"),
        };

        let denied = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 2,
                command: Command::LeaseStart,
                payload: RequestPayload::LeaseStart {
                    namespace: "default".to_string(),
                    key: b"lease-key".to_vec(),
                    lease_ttl_ms: 60_000,
                    allow_stale_ms: None,
                },
            },
        )
        .await;
        assert_eq!(denied.payload, ResponsePayload::LeaseDenied);

        let complete = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 3,
                command: Command::LeaseComplete,
                payload: RequestPayload::LeaseComplete {
                    namespace: "default".to_string(),
                    key: b"lease-key".to_vec(),
                    lease_token: token,
                    metadata: Metadata {
                        ttl: Some(crate::api::Ttl {
                            milliseconds: 60_000,
                        }),
                        ..Metadata::default()
                    },
                    value: b"fresh".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(complete.payload, ResponsePayload::Stored { evicted: 0 });

        let hit = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 4,
                command: Command::LeaseStart,
                payload: RequestPayload::LeaseStart {
                    namespace: "default".to_string(),
                    key: b"lease-key".to_vec(),
                    lease_ttl_ms: 60_000,
                    allow_stale_ms: None,
                },
            },
        )
        .await;
        assert_eq!(hit.payload, ResponsePayload::Hit(b"fresh".to_vec()));

        task.abort();
    }

    #[tokio::test]
    async fn put_and_get_raw_bytes() {
        let app = app(&Config::default());

        let put_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/blob")
                    .header("Cachebox-TTL", "60s")
                    .body(Body::from(vec![0, 255, 1, 2]))
                    .expect("request"),
            )
            .await
            .expect("put response");
        assert_eq!(put_response.status(), StatusCode::CREATED);

        let get_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/blob")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("get response");
        assert_eq!(get_response.status(), StatusCode::OK);
        assert_eq!(response_bytes(get_response).await, vec![0, 255, 1, 2]);
    }

    #[tokio::test]
    async fn batch_get_and_tag_invalidation_work_through_http() {
        let app = app(&Config::default());

        for (key, value, tags) in [
            ("a", "one", "group"),
            ("b", "two", "group"),
            ("c", "three", "other"),
        ] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/v1/namespaces/default/keys/{key}"))
                        .header("Cachebox-Tags", tags)
                        .body(Body::from(value.as_bytes().to_vec()))
                        .expect("request"),
                )
                .await
                .expect("put response");
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let batch_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/namespaces/default/batch/get")
                    .body(Body::from(r#"{"keys":["a","missing","c"]}"#))
                    .expect("request"),
            )
            .await
            .expect("batch response");
        assert_eq!(batch_response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&response_bytes(batch_response).await).expect("batch json");
        assert_eq!(body["results"][0]["status"], "hit");
        assert_eq!(body["results"][1]["status"], "miss");
        assert_eq!(body["results"][2]["status"], "hit");

        let invalidate_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/namespaces/default/tags/group/invalidate")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("invalidate response");
        assert_eq!(invalidate_response.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&response_bytes(invalidate_response).await)
                .expect("invalidate json");
        assert_eq!(body["removed"], 2);

        let get_response = app
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/a")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("get response");
        assert_eq!(get_response.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn request_body_limit_is_enforced() {
        let config = Config {
            max_body_bytes: 3,
            ..Config::default()
        };
        let response = app(&config)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/blob")
                    .body(Body::from("four"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn max_value_size_is_enforced_through_http() {
        let config = Config {
            max_value_bytes: 3,
            ..Config::default()
        };
        let response = app(&config)
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/blob")
                    .body(Body::from("four"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let body: serde_json::Value =
            serde_json::from_slice(&response_bytes(response).await).expect("error json");
        assert_eq!(body["code"], "value_too_large");
    }

    #[tokio::test]
    async fn memory_cap_evicts_lru_entry_through_http() {
        let config = Config {
            max_memory_bytes: 240,
            max_value_bytes: 1_000,
            ..Config::default()
        };
        let app = app(&config);

        for (key, value) in [("a", "1111111111"), ("b", "2222222222")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/v1/namespaces/default/keys/{key}"))
                        .body(Body::from(value.as_bytes().to_vec()))
                        .expect("request"),
                )
                .await
                .expect("put response");
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let get_a = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/a")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("get a");
        assert_eq!(get_a.status(), StatusCode::OK);

        let put_c = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/c")
                    .body(Body::from("3333333333"))
                    .expect("request"),
            )
            .await
            .expect("put c");
        assert_eq!(put_c.status(), StatusCode::CREATED);

        let get_b = app
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/b")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("get b");
        assert_eq!(get_b.status(), StatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_request_and_engine_counters() {
        let config = Config {
            max_memory_bytes: 240,
            max_value_bytes: 1_000,
            ..Config::default()
        };
        let app = app(&config);

        for (key, value) in [("a", "1111111111"), ("b", "2222222222")] {
            let response = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method("PUT")
                        .uri(format!("/v1/namespaces/default/keys/{key}"))
                        .body(Body::from(value.as_bytes().to_vec()))
                        .expect("request"),
                )
                .await
                .expect("put response");
            assert_eq!(response.status(), StatusCode::CREATED);
        }

        let hit = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/a")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("hit response");
        assert_eq!(hit.status(), StatusCode::OK);

        let evicting_put = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/c")
                    .body(Body::from("3333333333"))
                    .expect("request"),
            )
            .await
            .expect("evicting put");
        assert_eq!(evicting_put.status(), StatusCode::CREATED);

        let miss = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/b")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("miss response");
        assert_eq!(miss.status(), StatusCode::NOT_FOUND);

        let oversized = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/too-big")
                    .body(Body::from(vec![1; 1_001]))
                    .expect("request"),
            )
            .await
            .expect("oversized response");
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);

        let metrics = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("metrics response");
        assert_eq!(metrics.status(), StatusCode::OK);
        let body = String::from_utf8(response_bytes(metrics).await).expect("metrics utf-8");

        assert!(body.contains("cachebox_requests_total 6"));
        assert!(body.contains("cachebox_requests_get_total 2"));
        assert!(body.contains("cachebox_requests_put_total 4"));
        assert!(!body.contains("cachebox_requests_metrics_total"));
        assert!(body.contains("cachebox_cache_hits_total 1"));
        assert!(body.contains("cachebox_cache_misses_total 1"));
        assert!(body.contains("cachebox_errors_total 1"));
        assert!(body.contains("cachebox_evictions_total 1"));
        assert!(body.contains("cachebox_memory_limit_bytes 240"));
        assert!(body.contains("cachebox_connections_current 0"));

        let second_metrics = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("second metrics response");
        assert_eq!(second_metrics.status(), StatusCode::OK);
        let second_body =
            String::from_utf8(response_bytes(second_metrics).await).expect("metrics utf-8");
        assert!(second_body.contains("cachebox_requests_total 6"));
        assert!(!second_body.contains("cachebox_requests_metrics_total"));
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_accounted_cost_score_total() {
        let app = app(&Config::default());

        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/expensive")
                    .header("Cachebox-Cost", "99")
                    .body(Body::from("value"))
                    .expect("request"),
            )
            .await
            .expect("put response");
        assert_eq!(response.status(), StatusCode::CREATED);

        let metrics = app
            .oneshot(
                Request::builder()
                    .uri("/metrics")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("metrics response");
        assert_eq!(metrics.status(), StatusCode::OK);
        let body = String::from_utf8(response_bytes(metrics).await).expect("metrics utf-8");
        assert!(body.contains("cachebox_cost_score_total 99"));
    }

    #[tokio::test]
    async fn leases_can_be_acquired_denied_and_completed_through_http() {
        let app = app(&Config::default());

        let lease = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/namespaces/default/leases/missing")
                    .body(Body::from(r#"{"lease_ttl_ms":10000}"#))
                    .expect("request"),
            )
            .await
            .expect("lease response");
        assert_eq!(lease.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&response_bytes(lease).await).expect("lease json");
        assert_eq!(body["state"], "lease_granted");
        let token = body["lease_token"]
            .as_str()
            .expect("lease token")
            .to_string();

        let denied = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("POST")
                    .uri("/v1/namespaces/default/leases/missing")
                    .body(Body::from(r#"{"lease_ttl_ms":10000}"#))
                    .expect("request"),
            )
            .await
            .expect("denied response");
        assert_eq!(denied.status(), StatusCode::OK);
        let body: serde_json::Value =
            serde_json::from_slice(&response_bytes(denied).await).expect("denied json");
        assert_eq!(body["state"], "lease_denied");

        let complete = app
            .clone()
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/leases/missing/complete")
                    .header("Cachebox-Lease-Token", token)
                    .header("Cachebox-TTL", "60s")
                    .body(Body::from("fresh"))
                    .expect("request"),
            )
            .await
            .expect("complete response");
        assert_eq!(complete.status(), StatusCode::CREATED);

        let get = app
            .oneshot(
                Request::builder()
                    .uri("/v1/namespaces/default/keys/missing")
                    .body(Body::empty())
                    .expect("request"),
            )
            .await
            .expect("get response");
        assert_eq!(get.status(), StatusCode::OK);
        assert_eq!(response_bytes(get).await, b"fresh".to_vec());
    }
}
