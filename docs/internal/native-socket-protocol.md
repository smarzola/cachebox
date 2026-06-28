# Native Socket Protocol Specification

This document is Milestone 1 for the native socket transport goal loop. It
defines the first Cachebox native binary protocol before listener or codec code
is implemented.

The protocol is designed for persistent TCP and Unix domain socket connections.
It replaces the HTTP/2 cache data plane with length-prefixed binary frames while
preserving current cache semantics.

## Version

- Protocol name: Cachebox Native Protocol.
- Protocol version: `1`.
- All multi-byte integers use big-endian byte order.
- Strings are UTF-8 only where the field is explicitly named as a string.
- Keys and values are byte strings and do not require UTF-8.

## Connection Model

- A connection carries a sequence of request frames and response frames.
- Clients may send multiple requests without waiting for earlier responses.
- Every request includes a `request_id`.
- Every response echoes the matching `request_id`.
- Servers process requests concurrently up to an implementation-defined
  per-connection in-flight limit and may respond out of order. Clients must
  match responses by `request_id`.
- A protocol error response is preferred when the frame header is valid and the
  request id is readable.
- If the header is invalid, truncated, oversized, or impossible to resynchronize
  safely, the server closes the connection.

## Frame Header

Every frame starts with a fixed 24-byte header:

| Offset | Size | Field | Type | Description |
|---:|---:|---|---|---|
| 0 | 4 | `magic` | bytes | ASCII `CBX1` |
| 4 | 1 | `version` | `u8` | `1` |
| 5 | 1 | `kind` | `u8` | `0x00` request, `0x01` response |
| 6 | 1 | `command` | `u8` | Command id for requests, echoed for responses |
| 7 | 1 | `flags` | `u8` | Command-specific flags, zero unless specified |
| 8 | 8 | `request_id` | `u64` | Client-selected id echoed in response |
| 16 | 4 | `payload_len` | `u32` | Bytes following this header |
| 20 | 4 | `reserved` | `u32` | Must be zero |

Header validation:

- `magic` must be `CBX1`.
- `version` must be `1`.
- `kind` must be request for client-to-server frames.
- `payload_len` must be no greater than the configured max frame size.
- `reserved` must be zero.
- Unknown nonzero `flags` are rejected unless the command defines them.

## Primitive Encodings

| Type | Encoding |
|---|---|
| `u8` | 1 byte |
| `u16` | 2 bytes, big-endian |
| `u32` | 4 bytes, big-endian |
| `u64` | 8 bytes, big-endian |
| `bytes` | `u32` length followed by that many bytes |
| `string` | `bytes` containing UTF-8 |
| `bool` | `u8`, `0` false, `1` true |

Namespaces are `string` values and must satisfy the current namespace rule:
ASCII letters, numbers, `-`, and `_`.

Tags are `string` values and must satisfy the current tag rule: ASCII letters,
numbers, `-`, `_`, `:`, `.`, and `/`.

## Command IDs

| Command | ID |
|---|---:|
| `GET` | `0x01` |
| `PUT` | `0x02` |
| `DELETE` | `0x03` |
| `BATCH_GET` | `0x04` |
| `TAG_INVALIDATE` | `0x05` |
| `LEASE_START` | `0x06` |
| `LEASE_COMPLETE` | `0x07` |

Command ids `0x80` through `0xff` are reserved for future admin/control
commands and must not be used for cache data-plane operations in version 1.

## Metadata Block

`PUT` and `LEASE_COMPLETE` include a metadata block:

| Field | Type | Description |
|---|---|---|
| `ttl_ms` | `u64` | `0` means no fresh TTL |
| `stale_ttl_ms` | `u64` | `0` means no stale TTL |
| `cost_present` | `bool` | Whether `cost` is set |
| `cost` | `u64` | Valid only when `cost_present` is true |
| `tag_count` | `u16` | Number of following tag strings |
| `tags` | repeated `string` | Tags to attach to the entry |
| `content_type` | `u8` | `0` octet-stream, `1` other |

Rules:

- `ttl_ms == 0` disables TTL.
- `stale_ttl_ms > 0` without a TTL is accepted but ignored by the engine, same
  as the current HTTP metadata behavior.
- `ttl_ms` and `stale_ttl_ms` are millisecond values, not string durations.
- `tag_count` must fit inside the configured max frame size.
- Duplicate tags are accepted initially and may be normalized by clients later.

## Request Payloads

### `GET` Request `0x01`

```text
namespace: string
key: bytes
```

### `PUT` Request `0x02`

```text
namespace: string
key: bytes
metadata: MetadataBlock
value: bytes
```

### `DELETE` Request `0x03`

```text
namespace: string
key: bytes
```

### `BATCH_GET` Request `0x04`

```text
namespace: string
key_count: u32
keys: repeated bytes
```

Rules:

- `key_count` must be greater than zero.
- Response order must match request key order.

### `TAG_INVALIDATE` Request `0x05`

```text
namespace: string
tag: string
```

### `LEASE_START` Request `0x06`

```text
namespace: string
key: bytes
lease_ttl_ms: u64
allow_stale_ms: u64
```

Rules:

