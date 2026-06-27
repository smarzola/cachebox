//! Typed cache operations.
//!
//! This module maps the HTTP API contract into cache operations before a
//! request reaches the engine. Value bodies stay raw bytes; JSON parsing is
//! limited to small control envelopes for batch and lease operations.

use crate::api::{ApiError, ErrorEnvelope, Method, PutMetadata, Route, StatusCode};
use crate::api::{allowed_methods, parse_put_metadata, parse_route};

pub const LEASE_TOKEN_HEADER: &str = "Cachebox-Lease-Token";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestParts<'a> {
    pub method: Method,
    pub path: &'a str,
    pub headers: Vec<(&'a str, &'a str)>,
    pub body: Vec<u8>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Operation {
    Get {
        namespace: String,
        key: Vec<u8>,
    },
    Put {
        namespace: String,
        key: Vec<u8>,
        value: Vec<u8>,
        metadata: PutMetadata,
    },
    Delete {
        namespace: String,
        key: Vec<u8>,
    },
    BatchGet {
        namespace: String,
        keys: Vec<Vec<u8>>,
    },
    InvalidateTag {
        namespace: String,
        tag: Vec<u8>,
    },
    StartLease {
        namespace: String,
        key: Vec<u8>,
        lease_ttl_ms: u64,
        allow_stale_ms: Option<u64>,
    },
    CompleteLease {
        namespace: String,
        key: Vec<u8>,
        lease_token: String,
        value: Vec<u8>,
        metadata: PutMetadata,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OperationError {
    Api(ApiError),
    MethodNotAllowed {
        method: Method,
        allowed: Vec<Method>,
    },
    InvalidBody {
        message: String,
    },
    MissingHeader {
        header: &'static str,
    },
}

impl OperationError {
    pub fn status_code(&self) -> StatusCode {
        match self {
            Self::Api(error) => error.status_code(),
            Self::MethodNotAllowed { .. } => StatusCode::MethodNotAllowed,
            Self::InvalidBody { .. } | Self::MissingHeader { .. } => StatusCode::BadRequest,
        }
    }

    pub fn envelope(&self) -> ErrorEnvelope {
        match self {
            Self::Api(error) => error.envelope(),
            Self::MethodNotAllowed { method, allowed } => ErrorEnvelope {
                code: "method_not_allowed",
                message: format!("{method:?} is not allowed; expected one of {allowed:?}"),
            },
            Self::InvalidBody { message } => ErrorEnvelope {
                code: "invalid_body",
                message: message.clone(),
            },
            Self::MissingHeader { header } => ErrorEnvelope {
                code: "missing_header",
                message: format!("missing required header {header}"),
            },
        }
    }
}

impl From<ApiError> for OperationError {
    fn from(error: ApiError) -> Self {
        Self::Api(error)
    }
}

pub fn parse_operation(request: RequestParts<'_>) -> Result<Operation, OperationError> {
    let route = parse_route(request.path)?;
    if !allowed_methods(&route).contains(&request.method) {
        return Err(OperationError::MethodNotAllowed {
            method: request.method,
            allowed: allowed_methods(&route).to_vec(),
        });
    }

    match (request.method, route) {
        (Method::Get, Route::Key { namespace, key }) => {
            reject_body(&request.body)?;
            Ok(Operation::Get { namespace, key })
        }
        (Method::Put, Route::Key { namespace, key }) => Ok(Operation::Put {
            namespace,
            key,
            value: request.body,
            metadata: parse_put_metadata(&request.headers)?,
        }),
        (Method::Delete, Route::Key { namespace, key }) => {
            reject_body(&request.body)?;
            Ok(Operation::Delete { namespace, key })
        }
        (Method::Post, Route::BatchGet { namespace }) => Ok(Operation::BatchGet {
            namespace,
            keys: parse_batch_get_body(&request.body)?,
        }),
        (Method::Post, Route::InvalidateTag { namespace, tag }) => {
            reject_body(&request.body)?;
            Ok(Operation::InvalidateTag { namespace, tag })
        }
        (Method::Post, Route::StartLease { namespace, key }) => {
            let body = parse_start_lease_body(&request.body)?;
            Ok(Operation::StartLease {
                namespace,
                key,
                lease_ttl_ms: body.lease_ttl_ms,
                allow_stale_ms: body.allow_stale_ms,
            })
        }
        (Method::Put, Route::CompleteLease { namespace, key }) => {
            let lease_token = find_header(&request.headers, LEASE_TOKEN_HEADER)
                .filter(|value| !value.is_empty())
                .ok_or(OperationError::MissingHeader {
                    header: LEASE_TOKEN_HEADER,
                })?
                .to_string();

            Ok(Operation::CompleteLease {
                namespace,
                key,
                lease_token,
                value: request.body,
                metadata: parse_put_metadata(&request.headers)?,
            })
        }
        (_, Route::Health | Route::Metrics) => Err(OperationError::InvalidBody {
            message: "health and metrics routes are not cache operations".to_string(),
        }),
        _ => unreachable!("method was validated against route before operation mapping"),
    }
}

fn reject_body(body: &[u8]) -> Result<(), OperationError> {
    if body.is_empty() {
        Ok(())
    } else {
        Err(OperationError::InvalidBody {
            message: "request body must be empty for this operation".to_string(),
        })
    }
}

fn find_header<'a>(headers: &'a [(&str, &str)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(header_name, _)| header_name.eq_ignore_ascii_case(name))
        .map(|(_, value)| *value)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct StartLeaseBody {
    lease_ttl_ms: u64,
    allow_stale_ms: Option<u64>,
}

fn parse_batch_get_body(body: &[u8]) -> Result<Vec<Vec<u8>>, OperationError> {
    let body = body_as_str(body)?;
    let keys_value = json_field_value(body, "keys")?;
    let keys = parse_json_string_array(keys_value)?;

    if keys.is_empty() {
        return Err(OperationError::InvalidBody {
            message: "batch get requires at least one key".to_string(),
        });
    }

    keys.into_iter()
        .map(|key| percent_decode(&key, "batch keys must use valid percent encoding"))
        .collect()
}

fn parse_start_lease_body(body: &[u8]) -> Result<StartLeaseBody, OperationError> {
    let body = body_as_str(body)?;
    let lease_ttl_ms = parse_json_u64_field(body, "lease_ttl_ms")?;
    let allow_stale_ms = match json_field_value(body, "allow_stale_ms") {
        Ok(value) => Some(parse_json_u64(value, "allow_stale_ms must be an integer")?),
        Err(OperationError::InvalidBody { message }) if message.contains("missing field") => None,
        Err(error) => return Err(error),
    };

    if lease_ttl_ms == 0 {
        return Err(OperationError::InvalidBody {
            message: "lease_ttl_ms must be greater than zero".to_string(),
        });
    }

    Ok(StartLeaseBody {
        lease_ttl_ms,
        allow_stale_ms,
    })
}

fn body_as_str(body: &[u8]) -> Result<&str, OperationError> {
    std::str::from_utf8(body).map_err(|_| OperationError::InvalidBody {
        message: "control request body must be UTF-8 JSON".to_string(),
    })
}

fn parse_json_u64_field(body: &str, field: &str) -> Result<u64, OperationError> {
    parse_json_u64(
        json_field_value(body, field)?,
        &format!("{field} must be an integer"),
    )
}

fn parse_json_u64(value: &str, message: &str) -> Result<u64, OperationError> {
    let value = value.trim();
    if value.is_empty() || value.starts_with('-') {
        return Err(OperationError::InvalidBody {
            message: message.to_string(),
        });
    }

    value.parse().map_err(|_| OperationError::InvalidBody {
        message: message.to_string(),
    })
}

fn json_field_value<'a>(body: &'a str, field: &str) -> Result<&'a str, OperationError> {
    let body = body.trim();
    if !(body.starts_with('{') && body.ends_with('}')) {
        return Err(OperationError::InvalidBody {
            message: "control request body must be a JSON object".to_string(),
        });
    }

    let field_pattern = format!("\"{field}\"");
    let field_start = body
        .find(&field_pattern)
        .ok_or_else(|| OperationError::InvalidBody {
            message: format!("missing field '{field}'"),
        })?;
    let after_field = &body[field_start + field_pattern.len()..];
    let colon_index = after_field
        .find(':')
        .ok_or_else(|| OperationError::InvalidBody {
            message: format!("missing ':' after field '{field}'"),
        })?;
    let after_colon = after_field[colon_index + 1..].trim_start();

    Ok(read_json_value(after_colon))
}

