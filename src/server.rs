//! Admin HTTP server and native socket data-plane handlers.

use std::io;
use std::net::SocketAddr;
use std::ops::Range;
#[cfg(unix)]
use std::os::unix::fs::FileTypeExt;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use axum::extract::{OriginalUri, State};
use axum::http::header::CONTENT_TYPE;
use axum::http::{Method as HttpMethod, StatusCode as HttpStatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Router, routing::any};
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpListener;
#[cfg(unix)]
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Semaphore, mpsc};
use tokio::task::JoinHandle;
use tokio::time::MissedTickBehavior;

use crate::config::Config;
use crate::engine::{
    CompleteLeaseCommand, CompleteLeaseError, EngineLimits, GetOutcome, GetOutcomeRef, PutCommand,
    PutError, ShardedEngine, StartLeaseOutcome,
};
use crate::protocol::{
    BatchItem, Command, DecodeError, ErrorCode, HEADER_LEN, Metadata as NativeMetadata,
    RequestFrame, RequestFrameView, RequestPayload, RequestPayloadView, ResponseFrame,
    ResponsePayload, decode_borrowed_request_frame, decode_request_frame,
    encode_response_frame_into, encode_response_payload_view_frame_into,
};

const MAX_IN_FLIGHT_PER_CONNECTION: usize = 128;
const MAX_RESPONSE_WRITE_BATCH_FRAMES: usize = 32;
const MAX_RESPONSE_WRITE_BATCH_BYTES: usize = 64 * 1024;
const METRIC_SHARD_COUNT: usize = 64;
static NEXT_NATIVE_CONNECTION_ID: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Clone)]
pub struct StartupReport {
    pub admin_bind_addr: String,
    pub native_bind_addr: Option<String>,
    pub native_unix_socket: Option<String>,
    pub max_body_bytes: usize,
    pub max_memory_bytes: usize,
    pub max_value_bytes: usize,
    pub cleanup_interval_ms: u64,
    pub cleanup_max_entries_per_tick: usize,
}

#[derive(Clone)]
pub struct AppState {
    engine: Arc<ShardedEngine>,
    metrics: Arc<Metrics>,
}

impl AppState {
    fn from_config(config: &Config) -> Self {
        Self {
            engine: Arc::new(ShardedEngine::with_limits(EngineLimits {
                max_memory_bytes: config.max_memory_bytes,
                max_value_bytes: config.max_value_bytes,
            })),
            metrics: Arc::new(Metrics::default()),
        }
    }
}

struct Metrics {
    shards: Vec<MetricsShard>,
}

impl Metrics {
    fn shard_for_request(&self, connection_id: u64, request_id: u64) -> &MetricsShard {
        let mixed = connection_id ^ request_id.rotate_left(17);
        &self.shards[(mixed as usize) % self.shards.len()]
    }

    fn admin_shard(&self) -> &MetricsShard {
        &self.shards[0]
    }

    fn snapshot(&self) -> MetricsSnapshot {
        self.shards
            .iter()
            .map(MetricsShard::snapshot)
            .fold(MetricsSnapshot::default(), MetricsSnapshot::saturating_add)
    }
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            shards: (0..METRIC_SHARD_COUNT)
                .map(|_| MetricsShard::default())
                .collect(),
        }
    }
}

