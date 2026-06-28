# Native Protocol Specification

Cachebox native protocol version 1 is a small binary request/response protocol
for persistent TCP and Unix socket connections. It carries cache operations as
length-prefixed frames. The protocol is intentionally narrow: it has byte keys,
byte values, metadata, tags, leases, and explicit hit/stale/miss states.

Clients may have multiple outstanding requests on one connection. Every request
has a `request_id`, and the response repeats that id. Pipelined responses may
arrive out of order, so clients must match by `request_id`.

## Frame Header

Every frame starts with a 24-byte big-endian header:

| Offset | Size | Field | Value |
| --- | ---: | --- | --- |
| 0 | 4 | magic | ASCII `CBX1` |
| 4 | 1 | version | `0x01` |
| 5 | 1 | kind | `0x00` request, `0x01` response |
| 6 | 1 | command | Command id |
| 7 | 1 | flags | `0x00` |
| 8 | 8 | request_id | Unsigned request id |
| 16 | 4 | payload_len | Payload bytes after the header |
| 20 | 4 | reserved | `0x00000000` |

The payload immediately follows the header. `payload_len` is checked against
the server `--max-body-bytes` limit, which defaults to `8388608`.

## Primitive Types

All integer fields are unsigned and big-endian.

| Type | Encoding |
| --- | --- |
| `u8` | 1 byte |
| `u16` | 2 bytes |
| `u32` | 4 bytes |
| `u64` | 8 bytes |
| `bool` | `0x00` false, `0x01` true |
| `bytes` | `u32` length followed by that many bytes |
| `string` | `bytes` containing UTF-8 |
| `ttl_u64` | `0` means none; any non-zero value is milliseconds |
| `option_u64` | `0` means none; any non-zero value is present |

Namespaces and tags are UTF-8 strings. Namespaces must be non-empty and contain
only ASCII letters, ASCII digits, `-`, and `_`. Tags must be non-empty and may
also contain `:`, `.`, and `/`, which makes tags such as `user:123` and
`prompt/template/v2` valid.

## Commands

| Id | Command | Request Payload | Success Response |
| ---: | --- | --- | --- |
| `0x01` | `Get` | `namespace string`, `key bytes` | `Hit`, `Stale`, or `Miss` |
| `0x02` | `Put` | `namespace string`, `key bytes`, `metadata`, `value bytes` | `Stored` |
| `0x03` | `Delete` | `namespace string`, `key bytes` | `Deleted` |
| `0x04` | `BatchGet` | `namespace string`, `u32 key_count`, repeated `key bytes` | `BatchGet` |
| `0x05` | `TagInvalidate` | `namespace string`, `tag string` | `Invalidated` |
| `0x06` | `LeaseStart` | `namespace string`, `key bytes`, `lease_ttl_ms u64`, `allow_stale_ms option_u64` | `Hit`, `Stale`, `LeaseGranted`, or `LeaseDenied` |
| `0x07` | `LeaseComplete` | `namespace string`, `key bytes`, `lease_token string`, `metadata`, `value bytes` | `Stored` |

`BatchGet` must include at least one key. `LeaseStart` requires
`lease_ttl_ms > 0`. `LeaseComplete` requires a non-empty lease token.

## Metadata

`Put` and `LeaseComplete` include metadata:

| Field | Encoding |
| --- | --- |
| `ttl` | `ttl_u64` milliseconds |
| `stale_ttl` | `ttl_u64` milliseconds |
| `has_cost` | `bool` |
| `cost` | `u64`; ignored when `has_cost` is false |
| `tags` | `u16 tag_count`, repeated `tag string` |
| `content_type` | `u8`: `0x00` octet stream, `0x01` other |

TTL fields must be non-zero when present. `stale_ttl` starts after the fresh
TTL window, so a value with `ttl=300000` and `stale_ttl=60000` is fresh for 5
minutes and stale-readable for 1 more minute.

## Response Payloads

Every response payload starts with a status byte:

| Status | Name | Payload |
| ---: | --- | --- |
| `0x00` | `Ok` | No fields |
| `0x01` | `Hit` | `value bytes` |
| `0x02` | `Stale` | `value bytes` |
| `0x03` | `Miss` | No fields |
| `0x04` | `Stored` | `evicted u32` |
| `0x05` | `Deleted` | `removed bool` |
| `0x06` | `Invalidated` | `removed u32` |
| `0x07` | `LeaseGranted` | `lease_token string`, `has_stale bool`, optional `stale_value bytes` |
| `0x08` | `LeaseDenied` | No fields |
| `0xff` | `Error` | `error_code u16`, `message string` |

For `BatchGet`, status `0x00` means a batch response rather than generic `Ok`:

```text
u8  status = 0x00
u32 item_count
repeated item:
  u8 item_status: 0x01 hit, 0x02 stale, 0x03 miss
  bytes value when item_status is hit or stale
```

## Error Codes

| Code | Name | Meaning |
| ---: | --- | --- |
| `0x0001` | `BadFrame` | Malformed frame or invalid payload |
| `0x0002` | `UnsupportedVersion` | Header version is not supported |
| `0x0003` | `UnknownCommand` | Command id is not recognized |
| `0x0004` | `InvalidNamespace` | Namespace failed validation |
| `0x0005` | `InvalidTag` | Tag failed validation |
| `0x0006` | `InvalidTtl` | TTL metadata is invalid |
| `0x0007` | `ValueTooLarge` | Value exceeds `--max-value-bytes` |
| `0x0008` | `EntryTooLarge` | Entry cannot fit the engine limit |
| `0x0009` | `InsufficientMemory` | No live entry can be evicted to fit the write |
| `0x000a` | `InvalidLeaseToken` | Lease completion token does not match the active lease |
| `0x000b` | `FrameTooLarge` | Frame payload exceeds the configured body limit |

Diagnostic messages are for humans. Client logic should branch on `error_code`.

## Ordering And Pipelining

A sequential client may send one frame, wait for the response with the same
`request_id`, and then send the next frame. A pipelined client may write many
frames before reading any responses. The server may execute pipelined frames
concurrently and may write responses as they finish.

Correct clients therefore:

- generate non-zero request ids that are unique among outstanding requests on
  the connection;
- treat response order as independent from request order when pipelining;
- verify the response `command` and `request_id`;
- continue reading all expected responses when one request returns an error, so
  the connection stays aligned.

The official Rust client follows those rules and returns pipelined results in
the order the caller submitted requests.

## Example Get Request

A `Get` request for namespace `default`, key `user:123`, request id `1` has:

```text
header:
  magic       43 42 58 31
  version     01
  kind        00
  command     01
  flags       00
  request_id  00 00 00 00 00 00 00 01
  payload_len 00 00 00 17
  reserved    00 00 00 00

payload:
  namespace length 00 00 00 07
  namespace bytes  64 65 66 61 75 6c 74
  key length       00 00 00 08
  key bytes        75 73 65 72 3a 31 32 33
```
