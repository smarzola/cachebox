//! HTTP API contract definitions.
//!
//! Cachebox will serve this contract with axum on tokio/Hyper. The route and
//! metadata parsing lives here so it can be tested without opening sockets.

use serde::Serialize;

pub const API_VERSION: &str = "v1";
pub const HEALTH_ROUTE: &str = "/healthz";
pub const METRICS_ROUTE: &str = "/metrics";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    Get,
    Put,
    Delete,
    Post,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Route {
    Health,
    Metrics,
    Key { namespace: String, key: Vec<u8> },
    BatchGet { namespace: String },
    InvalidateTag { namespace: String, tag: Vec<u8> },
    StartLease { namespace: String, key: Vec<u8> },
    CompleteLease { namespace: String, key: Vec<u8> },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PutMetadata {
    pub ttl: Option<Ttl>,
    pub stale_ttl: Option<Ttl>,
    pub tags: Vec<String>,
    pub cost: Option<u64>,
    pub content_type: ContentType,
}

impl Default for PutMetadata {
    fn default() -> Self {
        Self {
            ttl: None,
            stale_ttl: None,
            tags: Vec::new(),
            cost: None,
            content_type: ContentType::OctetStream,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContentType {
    OctetStream,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ttl {
    pub milliseconds: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ErrorEnvelope {
    pub code: &'static str,
    pub message: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusCode {
    Ok,
    Created,
    NoContent,
    BadRequest,
    NotFound,
    MethodNotAllowed,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApiError {
    InvalidPath,
    InvalidNamespace,
    InvalidKeyEncoding,
    InvalidTagEncoding,
    InvalidTtl { header: &'static str, value: String },
    InvalidTags,
    InvalidCost { value: String },
}

impl ApiError {
    pub fn status_code(&self) -> StatusCode {
        StatusCode::BadRequest
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        match self {
            Self::InvalidPath => ErrorEnvelope {
                code: "invalid_path",
                message: "request path is not a supported Cachebox route".to_string(),
            },
            Self::InvalidNamespace => ErrorEnvelope {
                code: "invalid_namespace",
                message: "namespace must contain only ASCII letters, numbers, '-', and '_'"
                    .to_string(),
            },
            Self::InvalidKeyEncoding => ErrorEnvelope {
                code: "invalid_key_encoding",
                message: "key must use valid percent encoding".to_string(),
            },
            Self::InvalidTagEncoding => ErrorEnvelope {
                code: "invalid_tag_encoding",
                message: "tag must use valid percent encoding".to_string(),
            },
            Self::InvalidTtl { header, value } => ErrorEnvelope {
                code: "invalid_ttl",
                message: format!("{header} has invalid TTL value '{value}'"),
            },
            Self::InvalidTags => ErrorEnvelope {
                code: "invalid_tags",
                message: "tags must be comma-separated non-empty ASCII values".to_string(),
            },
            Self::InvalidCost { value } => ErrorEnvelope {
                code: "invalid_cost",
                message: format!("Cachebox-Cost has invalid value '{value}'"),
            },
        }
    }
}

pub fn parse_route(path: &str) -> Result<Route, ApiError> {
    if path == HEALTH_ROUTE {
        return Ok(Route::Health);
    }
    if path == METRICS_ROUTE {
        return Ok(Route::Metrics);
    }

    let parts = split_path(path);
    match parts.as_slice() {
        [version, "namespaces", namespace, "keys", key] if *version == API_VERSION => {
            Ok(Route::Key {
                namespace: parse_namespace(namespace)?,
                key: percent_decode(key).map_err(|_| ApiError::InvalidKeyEncoding)?,
            })
        }
        [version, "namespaces", namespace, "batch", "get"] if *version == API_VERSION => {
            Ok(Route::BatchGet {
                namespace: parse_namespace(namespace)?,
            })
        }
        [version, "namespaces", namespace, "tags", tag, "invalidate"]
            if *version == API_VERSION =>
        {
            Ok(Route::InvalidateTag {
                namespace: parse_namespace(namespace)?,
                tag: percent_decode(tag).map_err(|_| ApiError::InvalidTagEncoding)?,
            })
        }
        [version, "namespaces", namespace, "leases", key] if *version == API_VERSION => {
            Ok(Route::StartLease {
                namespace: parse_namespace(namespace)?,
                key: percent_decode(key).map_err(|_| ApiError::InvalidKeyEncoding)?,
            })
        }
        [version, "namespaces", namespace, "leases", key, "complete"]
            if *version == API_VERSION =>
        {
            Ok(Route::CompleteLease {
                namespace: parse_namespace(namespace)?,
                key: percent_decode(key).map_err(|_| ApiError::InvalidKeyEncoding)?,
            })
        }
        _ => Err(ApiError::InvalidPath),
    }
}

pub fn allowed_methods(route: &Route) -> &'static [Method] {
    match route {
        Route::Health | Route::Metrics => &[Method::Get],
        Route::Key { .. } => &[Method::Get, Method::Put, Method::Delete],
        Route::BatchGet { .. } | Route::InvalidateTag { .. } | Route::StartLease { .. } => {
            &[Method::Post]
        }
        Route::CompleteLease { .. } => &[Method::Put],
    }
}

pub fn parse_put_metadata(headers: &[(&str, &str)]) -> Result<PutMetadata, ApiError> {
    let mut metadata = PutMetadata::default();

    for (name, value) in headers {
        match normalized_header_name(name).as_str() {
            "cachebox-ttl" => {
                metadata.ttl = Some(parse_ttl("Cachebox-TTL", value)?);
            }
            "cachebox-stale-ttl" => {
                metadata.stale_ttl = Some(parse_ttl("Cachebox-Stale-TTL", value)?);
            }
            "cachebox-tags" => {
                metadata.tags = parse_tags(value)?;
            }
            "cachebox-cost" => {
                metadata.cost = Some(value.parse().map_err(|_| ApiError::InvalidCost {
                    value: (*value).to_string(),
                })?);
            }
            "content-type" => {
                metadata.content_type = if value.eq_ignore_ascii_case("application/octet-stream") {
                    ContentType::OctetStream
                } else {
                    ContentType::Other
                };
            }
            _ => {}
        }
    }

    Ok(metadata)
}

fn split_path(path: &str) -> Vec<&str> {
    path.trim_matches('/')
        .split('/')
        .filter(|part| !part.is_empty())
        .collect()
}

fn parse_namespace(namespace: &str) -> Result<String, ApiError> {
    if namespace.is_empty()
        || !namespace
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
    {
        return Err(ApiError::InvalidNamespace);
    }

    Ok(namespace.to_string())
}

fn parse_ttl(header: &'static str, value: &str) -> Result<Ttl, ApiError> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return Err(ApiError::InvalidTtl {
            header,
            value: value.to_string(),
        });
    }

    let (number, multiplier) = if let Some(number) = value.strip_suffix("ms") {
        (number, 1)
    } else if let Some(number) = value.strip_suffix('s') {
        (number, 1_000)
    } else if let Some(number) = value.strip_suffix('m') {
        (number, 60_000)
    } else if let Some(number) = value.strip_suffix('h') {
        (number, 3_600_000)
    } else {
        (value, 1_000)
    };

    let amount: u64 = number.parse().map_err(|_| ApiError::InvalidTtl {
        header,
        value: value.to_string(),
    })?;
    let milliseconds = amount
        .checked_mul(multiplier)
        .ok_or_else(|| ApiError::InvalidTtl {
            header,
            value: value.to_string(),
        })?;

    if milliseconds == 0 {
        return Err(ApiError::InvalidTtl {
            header,
            value: value.to_string(),
        });
    }

    Ok(Ttl { milliseconds })
}

fn parse_tags(value: &str) -> Result<Vec<String>, ApiError> {
    let mut tags = Vec::new();

    for tag in value.split(',') {
        let tag = tag.trim();
        if tag.is_empty()
            || !tag.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b':' | b'.' | b'/')
            })
        {
            return Err(ApiError::InvalidTags);
        }
        tags.push(tag.to_string());
    }

    Ok(tags)
}

fn normalized_header_name(name: &str) -> String {
    name.to_ascii_lowercase()
}

fn percent_decode(value: &str) -> Result<Vec<u8>, ()> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high = bytes.get(index + 1).copied().ok_or(())?;
            let low = bytes.get(index + 2).copied().ok_or(())?;
            decoded.push((hex_value(high)? << 4) | hex_value(low)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    Ok(decoded)
}

fn hex_value(byte: u8) -> Result<u8, ()> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_key_route_with_binary_percent_encoded_key() {
        let route =
            parse_route("/v1/namespaces/default/keys/user%3A123%00%FF").expect("valid key route");

        assert_eq!(
            route,
            Route::Key {
                namespace: "default".to_string(),
                key: b"user:123\0\xff".to_vec()
            }
        );
    }

    #[test]
    fn parses_control_routes() {
        let cases = [
            ("/healthz", Route::Health, vec![Method::Get]),
            ("/metrics", Route::Metrics, vec![Method::Get]),
            (
                "/v1/namespaces/app/batch/get",
                Route::BatchGet {
                    namespace: "app".to_string(),
                },
                vec![Method::Post],
            ),
            (
                "/v1/namespaces/app/tags/org%3A9/invalidate",
                Route::InvalidateTag {
                    namespace: "app".to_string(),
                    tag: b"org:9".to_vec(),
                },
                vec![Method::Post],
            ),
            (
                "/v1/namespaces/app/leases/k%2F1",
                Route::StartLease {
                    namespace: "app".to_string(),
                    key: b"k/1".to_vec(),
                },
                vec![Method::Post],
            ),
            (
                "/v1/namespaces/app/leases/k%2F1/complete",
                Route::CompleteLease {
                    namespace: "app".to_string(),
                    key: b"k/1".to_vec(),
                },
                vec![Method::Put],
            ),
        ];

        for (path, expected_route, expected_methods) in cases {
            let route = parse_route(path).expect("valid route");
            assert_eq!(route, expected_route);
            assert_eq!(allowed_methods(&route), expected_methods.as_slice());
        }
    }

    #[test]
    fn rejects_malformed_routes() {
        let cases = [
            "/v2/namespaces/default/keys/a",
            "/v1/namespaces/default",
            "/v1/namespaces/default/keys/a/extra",
            "/v1/namespaces/default/tags/org%ZZ/invalidate",
        ];

        for path in cases {
            assert!(parse_route(path).is_err(), "{path} should fail");
        }
    }

    #[test]
    fn rejects_invalid_namespaces() {
        let cases = [
            "/v1/namespaces/has.dot/keys/a",
            "/v1/namespaces/has%2Fslash/keys/a",
            "/v1/namespaces//keys/a",
        ];

        for path in cases {
            assert!(matches!(
                parse_route(path),
                Err(ApiError::InvalidPath | ApiError::InvalidNamespace)
            ));
        }
    }

    #[test]
    fn parses_put_metadata() {
        let metadata = parse_put_metadata(&[
            ("Cachebox-TTL", "300s"),
            ("Cachebox-Stale-TTL", "5m"),
            ("Cachebox-Tags", "user:123, org:9"),
            ("Cachebox-Cost", "42"),
            ("Content-Type", "application/octet-stream"),
        ])
        .expect("valid metadata");

        assert_eq!(
            metadata,
            PutMetadata {
                ttl: Some(Ttl {
                    milliseconds: 300_000
                }),
                stale_ttl: Some(Ttl {
                    milliseconds: 300_000
                }),
                tags: vec!["user:123".to_string(), "org:9".to_string()],
                cost: Some(42),
                content_type: ContentType::OctetStream,
            }
        );
    }

    #[test]
    fn parses_ttl_units() {
        let cases = [
            ("1ms", 1),
            ("2s", 2_000),
            ("3m", 180_000),
            ("4h", 14_400_000),
            ("5", 5_000),
        ];

        for (value, milliseconds) in cases {
            let metadata =
                parse_put_metadata(&[("Cachebox-TTL", value)]).expect("valid ttl metadata");
            assert_eq!(metadata.ttl, Some(Ttl { milliseconds }));
        }
    }

    #[test]
    fn rejects_malformed_metadata() {
        let cases = [
            ("Cachebox-TTL", "0"),
            ("Cachebox-TTL", "-1s"),
            ("Cachebox-TTL", "1day"),
            ("Cachebox-Tags", "valid,,invalid"),
            ("Cachebox-Tags", "snowman-☃"),
            ("Cachebox-Cost", "large"),
        ];

        for (header, value) in cases {
            assert!(
                parse_put_metadata(&[(header, value)]).is_err(),
                "{header}: {value} should fail"
            );
        }
    }

    #[test]
    fn error_envelopes_are_deterministic() {
        let error = ApiError::InvalidPath;

        assert_eq!(error.status_code(), StatusCode::BadRequest);
        assert_eq!(
            error.envelope(),
            ErrorEnvelope {
                code: "invalid_path",
                message: "request path is not a supported Cachebox route".to_string()
            }
        );
    }
}