#[derive(Debug, Default)]
struct MetricsShard {
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

impl MetricsShard {
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

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
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

impl MetricsSnapshot {
    fn saturating_add(mut self, other: Self) -> Self {
        self.requests_total = self.requests_total.saturating_add(other.requests_total);
        self.health_requests = self.health_requests.saturating_add(other.health_requests);
        self.get_requests = self.get_requests.saturating_add(other.get_requests);
        self.put_requests = self.put_requests.saturating_add(other.put_requests);
        self.delete_requests = self.delete_requests.saturating_add(other.delete_requests);
        self.batch_get_requests = self
            .batch_get_requests
            .saturating_add(other.batch_get_requests);
        self.tag_invalidate_requests = self
            .tag_invalidate_requests
            .saturating_add(other.tag_invalidate_requests);
        self.hits_total = self.hits_total.saturating_add(other.hits_total);
        self.misses_total = self.misses_total.saturating_add(other.misses_total);
        self.stale_total = self.stale_total.saturating_add(other.stale_total);
        self.lease_grants = self.lease_grants.saturating_add(other.lease_grants);
        self.lease_denials = self.lease_denials.saturating_add(other.lease_denials);
        self.errors_total = self.errors_total.saturating_add(other.errors_total);
        self
    }
}

pub fn startup_report(config: &Config) -> StartupReport {
    StartupReport {
        admin_bind_addr: config.bind_addr.to_string(),
        native_bind_addr: config.native_bind_addr.map(|addr| addr.to_string()),
        native_unix_socket: config
            .native_unix_socket
            .as_ref()
            .map(|path| path.display().to_string()),
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

#[cfg(unix)]
pub async fn serve_native_unix(listener: UnixListener, config: &Config) -> std::io::Result<()> {
    run_native_unix_listener(
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
        "event=server_start admin_bind_addr={local_addr} native_bind_addr={} native_unix_socket={} max_body_bytes={} max_memory_bytes={} max_value_bytes={} cleanup_interval_ms={} cleanup_max_entries_per_tick={}",
        config
            .native_bind_addr
            .map(|addr| addr.to_string())
            .unwrap_or_else(|| "disabled".to_string()),
        config
            .native_unix_socket
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "disabled".to_string()),
        config.max_body_bytes,
        config.max_memory_bytes,
        config.max_value_bytes,
        config.cleanup_interval_ms,
        config.cleanup_max_entries_per_tick
    );

    #[cfg(not(unix))]
    if config.native_unix_socket.is_some() {
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native Unix sockets are only supported on Unix platforms",
        ));
    }

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
    #[cfg(unix)]
    let native_unix_socket = config.native_unix_socket.clone();
    #[cfg(unix)]
    let native_unix_listener = match native_unix_socket.as_deref() {
        Some(path) => Some(bind_native_unix_listener(path).await?),
        None => None,
    };
    #[cfg(unix)]
    let native_unix_task = native_unix_listener.map(|listener| {
        tokio::spawn(run_native_unix_listener(
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
    #[cfg(unix)]
    if let Some(native_unix_task) = native_unix_task {
        native_unix_task.abort();
    }
    #[cfg(unix)]
    if let Some(path) = native_unix_socket {
        let _ = std::fs::remove_file(path);
    }

    result
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("event=server_shutdown signal=ctrl_c");
}

fn spawn_expiration_worker(
    engine: Arc<ShardedEngine>,
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
            engine.reclaim_expired_budget(max_entries_per_tick);
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
        stream.set_nodelay(true)?;
        let state = state.clone();
        tokio::spawn(async move {
            let _ = handle_native_connection(stream, state, max_payload_len).await;
        });
    }
}

#[cfg(unix)]
async fn run_native_unix_listener(
    listener: UnixListener,
    state: AppState,
    max_payload_len: usize,
) -> std::io::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            let _ = handle_native_connection(stream, state, max_payload_len).await;
        });
    }
}

#[cfg(unix)]
async fn bind_native_unix_listener(path: &Path) -> std::io::Result<UnixListener> {
    if path.exists() {
        match UnixStream::connect(path).await {
            Ok(_) => {
                return Err(io::Error::new(
                    io::ErrorKind::AddrInUse,
                    format!("native Unix socket is already active: {}", path.display()),
                ));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(_) => {
                let metadata = std::fs::metadata(path)?;
                if !metadata.file_type().is_socket() {
                    return Err(io::Error::new(
                        io::ErrorKind::AlreadyExists,
                        format!(
                            "native Unix socket path is not a socket: {}",
                            path.display()
                        ),
                    ));
                }
                std::fs::remove_file(path)?;
            }
        }
    }
    UnixListener::bind(path)
}

async fn handle_native_connection<S>(
    stream: S,
    state: AppState,
    max_payload_len: usize,
) -> std::io::Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin + Send + 'static,
{
    let connection_id = NEXT_NATIVE_CONNECTION_ID.fetch_add(1, Ordering::Relaxed);
    let (mut reader, writer) = tokio::io::split(stream);
    let mut writer = Some(writer);
    let mut response_tx = None;
    let mut writer_task = None;
    let in_flight = Arc::new(Semaphore::new(MAX_IN_FLIGHT_PER_CONNECTION));
    let mut read_buffer = Vec::with_capacity(HEADER_LEN + 1024);
    let mut response_buffer = Vec::with_capacity(HEADER_LEN + 1024);
    let mut pipelined = false;

    while let Some(frame_ranges) =
        read_native_frame_ranges(&mut reader, &mut read_buffer, max_payload_len).await?
    {
        if frame_ranges.len() > 1 {
            pipelined = true;
        }

        if !pipelined {
            for frame_range in &frame_ranges {
                response_buffer.clear();
                execute_native_frame_bytes_into(
                    &state,
                    &read_buffer[frame_range.clone()],
                    max_payload_len,
                    connection_id,
                    &mut response_buffer,
                );
                if let Some(writer) = writer.as_mut() {
                    writer.write_all(&response_buffer).await?;
                }
            }
            drain_frame_ranges(&mut read_buffer, &frame_ranges);
            continue;
        }

        if response_tx.is_none() {
            let (tx, rx) = mpsc::channel::<Vec<u8>>(MAX_IN_FLIGHT_PER_CONNECTION);
            let writer = writer
                .take()
                .expect("native writer should exist before pipelined mode");
            writer_task = Some(spawn_native_writer_task(writer, rx));
            response_tx = Some(tx);
        }

        let frames: Vec<Vec<u8>> = frame_ranges
            .iter()
            .map(|range| read_buffer[range.clone()].to_vec())
            .collect();
        drain_frame_ranges(&mut read_buffer, &frame_ranges);

        for frame in frames {
            let permit = Arc::clone(&in_flight)
                .acquire_owned()
                .await
                .expect("connection semaphore should stay open");
            let state = state.clone();
            let response_tx = response_tx
                .as_ref()
                .expect("pipelined response channel should be initialized")
                .clone();
            tokio::spawn(async move {
                let mut response = Vec::new();
                execute_native_frame_bytes_into(
                    &state,
                    &frame,
                    max_payload_len,
                    connection_id,
                    &mut response,
                );
                let _ = response_tx.send(response).await;
                drop(permit);
            });
        }
    }

    drop(response_tx);
    if let Some(writer_task) = writer_task {
        writer_task
            .await
            .map_err(|error| io::Error::other(format!("native writer task failed: {error}")))?
    } else {
        Ok(())
    }
}

fn spawn_native_writer_task<W>(
    mut writer: W,
    mut response_rx: mpsc::Receiver<Vec<u8>>,
) -> JoinHandle<std::io::Result<()>>
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    tokio::spawn(async move {
        while let Some(response) = response_rx.recv().await {
            let response = coalesce_native_responses(response, &mut response_rx);
            writer.write_all(&response).await?;
        }
        std::io::Result::Ok(())
    })
}