fn read_json_value(value: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    let mut array_depth = 0usize;

    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'\\' if in_string => escaped = !escaped,
            b'"' if !escaped => in_string = !in_string,
            b'[' if !in_string => array_depth += 1,
            b']' if !in_string => array_depth = array_depth.saturating_sub(1),
            b',' | b'}' if !in_string && array_depth == 0 => return value[..index].trim(),
            _ => escaped = false,
        }
    }

    value.trim()
}

fn parse_json_string_array(value: &str) -> Result<Vec<String>, OperationError> {
    let value = value.trim();
    if !(value.starts_with('[') && value.ends_with(']')) {
        return Err(OperationError::InvalidBody {
            message: "keys must be a JSON string array".to_string(),
        });
    }

    let inner = value[1..value.len() - 1].trim();
    if inner.is_empty() {
        return Ok(Vec::new());
    }

    let mut strings = Vec::new();
    for part in split_top_level_commas(inner) {
        strings.push(parse_json_string(part)?);
    }
    Ok(strings)
}

fn split_top_level_commas(value: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (index, byte) in value.bytes().enumerate() {
        match byte {
            b'\\' if in_string => escaped = !escaped,
            b'"' if !escaped => in_string = !in_string,
            b',' if !in_string => {
                parts.push(value[start..index].trim());
                start = index + 1;
            }
            _ => escaped = false,
        }
    }
    parts.push(value[start..].trim());
    parts
}