- `lease_ttl_ms` must be greater than zero.
- `allow_stale_ms == 0` means absent. Version 1 preserves current behavior:
  the server parses the field but does not use it to alter engine behavior.

### `LEASE_COMPLETE` Request `0x07`

```text
namespace: string
key: bytes
lease_token: string
metadata: MetadataBlock
value: bytes
```

Rules:

- `lease_token` must be non-empty.

## Response Status Codes

Responses use the same frame header with `kind = 0x01`, the request command id,
and the matching `request_id`.

Every response payload starts with:

```text
status: u8
```

| Status | ID | Meaning |
|---|---:|---|
| `OK` | `0x00` | Generic success |
| `HIT` | `0x01` | Fresh value returned |
| `STALE` | `0x02` | Stale value returned |
| `MISS` | `0x03` | Key absent or expired |
| `STORED` | `0x04` | Value stored |
| `DELETED` | `0x05` | Delete completed |
| `INVALIDATED` | `0x06` | Tag invalidation completed |
| `LEASE_GRANTED` | `0x07` | Client should recompute and complete lease |
| `LEASE_DENIED` | `0x08` | Another client owns the lease |
| `ERROR` | `0xff` | Protocol or operation error |

## Response Payloads

### `GET` Response

Hit:

```text
status: HIT
value: bytes
```

Stale:

```text
status: STALE
value: bytes
```

Miss:

```text
status: MISS
```

### `PUT` Response

```text
status: STORED
evicted: u32
```

### `DELETE` Response

```text
status: DELETED
removed: bool
```

### `BATCH_GET` Response

```text
status: OK
item_count: u32
items:
  item_status: u8  // HIT, STALE, or MISS
  value: bytes     // present only for HIT or STALE
```

### `TAG_INVALIDATE` Response

```text
status: INVALIDATED
removed: u32
```

### `LEASE_START` Response

Fresh hit:

```text
status: HIT
value: bytes
```

Stale while another lease is active:

```text
status: STALE
value: bytes
```

Lease granted:

```text
status: LEASE_GRANTED
lease_token: string
has_stale_value: bool
stale_value: bytes // present only when has_stale_value is true
```

Lease denied:

```text
status: LEASE_DENIED
```

### `LEASE_COMPLETE` Response

```text
status: STORED
evicted: u32
```

## Error Payload

All commands may return:

```text
status: ERROR
error_code: u16
message: string
```

Error codes:

| Code | ID | Meaning |
|---|---:|---|
| `BAD_FRAME` | `0x0001` | Malformed frame or payload |
| `UNSUPPORTED_VERSION` | `0x0002` | Protocol version is unsupported |
| `UNKNOWN_COMMAND` | `0x0003` | Command id is not recognized |
| `INVALID_NAMESPACE` | `0x0004` | Namespace validation failed |
| `INVALID_TAG` | `0x0005` | Tag validation failed |
| `INVALID_TTL` | `0x0006` | TTL or stale TTL is invalid |
| `VALUE_TOO_LARGE` | `0x0007` | Value exceeds configured value limit |
| `ENTRY_TOO_LARGE` | `0x0008` | Entry cannot fit configured memory limit |
| `INSUFFICIENT_MEMORY` | `0x0009` | Entry could not fit after cleanup/eviction |
| `INVALID_LEASE_TOKEN` | `0x000a` | Lease completion token is invalid |
| `FRAME_TOO_LARGE` | `0x000b` | Payload exceeds configured frame limit |

Rules:

- If the header is valid enough to read `request_id`, return an `ERROR`
  response when possible.
- If the server cannot trust the frame boundary, close the connection.
- Error messages are for diagnostics. Clients should branch on `error_code`.

## Limits

Native transport must enforce:

- Maximum frame payload length.
- Maximum accepted value size.
- Maximum estimated cache memory.
- Maximum batch key count, derived from frame size unless a stricter config is
  added.
- Maximum tag count, derived from frame size unless a stricter config is added.

The first implementation should reuse existing configured limits for body,
value, and memory:

- `--max-body-bytes` maps to maximum native payload size until renamed.
- `--max-value-bytes` remains the maximum value size.
- `--max-memory-bytes` remains the engine memory cap.

## Example Frames

Pseudo-binary `GET default / key "user:1"`:

```text
Header:
  magic       = "CBX1"
  version     = 1
  kind        = request
  command     = GET
  flags       = 0
  request_id  = 42
  payload_len = 24
  reserved    = 0

Payload:
  namespace_len = 7
  namespace     = "default"
  key_len       = 6
  key           = 75 73 65 72 3a 31
```

Pseudo-binary `HIT` response for request `42` with value `bytes`:

```text
Header:
  magic       = "CBX1"
  version     = 1
  kind        = response
  command     = GET
  flags       = 0
  request_id  = 42
  payload_len = 10
  reserved    = 0

Payload:
  status    = HIT
  value_len = 5
  value     = 62 79 74 65 73
```

## Milestone 1 Checkpoint

This specification defines:

- Header fields and validation.
- Request and response payload layouts for every current cache data-plane
  operation.
- Response states and error codes.
- Pipelining semantics through `request_id`.
- Size-limit and malformed-frame behavior.
- The transitional mapping from existing config limits to native frame limits.

Implementation should proceed with a codec module before opening sockets.