fn coalesce_native_responses(first: Vec<u8>, response_rx: &mut mpsc::Receiver<Vec<u8>>) -> Vec<u8> {
    let mut batch = first;
    let mut frames = 1;
    while frames < MAX_RESPONSE_WRITE_BATCH_FRAMES && batch.len() < MAX_RESPONSE_WRITE_BATCH_BYTES {
        match response_rx.try_recv() {
            Ok(response) => {
                let exceeds_batch_bytes =
                    batch.len().saturating_add(response.len()) > MAX_RESPONSE_WRITE_BATCH_BYTES;
                batch.extend_from_slice(&response);
                frames += 1;
                if exceeds_batch_bytes {
                    break;
                }
            }
            Err(_) => break,
        }
    }
    batch
}

async fn read_native_frame_ranges<R>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
    max_payload_len: usize,
) -> std::io::Result<Option<Vec<Range<usize>>>>
where
    R: AsyncRead + Unpin,
{
    loop {
        match find_complete_native_frames(buffer, max_payload_len) {
            FrameDrain::Frames(frames) => return Ok(Some(frames)),
            FrameDrain::Close => return Ok(None),
            FrameDrain::NeedMore => {}
        }

        let read = reader.read_buf(buffer).await?;
        if read == 0 {
            return Ok(None);
        }
    }
}

enum FrameDrain {
    Frames(Vec<Range<usize>>),
    NeedMore,
    Close,
}

fn find_complete_native_frames(buffer: &[u8], max_payload_len: usize) -> FrameDrain {
    let mut frames = Vec::new();
    let mut offset = 0;
    while buffer.len().saturating_sub(offset) >= HEADER_LEN {
        let payload_len = u32::from_be_bytes(
            buffer[offset + 16..offset + 20]
                .try_into()
                .expect("slice length"),
        ) as usize;
        if payload_len > max_payload_len {
            return FrameDrain::Close;
        }

        let frame_len = HEADER_LEN + payload_len;
        if buffer.len().saturating_sub(offset) < frame_len {
            break;
        }
        frames.push(offset..offset + frame_len);
        offset += frame_len;
    }

    if frames.is_empty() {
        FrameDrain::NeedMore
    } else {
        FrameDrain::Frames(frames)
    }
}

fn drain_frame_ranges(buffer: &mut Vec<u8>, ranges: &[Range<usize>]) {
    if let Some(last) = ranges.last() {
        buffer.drain(..last.end);
    }
}