fn parse_json_string(value: &str) -> Result<String, OperationError> {
    let value = value.trim();
    if !(value.starts_with('"') && value.ends_with('"')) {
        return Err(OperationError::InvalidBody {
            message: "keys must be JSON strings".to_string(),
        });
    }

    let mut decoded = String::new();
    let mut chars = value[1..value.len() - 1].chars();
    while let Some(ch) = chars.next() {
        if ch == '\\' {
            let escaped = chars.next().ok_or_else(|| OperationError::InvalidBody {
                message: "JSON string escape is incomplete".to_string(),
            })?;
            match escaped {
                '"' | '\\' | '/' => decoded.push(escaped),
                'n' => decoded.push('\n'),
                'r' => decoded.push('\r'),
                't' => decoded.push('\t'),
                _ => {
                    return Err(OperationError::InvalidBody {
                        message: "unsupported JSON string escape".to_string(),
                    });
                }
            }
        } else {
            decoded.push(ch);
        }
    }

    Ok(decoded)
}

fn percent_decode(value: &str, message: &'static str) -> Result<Vec<u8>, OperationError> {
    let mut decoded = Vec::with_capacity(value.len());
    let bytes = value.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] == b'%' {
            let high =
                bytes
                    .get(index + 1)
                    .copied()
                    .ok_or_else(|| OperationError::InvalidBody {
                        message: message.to_string(),
                    })?;
            let low = bytes
                .get(index + 2)
                .copied()
                .ok_or_else(|| OperationError::InvalidBody {
                    message: message.to_string(),
                })?;
            decoded.push((hex_value(high, message)? << 4) | hex_value(low, message)?);
            index += 3;
        } else {
            decoded.push(bytes[index]);
            index += 1;
        }
    }

    Ok(decoded)
}

