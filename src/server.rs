//! HTTP server startup and handlers.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::{OriginalUri, State};
use axum::http::header::{CONTENT_TYPE, HeaderMap, HeaderName, HeaderValue};
use axum::http::{Method as HttpMethod, StatusCode as HttpStatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Json, Router, routing::any};
use serde::Serialize;
use tokio::net::TcpListener;

use crate::api::{ErrorEnvelope, Method, StatusCode};
use crate::config::Config;
use crate::engine::{
    CompleteLeaseCommand, CompleteLeaseError, Engine, EngineLimits, GetOutcome, PutCommand,
    PutError, StartLeaseOutcome,
};
use crate::operation::{Operation, OperationError, RequestParts, parse_operation};

#[derive(Debug, Clone)]
pub struct StartupReport {
    pub bind_addr: String,
    pub max_body_bytes: usize,
    pub max_memory_bytes: usize,
    pub max_value_bytes: usize,
}

#[derive(Clone)]
pub struct AppState {
    engine: Arc<Mutex<Engine>>,
    metrics: Arc<Metrics>,
    max_body_bytes: usize,
}

#[derive(Debug, Default)]
struct Metrics {
    requests_total: AtomicU64,
    health_requests: AtomicU64,
    metrics_requests: AtomicU64,
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
            metrics_requests: self.metrics_requests.load(Ordering::Relaxed),
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
    metrics_requests: u64,
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
        max_body_bytes: config.max_body_bytes,
        max_memory_bytes: config.max_memory_bytes,
        max_value_bytes: config.max_value_bytes,
    }
}

pub fn app(config: &Config) -> Router {
    Router::new()
        .fallback(any(handle_request))
        .with_state(AppState {
            engine: Arc::new(Mutex::new(Engine::with_limits(EngineLimits {
                max_memory_bytes: config.max_memory_bytes,
                max_value_bytes: config.max_value_bytes,
            }))),
            metrics: Arc::new(Metrics::default()),
            max_body_bytes: config.max_body_bytes,
        })
}

pub async fn run(config: Config) -> std::io::Result<()> {
    let listener = TcpListener::bind(config.bind_addr).await?;
    let local_addr = listener.local_addr()?;
    println!(
        "event=server_start bind_addr={local_addr} max_body_bytes={} max_memory_bytes={} max_value_bytes={}",
        config.max_body_bytes, config.max_memory_bytes, config.max_value_bytes
    );

    axum::serve(listener, app(&config))
        .with_graceful_shutdown(shutdown_signal())
        .await
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    println!("event=server_shutdown signal=ctrl_c");
}

async fn handle_request(
    State(state): State<AppState>,
    method: HttpMethod,
    OriginalUri(uri): OriginalUri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    state.metrics.requests_total.fetch_add(1, Ordering::Relaxed);

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

    let path = uri.path();
    if method == HttpMethod::GET && path == crate::api::HEALTH_ROUTE {
        state
            .metrics
            .health_requests
            .fetch_add(1, Ordering::Relaxed);
        return (HttpStatusCode::OK, "ok").into_response();
    }
    if method == HttpMethod::GET && path == crate::api::METRICS_ROUTE {
        state
            .metrics
            .metrics_requests
            .fetch_add(1, Ordering::Relaxed);
        return metrics_response(&state);
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

fn metrics_response(state: &AppState) -> Response {
    let snapshot = state.metrics.snapshot();
    let mut engine = state.engine.lock().expect("engine mutex poisoned");
    let engine_stats = engine.stats();
    let memory_used_bytes = engine.memory_used_bytes();
    let limits = engine.limits();

    let body = format!(
        "\
# HELP cachebox_requests_total Total HTTP requests handled.
# TYPE cachebox_requests_total counter
cachebox_requests_total {}
cachebox_requests_health_total {}
cachebox_requests_metrics_total {}
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
cachebox_connections_current 0
",
        snapshot.requests_total,
        snapshot.health_requests,
        snapshot.metrics_requests,
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
        limits.max_memory_bytes
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

    async fn response_bytes(response: Response) -> Vec<u8> {
        response
            .into_body()
            .collect()
            .await
            .expect("body should collect")
            .to_bytes()
            .to_vec()
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

        assert!(body.contains("cachebox_requests_total 7"));
        assert!(body.contains("cachebox_requests_get_total 2"));
        assert!(body.contains("cachebox_requests_put_total 4"));
        assert!(body.contains("cachebox_requests_metrics_total 1"));
        assert!(body.contains("cachebox_cache_hits_total 1"));
        assert!(body.contains("cachebox_cache_misses_total 1"));
        assert!(body.contains("cachebox_errors_total 1"));
        assert!(body.contains("cachebox_evictions_total 1"));
        assert!(body.contains("cachebox_memory_limit_bytes 240"));
        assert!(body.contains("cachebox_connections_current 0"));
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