fn execute_native_frame_bytes_into(
    state: &AppState,
    frame: &[u8],
    max_payload_len: usize,
    connection_id: u64,
    response_buffer: &mut Vec<u8>,
) {
    match decode_borrowed_request_frame(frame, max_payload_len) {
        Ok(Some(request)) => {
            let metrics = state
                .metrics
                .shard_for_request(connection_id, request.request_id);
            if let RequestPayloadView::Get { namespace, key } = request.payload {
                encode_native_get_response_frame(
                    state,
                    metrics,
                    request.request_id,
                    request.command,
                    namespace,
                    key,
                    response_buffer,
                );
            } else {
                let response = execute_native_request_view(state, metrics, request);
                encode_response_frame_into(&response, response_buffer);
            }
        }
        Ok(None) => match decode_request_frame(frame, max_payload_len) {
            Ok(request) => {
                let metrics = state
                    .metrics
                    .shard_for_request(connection_id, request.request_id);
                let response = execute_native_request(state, metrics, request);
                encode_response_frame_into(&response, response_buffer);
            }
            Err(error) => {
                if let Some(response) = native_decode_error_response(frame, error) {
                    encode_response_frame_into(&response, response_buffer);
                }
            }
        },
        Err(error) => {
            if let Some(response) = native_decode_error_response(frame, error) {
                encode_response_frame_into(&response, response_buffer);
            }
        }
    }
}

async fn handle_request(
    State(state): State<AppState>,
    method: HttpMethod,
    OriginalUri(uri): OriginalUri,
) -> Response {
    let path = uri.path();
    if method == HttpMethod::GET && path == crate::api::METRICS_ROUTE {
        return metrics_response(&state);
    }

    if method == HttpMethod::GET && path == crate::api::HEALTH_ROUTE {
        let metrics = state.metrics.admin_shard();
        metrics.requests_total.fetch_add(1, Ordering::Relaxed);
        metrics.health_requests.fetch_add(1, Ordering::Relaxed);
        return (HttpStatusCode::OK, "ok").into_response();
    }

    let metrics = state.metrics.admin_shard();
    metrics.requests_total.fetch_add(1, Ordering::Relaxed);
    metrics.errors_total.fetch_add(1, Ordering::Relaxed);
    (
        HttpStatusCode::NOT_FOUND,
        "HTTP is admin-only; use the native socket protocol for cache operations",
    )
        .into_response()
}

fn execute_native_request(
    state: &AppState,
    metrics: &MetricsShard,
    frame: RequestFrame,
) -> ResponseFrame {
    ResponseFrame {
        request_id: frame.request_id,
        command: frame.command,
        payload: execute_native_payload_with_metrics(state, metrics, frame.payload),
    }
}

fn execute_native_request_view(
    state: &AppState,
    metrics: &MetricsShard,
    frame: RequestFrameView<'_>,
) -> ResponseFrame {
    ResponseFrame {
        request_id: frame.request_id,
        command: frame.command,
        payload: execute_native_payload_view(state, metrics, frame.payload),
    }
}

fn execute_native_payload_view(
    state: &AppState,
    metrics: &MetricsShard,
    payload: RequestPayloadView<'_>,
) -> ResponsePayload {
    match payload {
        RequestPayloadView::Get { namespace, key } => {
            execute_native_get(state, metrics, namespace, key)
        }
        RequestPayloadView::Delete { namespace, key } => {
            metrics.delete_requests.fetch_add(1, Ordering::Relaxed);
            let removed = state.engine.delete(namespace, key);
            ResponsePayload::Deleted { removed }
        }
        RequestPayloadView::TagInvalidate { namespace, tag } => {
            metrics
                .tag_invalidate_requests
                .fetch_add(1, Ordering::Relaxed);
            let removed = state.engine.invalidate_tag(namespace, tag);
            ResponsePayload::Invalidated {
                removed: removed.min(u32::MAX as usize) as u32,
            }
        }
        RequestPayloadView::LeaseStart {
            namespace,
            key,
            lease_ttl_ms,
            allow_stale_ms: _,
        } => execute_native_lease_start(state, metrics, namespace, key, lease_ttl_ms),
    }
}

#[cfg(test)]
fn execute_native_payload(state: &AppState, payload: RequestPayload) -> ResponsePayload {
    let metrics = state.metrics.admin_shard();
    execute_native_payload_with_metrics(state, metrics, payload)
}