fn hex_value(byte: u8, message: &'static str) -> Result<u8, OperationError> {
    match byte {
        b'0'..=b'9' => Ok(byte - b'0'),
        b'a'..=b'f' => Ok(byte - b'a' + 10),
        b'A'..=b'F' => Ok(byte - b'A' + 10),
        _ => Err(OperationError::InvalidBody {
            message: message.to_string(),
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::Ttl;

    fn request<'a>(
        method: Method,
        path: &'a str,
        headers: Vec<(&'a str, &'a str)>,
        body: impl Into<Vec<u8>>,
    ) -> RequestParts<'a> {
        RequestParts {
            method,
            path,
            headers,
            body: body.into(),
        }
    }

    #[test]
    fn parses_get_put_and_delete_operations() {
        let get = parse_operation(request(
            Method::Get,
            "/v1/namespaces/default/keys/user%3A1%00",
            vec![],
            [],
        ))
        .expect("get operation");
        assert_eq!(
            get,
            Operation::Get {
                namespace: "default".to_string(),
                key: b"user:1\0".to_vec(),
            }
        );

        let put = parse_operation(request(
            Method::Put,
            "/v1/namespaces/default/keys/blob",
            vec![("Cachebox-TTL", "2s"), ("Cachebox-Tags", "a,b")],
            b"\x00\xffvalue".to_vec(),
        ))
        .expect("put operation");
        assert_eq!(
            put,
            Operation::Put {
                namespace: "default".to_string(),
                key: b"blob".to_vec(),
                value: b"\x00\xffvalue".to_vec(),
                metadata: PutMetadata {
                    ttl: Some(Ttl {
                        milliseconds: 2_000
                    }),
                    stale_ttl: None,
                    tags: vec!["a".to_string(), "b".to_string()],
                    cost: None,
                    content_type: crate::api::ContentType::OctetStream,
                },
            }
        );

        let delete = parse_operation(request(
            Method::Delete,
            "/v1/namespaces/default/keys/blob",
            vec![],
            [],
        ))
        .expect("delete operation");
        assert_eq!(
            delete,
            Operation::Delete {
                namespace: "default".to_string(),
                key: b"blob".to_vec(),
            }
        );
    }

    #[test]
    fn parses_batch_get_operation() {
        let operation = parse_operation(request(
            Method::Post,
            "/v1/namespaces/app/batch/get",
            vec![],
            br#"{"keys":["a","user%3A1","bin%00%FF"]}"#.to_vec(),
        ))
        .expect("batch get operation");

        assert_eq!(
            operation,
            Operation::BatchGet {
                namespace: "app".to_string(),
                keys: vec![b"a".to_vec(), b"user:1".to_vec(), b"bin\0\xff".to_vec()],
            }
        );
    }

    #[test]
    fn parses_tag_invalidation_operation() {
        let operation = parse_operation(request(
            Method::Post,
            "/v1/namespaces/app/tags/org%3A9/invalidate",
            vec![],
            [],
        ))
        .expect("tag invalidation operation");

        assert_eq!(
            operation,
            Operation::InvalidateTag {
                namespace: "app".to_string(),
                tag: b"org:9".to_vec(),
            }
        );
    }

    #[test]
    fn parses_lease_operations() {
        let start = parse_operation(request(
            Method::Post,
            "/v1/namespaces/app/leases/key",
            vec![],
            br#"{"lease_ttl_ms":10000,"allow_stale_ms":60000}"#.to_vec(),
        ))
        .expect("start lease operation");
        assert_eq!(
            start,
            Operation::StartLease {
                namespace: "app".to_string(),
                key: b"key".to_vec(),
                lease_ttl_ms: 10_000,
                allow_stale_ms: Some(60_000),
            }
        );

        let complete = parse_operation(request(
            Method::Put,
            "/v1/namespaces/app/leases/key/complete",
            vec![
                (LEASE_TOKEN_HEADER, "lease-1"),
                ("Cachebox-Stale-TTL", "30s"),
            ],
            b"fresh bytes".to_vec(),
        ))
        .expect("complete lease operation");
        assert_eq!(
            complete,
            Operation::CompleteLease {
                namespace: "app".to_string(),
                key: b"key".to_vec(),
                lease_token: "lease-1".to_string(),
                value: b"fresh bytes".to_vec(),
                metadata: PutMetadata {
                    ttl: None,
                    stale_ttl: Some(Ttl {
                        milliseconds: 30_000
                    }),
                    tags: Vec::new(),
                    cost: None,
                    content_type: crate::api::ContentType::OctetStream,
                },
            }
        );
    }

    #[test]
    fn rejects_unsupported_methods_and_routes() {
        let method_error = parse_operation(request(
            Method::Post,
            "/v1/namespaces/app/keys/key",
            vec![],
            [],
        ))
        .expect_err("post key should fail");
        assert_eq!(method_error.status_code(), StatusCode::MethodNotAllowed);
        assert_eq!(method_error.envelope().code, "method_not_allowed");

        let route_error = parse_operation(request(Method::Get, "/nope", vec![], []))
            .expect_err("bad route should fail");
        assert!(matches!(
            route_error,
            OperationError::Api(ApiError::InvalidPath)
        ));
    }

    #[test]
    fn rejects_unexpected_bodies() {
        let error = parse_operation(request(
            Method::Get,
            "/v1/namespaces/app/keys/key",
            vec![],
            b"body".to_vec(),
        ))
        .expect_err("get with body should fail");

        assert_eq!(
            error.envelope(),
            ErrorEnvelope {
                code: "invalid_body",
                message: "request body must be empty for this operation".to_string(),
            }
        );
    }

    #[test]
    fn rejects_malformed_batch_bodies() {
        let cases = [
            br#"{}"#.as_slice(),
            br#"{"keys":[]}"#.as_slice(),
            br#"{"keys":[1]}"#.as_slice(),
            br#"{"keys":["bad%XX"]}"#.as_slice(),
            b"\xff",
        ];

        for body in cases {
            assert!(
                parse_operation(request(
                    Method::Post,
                    "/v1/namespaces/app/batch/get",
                    vec![],
                    body.to_vec(),
                ))
                .is_err(),
                "{body:?} should fail"
            );
        }
    }

    #[test]
    fn rejects_malformed_lease_requests() {
        let start_cases = [
            br#"{}"#.as_slice(),
            br#"{"lease_ttl_ms":0}"#.as_slice(),
            br#"{"lease_ttl_ms":-1}"#.as_slice(),
            br#"{"lease_ttl_ms":"100"}"#.as_slice(),
        ];

        for body in start_cases {
            assert!(
                parse_operation(request(
                    Method::Post,
                    "/v1/namespaces/app/leases/key",
                    vec![],
                    body.to_vec(),
                ))
                .is_err(),
                "{body:?} should fail"
            );
        }

        let complete_error = parse_operation(request(
            Method::Put,
            "/v1/namespaces/app/leases/key/complete",
            vec![],
            b"value".to_vec(),
        ))
        .expect_err("missing lease token should fail");
        assert_eq!(
            complete_error,
            OperationError::MissingHeader {
                header: LEASE_TOKEN_HEADER
            }
        );
    }
}