fn execute_native_payload_with_metrics(
    state: &AppState,
    metrics: &MetricsShard,
    payload: RequestPayload,
) -> ResponsePayload {
    match payload {
        RequestPayload::Get { namespace, key } => {
            execute_native_get(state, metrics, &namespace, &key)
        }
        RequestPayload::Put {
            namespace,
            key,
            metadata,
            value,
        } => {
            metrics.put_requests.fetch_add(1, Ordering::Relaxed);
            match state
                .engine
                .put(native_put_command(namespace, key, metadata, value))
            {
                Ok(outcome) => ResponsePayload::Stored {
                    evicted: outcome.evicted.min(u32::MAX as usize) as u32,
                },
                Err(error) => {
                    metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    native_put_error(error)
                }
            }
        }
        RequestPayload::Delete { namespace, key } => {
            metrics.delete_requests.fetch_add(1, Ordering::Relaxed);
            let removed = state.engine.delete(&namespace, &key);
            ResponsePayload::Deleted { removed }
        }
        RequestPayload::BatchGet { namespace, keys } => {
            metrics.batch_get_requests.fetch_add(1, Ordering::Relaxed);
            let outcomes = state.engine.batch_get(&namespace, &keys);
            let mut items = Vec::with_capacity(outcomes.len());
            for outcome in outcomes {
                match outcome {
                    GetOutcome::Hit(value) => {
                        metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Hit(value));
                    }
                    GetOutcome::Stale(value) => {
                        metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Stale(value));
                    }
                    GetOutcome::Miss => {
                        metrics.misses_total.fetch_add(1, Ordering::Relaxed);
                        items.push(BatchItem::Miss);
                    }
                }
            }
            ResponsePayload::BatchGet { items }
        }
        RequestPayload::TagInvalidate { namespace, tag } => {
            metrics
                .tag_invalidate_requests
                .fetch_add(1, Ordering::Relaxed);
            let removed = state.engine.invalidate_tag(&namespace, &tag);
            ResponsePayload::Invalidated {
                removed: removed.min(u32::MAX as usize) as u32,
            }
        }
        RequestPayload::LeaseStart {
            namespace,
            key,
            lease_ttl_ms,
            allow_stale_ms: _,
        } => execute_native_lease_start(state, metrics, &namespace, &key, lease_ttl_ms),
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
            match state.engine.complete_lease(command) {
                Ok(outcome) => ResponsePayload::Stored {
                    evicted: outcome.evicted.min(u32::MAX as usize) as u32,
                },
                Err(CompleteLeaseError::InvalidLeaseToken) => {
                    metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    ResponsePayload::Error {
                        code: ErrorCode::InvalidLeaseToken,
                        message: "lease token is missing, expired, or no longer active".to_string(),
                    }
                }
                Err(CompleteLeaseError::Put(error)) => {
                    metrics.errors_total.fetch_add(1, Ordering::Relaxed);
                    native_put_error(error)
                }
            }
        }
    }
}

fn execute_native_get(
    state: &AppState,
    metrics: &MetricsShard,
    namespace: &str,
    key: &[u8],
) -> ResponsePayload {
    metrics.get_requests.fetch_add(1, Ordering::Relaxed);
    match state.engine.get(namespace, key) {
        GetOutcome::Hit(value) => {
            metrics.hits_total.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::Hit(value)
        }
        GetOutcome::Stale(value) => {
            metrics.stale_total.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::Stale(value)
        }
        GetOutcome::Miss => {
            metrics.misses_total.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::Miss
        }
    }
}

fn encode_native_get_response_frame(
    state: &AppState,
    metrics: &MetricsShard,
    request_id: u64,
    command: Command,
    namespace: &str,
    key: &[u8],
    out: &mut Vec<u8>,
) {
    metrics.get_requests.fetch_add(1, Ordering::Relaxed);
    state
        .engine
        .get_ref(namespace, key, |outcome| match outcome {
            GetOutcomeRef::Hit(value) => {
                metrics.hits_total.fetch_add(1, Ordering::Relaxed);
                encode_response_payload_view_frame_into(
                    request_id,
                    command,
                    crate::protocol::ResponsePayloadView::Hit(value),
                    out,
                );
            }
            GetOutcomeRef::Stale(value) => {
                metrics.stale_total.fetch_add(1, Ordering::Relaxed);
                encode_response_payload_view_frame_into(
                    request_id,
                    command,
                    crate::protocol::ResponsePayloadView::Stale(value),
                    out,
                );
            }
            GetOutcomeRef::Miss => {
                metrics.misses_total.fetch_add(1, Ordering::Relaxed);
                encode_response_payload_view_frame_into(
                    request_id,
                    command,
                    crate::protocol::ResponsePayloadView::Miss,
                    out,
                );
            }
        });
}

fn execute_native_lease_start(
    state: &AppState,
    metrics: &MetricsShard,
    namespace: &str,
    key: &[u8],
    lease_ttl_ms: u64,
) -> ResponsePayload {
    match state.engine.start_lease(namespace, key, lease_ttl_ms) {
        StartLeaseOutcome::Hit(value) => {
            metrics.hits_total.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::Hit(value)
        }
        StartLeaseOutcome::Stale { value } => {
            metrics.stale_total.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::Stale(value)
        }
        StartLeaseOutcome::LeaseGranted { token, stale_value } => {
            metrics.lease_grants.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::LeaseGranted {
                lease_token: token,
                stale_value,
            }
        }
        StartLeaseOutcome::LeaseDenied => {
            metrics.lease_denials.fetch_add(1, Ordering::Relaxed);
            ResponsePayload::LeaseDenied
        }
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
    let engine_stats = state.engine.stats();
    let memory_used_bytes = state.engine.memory_used_bytes();
    let cost_score_total = state.engine.cost_score_total();
    let limits = state.engine.limits();

    let body = format!(
        "\
# HELP cachebox_requests_total Total admin HTTP requests handled.
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

#[allow(dead_code)]
fn _assert_socket_addr(_: SocketAddr) {}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use http_body_util::BodyExt;
    use tokio::net::TcpStream;
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

    #[cfg(unix)]
    fn native_unix_test_path(name: &str) -> PathBuf {
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "cachebox-{name}-{}-{unique}.sock",
            std::process::id()
        ))
    }

    #[cfg(unix)]
    async fn native_unix_test_client() -> (UnixStream, JoinHandle<std::io::Result<()>>, PathBuf) {
        let state = AppState::from_config(&Config::default());
        let path = native_unix_test_path("native-unix");
        let listener = bind_native_unix_listener(&path)
            .await
            .expect("native unix test listener");
        let task = tokio::spawn(run_native_unix_listener(listener, state, 8192));
        let stream = UnixStream::connect(&path)
            .await
            .expect("native unix test connection");
        (stream, task, path)
    }

    async fn native_roundtrip<S>(stream: &mut S, request: NativeRequestFrame) -> NativeResponseFrame
    where
        S: AsyncRead + AsyncWrite + Unpin,
    {
        write_native_request(stream, request).await;
        read_native_response(stream).await
    }

    async fn write_native_request<S>(stream: &mut S, request: NativeRequestFrame)
    where
        S: AsyncWrite + Unpin,
    {
        stream
            .write_all(&encode_request_frame(&request))
            .await
            .expect("native request write");
    }

    async fn read_native_response<S>(stream: &mut S) -> NativeResponseFrame
    where
        S: AsyncRead + Unpin,
    {
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

        for key in [b"a", b"b", b"c"] {
            state
                .engine
                .put(PutCommand {
                    namespace: "default".to_string(),
                    key: key.to_vec(),
                    value: b"value".to_vec(),
                    ttl: Some(cachebox_protocol::Ttl { milliseconds: 1 }),
                    stale_ttl: None,
                    tags: Vec::new(),
                    cost: None,
                })
                .expect("value should fit");
        }

        tokio::time::sleep(Duration::from_millis(30)).await;
        cleanup_task.abort();

        assert_eq!(state.engine.len(), 0);
        assert_eq!(state.engine.stats().expirations, 3);
    }

    #[test]
    fn metrics_snapshot_aggregates_request_shards() {
        let metrics = Metrics::default();
        metrics
            .shard_for_request(1, 1)
            .get_requests
            .fetch_add(1, Ordering::Relaxed);
        metrics
            .shard_for_request(2, 1)
            .hits_total
            .fetch_add(1, Ordering::Relaxed);
        metrics
            .admin_shard()
            .requests_total
            .fetch_add(1, Ordering::Relaxed);

        let snapshot = metrics.snapshot();
        assert_eq!(snapshot.requests_total, 1);
        assert_eq!(snapshot.get_requests, 1);
        assert_eq!(snapshot.hits_total, 1);
    }

    #[test]
    fn coalesce_native_responses_drains_queued_frames_in_order() {
        let (tx, mut rx) = mpsc::channel(4);
        tx.try_send(vec![3, 4]).expect("second response");
        tx.try_send(vec![5]).expect("third response");
        drop(tx);

        assert_eq!(
            coalesce_native_responses(vec![1, 2], &mut rx),
            vec![1, 2, 3, 4, 5]
        );
    }

    #[test]
    fn find_complete_native_frames_returns_all_buffered_ranges() {
        let first = encode_request_frame(&NativeRequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"a".to_vec(),
            },
        });
        let second = encode_request_frame(&NativeRequestFrame {
            request_id: 2,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"b".to_vec(),
            },
        });
        let mut buffer = [first.as_slice(), second.as_slice()].concat();

        let FrameDrain::Frames(frames) = find_complete_native_frames(&buffer, 8192) else {
            panic!("expected complete frames");
        };

        assert_eq!(frames, vec![0..first.len(), first.len()..buffer.len()]);
        assert_eq!(&buffer[frames[0].clone()], first.as_slice());
        assert_eq!(&buffer[frames[1].clone()], second.as_slice());
        drain_frame_ranges(&mut buffer, &frames);
        assert!(buffer.is_empty());
    }

    #[test]
    fn find_complete_native_frames_keeps_partial_frame_buffered() {
        let frame = encode_request_frame(&NativeRequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"a".to_vec(),
            },
        });
        let split = frame.len() - 3;
        let buffer = frame[..split].to_vec();

        assert!(matches!(
            find_complete_native_frames(&buffer, 8192),
            FrameDrain::NeedMore
        ));
        assert_eq!(buffer, frame[..split]);
    }

    #[test]
    fn find_complete_native_frames_closes_on_oversized_payload() {
        let mut buffer = encode_request_frame(&NativeRequestFrame {
            request_id: 1,
            command: Command::Get,
            payload: RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"a".to_vec(),
            },
        });
        buffer[16..20].copy_from_slice(&9000_u32.to_be_bytes());

        assert!(matches!(
            find_complete_native_frames(&buffer, 8192),
            FrameDrain::Close
        ));
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
                        ttl: Some(cachebox_protocol::Ttl {
                            milliseconds: 60_000,
                        }),
                        stale_ttl: None,
                        cost: Some(7),
                        tags: vec!["group".to_string()],
                        content_type: cachebox_protocol::ContentType::OctetStream,
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
    async fn native_tcp_supports_pipelined_requests_on_one_connection() {
        let (mut stream, task) = native_test_client().await;

        let requests = [
            NativeRequestFrame {
                request_id: 101,
                command: Command::Put,
                payload: RequestPayload::Put {
                    namespace: "default".to_string(),
                    key: b"pipelined".to_vec(),
                    metadata: Metadata::default(),
                    value: b"value".to_vec(),
                },
            },
            NativeRequestFrame {
                request_id: 102,
                command: Command::Get,
                payload: RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"pipelined".to_vec(),
                },
            },
            NativeRequestFrame {
                request_id: 103,
                command: Command::Get,
                payload: RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"missing".to_vec(),
                },
            },
        ];

        for request in requests {
            write_native_request(&mut stream, request).await;
        }

        let mut responses = Vec::new();
        for _ in 0..3 {
            responses.push(read_native_response(&mut stream).await);
        }

        let response = |request_id| {
            responses
                .iter()
                .find(|response| response.request_id == request_id)
                .expect("response id")
        };
        assert_eq!(
            response(101).payload,
            ResponsePayload::Stored { evicted: 0 }
        );
        assert_eq!(response(101).command, Command::Put);
        assert!(
            matches!(
                response(102).payload,
                ResponsePayload::Hit(_) | ResponsePayload::Miss
            ),
            "pipelined get can race with pipelined put and must be matched by request id"
        );
        assert_eq!(response(102).command, Command::Get);
        assert_eq!(response(103).payload, ResponsePayload::Miss);
        assert_eq!(response(103).command, Command::Get);

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

    #[test]
    fn native_execution_uses_decoded_payloads_without_http_request_accounting() {
        let state = AppState::from_config(&Config::default());

        let stored = execute_native_payload(
            &state,
            RequestPayload::Put {
                namespace: "default".to_string(),
                key: b"direct-key".to_vec(),
                metadata: Metadata {
                    tags: vec!["direct-tag".to_string()],
                    ..Metadata::default()
                },
                value: b"direct-value".to_vec(),
            },
        );
        assert_eq!(stored, ResponsePayload::Stored { evicted: 0 });

        let hit = execute_native_payload(
            &state,
            RequestPayload::Get {
                namespace: "default".to_string(),
                key: b"direct-key".to_vec(),
            },
        );
        assert_eq!(hit, ResponsePayload::Hit(b"direct-value".to_vec()));

        let batch = execute_native_payload(
            &state,
            RequestPayload::BatchGet {
                namespace: "default".to_string(),
                keys: vec![b"direct-key".to_vec(), b"missing".to_vec()],
            },
        );
        assert_eq!(
            batch,
            ResponsePayload::BatchGet {
                items: vec![BatchItem::Hit(b"direct-value".to_vec()), BatchItem::Miss]
            }
        );

        let invalidated = execute_native_payload(
            &state,
            RequestPayload::TagInvalidate {
                namespace: "default".to_string(),
                tag: "direct-tag".to_string(),
            },
        );
        assert_eq!(invalidated, ResponsePayload::Invalidated { removed: 1 });

        let lease = execute_native_payload(
            &state,
            RequestPayload::LeaseStart {
                namespace: "default".to_string(),
                key: b"lease-direct".to_vec(),
                lease_ttl_ms: 60_000,
                allow_stale_ms: None,
            },
        );
        let token = match lease {
            ResponsePayload::LeaseGranted { lease_token, .. } => lease_token,
            other => panic!("expected native lease grant, got {other:?}"),
        };

        let complete = execute_native_payload(
            &state,
            RequestPayload::LeaseComplete {
                namespace: "default".to_string(),
                key: b"lease-direct".to_vec(),
                lease_token: token,
                metadata: Metadata::default(),
                value: b"lease-value".to_vec(),
            },
        );
        assert_eq!(complete, ResponsePayload::Stored { evicted: 0 });

        let lease_hit = execute_native_payload(
            &state,
            RequestPayload::LeaseStart {
                namespace: "default".to_string(),
                key: b"lease-direct".to_vec(),
                lease_ttl_ms: 60_000,
                allow_stale_ms: None,
            },
        );
        assert_eq!(lease_hit, ResponsePayload::Hit(b"lease-value".to_vec()));

        let snapshot = state.metrics.snapshot();
        assert_eq!(snapshot.requests_total, 0);
        assert_eq!(snapshot.put_requests, 1);
        assert_eq!(snapshot.get_requests, 1);
        assert_eq!(snapshot.batch_get_requests, 1);
        assert_eq!(snapshot.tag_invalidate_requests, 1);
        assert_eq!(snapshot.hits_total, 3);
        assert_eq!(snapshot.misses_total, 1);
        assert_eq!(snapshot.lease_grants, 1);
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
                        ttl: Some(cachebox_protocol::Ttl {
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

    #[cfg(unix)]
    #[tokio::test]
    async fn native_unix_socket_supports_cache_workflow() {
        let (mut stream, task, path) = native_unix_test_client().await;

        let put = native_roundtrip(
            &mut stream,
            NativeRequestFrame {
                request_id: 1,
                command: Command::Put,
                payload: RequestPayload::Put {
                    namespace: "default".to_string(),
                    key: b"unix-key".to_vec(),
                    metadata: Metadata::default(),
                    value: b"unix-value".to_vec(),
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
                    key: b"unix-key".to_vec(),
                },
            },
        )
        .await;
        assert_eq!(get.payload, ResponsePayload::Hit(b"unix-value".to_vec()));

        task.abort();
        let _ = std::fs::remove_file(path);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn native_unix_bind_removes_stale_socket_file() {
        let path = native_unix_test_path("stale");
        let stale_listener = UnixListener::bind(&path).expect("stale listener bind");
        drop(stale_listener);
        assert!(path.exists());

        let listener = bind_native_unix_listener(&path)
            .await
            .expect("stale socket file should be replaced");
        drop(listener);
        assert!(path.exists());
        let _ = std::fs::remove_file(path);
    }

    #[tokio::test]
    async fn admin_http_rejects_cache_routes() {
        let response = app(&Config::default())
            .oneshot(
                Request::builder()
                    .method("PUT")
                    .uri("/v1/namespaces/default/keys/blob")
                    .body(Body::from("value"))
                    .expect("request"),
            )
            .await
            .expect("response");

        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(
            response_bytes(response).await,
            b"HTTP is admin-only; use the native socket protocol for cache operations".to_vec()
        );
    }

    #[tokio::test]
    async fn metrics_endpoint_reports_native_request_and_engine_counters() {
        let state = AppState::from_config(&Config::default());
        assert_eq!(
            execute_native_payload(
                &state,
                RequestPayload::Put {
                    namespace: "default".to_string(),
                    key: b"expensive".to_vec(),
                    metadata: Metadata {
                        cost: Some(99),
                        ..Metadata::default()
                    },
                    value: b"value".to_vec(),
                },
            ),
            ResponsePayload::Stored { evicted: 0 }
        );
        assert_eq!(
            execute_native_payload(
                &state,
                RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"expensive".to_vec(),
                },
            ),
            ResponsePayload::Hit(b"value".to_vec())
        );
        assert_eq!(
            execute_native_payload(
                &state,
                RequestPayload::Get {
                    namespace: "default".to_string(),
                    key: b"missing".to_vec(),
                },
            ),
            ResponsePayload::Miss
        );

        let metrics = app_with_state(state)
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

        assert!(body.contains("cachebox_requests_total 0"));
        assert!(body.contains("cachebox_requests_get_total 2"));
        assert!(body.contains("cachebox_requests_put_total 1"));
        assert!(body.contains("cachebox_cache_hits_total 1"));
        assert!(body.contains("cachebox_cache_misses_total 1"));
        assert!(body.contains("cachebox_cost_score_total 99"));
    }
}
