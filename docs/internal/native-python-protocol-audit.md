# Native Python Protocol Contract Audit

This is the Milestone 1 audit for `docs/internal/native-python-client-goal-loop.md`.
It records the current protocol contract before replacing the Python package's
default Rust binding path with a native Python implementation.

## Current State

- `docs/protocol.md` is the user-facing native protocol specification.
- `docs/internal/native-socket-protocol.md` is the earlier internal design
  record for the native socket transport.
- `crates/cachebox-protocol` owns the Rust reference types and encode/decode
  implementation.
- `src/server.rs` executes decoded native request payloads and maps engine
  outcomes to protocol responses.
- `tests/spawned_client.rs` verifies the official Rust client against a real
  spawned `cachebox` binary.
- Golden byte coverage exists only as inline Rust unit tests for one `Get`
  request and one `Hit` response. There are no checked-in fixture files that a
  Python or TypeScript test suite can consume.

## Contract Already Clear Enough For Native Python

The public docs and Rust protocol crate agree on these points:

- Frame headers are 24 bytes, big-endian, and start with `CBX1`.
- Protocol version is `1`.
- Request kind is `0x00`; response kind is `0x01`.
- Header flags and reserved fields must be zero.
- Command ids are stable for `Get`, `Put`, `Delete`, `BatchGet`,
  `TagInvalidate`, `LeaseStart`, and `LeaseComplete`.
- Namespace and tag validation rules are documented and implemented.
- Keys and values are byte strings, not UTF-8 strings.
- Metadata encodes TTL, stale TTL, optional cost, tags, and content type.
- `BatchGet` preserves request key order in the response item list.
- Response status bytes distinguish hit, stale, miss, stored, deleted,
  invalidated, lease granted, lease denied, batch, and error responses.
- Structured error codes are numeric and should drive client behavior instead
  of diagnostic messages.
- Pipelined protocol responses may arrive out of order and must be matched by
  `request_id`.

## Important Implementation Details For Python

- The Rust codec rejects unknown command ids, nonzero flags, nonzero reserved
  fields, unsupported versions, invalid bool values, trailing payload bytes,
  invalid UTF-8 strings, invalid namespaces, invalid tags, empty `BatchGet`
  requests, zero lease TTLs, and empty lease completion tokens.
- Response status `0x00` is overloaded: for `BatchGet` it means a batch item
  list; for other commands it means generic `Ok`.
- The server maps malformed frames with a readable header into structured
  protocol errors where possible, but may close the connection when a frame
  cannot be safely synchronized.
- Borrowed decode exists only for hot server-side request shapes: `Get`,
  `Delete`, `TagInvalidate`, and `LeaseStart`. Python does not need to mirror
  this optimization in the first native client.
- The official Rust client returns pipelined results in the caller's request
  order after matching responses by request id. The first Python release will
  not expose a public pipelining API, but its codec should not make future
  request-id matching hard.

## Known Gaps And Ambiguities

- `allow_stale_ms` is encoded, decoded, and documented, but `src/server.rs`
  currently ignores it for both borrowed and owned `LeaseStart` execution
  paths. Native Python dogpile behavior must not depend on this field changing
  server behavior until the server implements it.
- There is no `LeaseAbort` command. If a caller receives a lease and then fails
  while recomputing, the lease remains active until `lease_ttl_ms` expires.
  Python dogpile helpers must document this and choose conservative retry and
  stale fallback behavior.
- Golden frames are not available as standalone files. Inline Rust tests are
  useful for the Rust crate, but not enough for Python and future TypeScript
  conformance suites.
- The docs describe pipelining semantics, but the native Python goal-loop
  consensus is to defer public Python pipelining for the first release.
  Fixture coverage should still include request ids and multiple commands so
  future pipelining can be added safely.
- Source distribution and pure Python package checks cannot rely on the current
  PyO3 package layout; that layout is intentionally replaced in later
  milestones.

## Required Golden Fixture Set

Milestone 2 should add checked-in fixtures that are easy for Rust, Python, and
TypeScript tests to consume. Prefer a data format with explicit hex strings and
semantic metadata, such as JSON files under a protocol fixture directory.

Request fixtures:

- `get_request`: namespace `default`, key `user:123`.
- `put_request_default_metadata`: byte key and value with default metadata.
- `put_request_full_metadata`: TTL, stale TTL, cost, multiple tags, and
  `content_type=other`.
- `delete_request`: existing byte key.
- `batch_get_request`: at least two keys.
- `tag_invalidate_request`: tag containing allowed separators such as
  `user:123/profile`.
- `lease_start_request`: positive lease TTL and absent `allow_stale_ms`.
- `lease_start_request_allow_stale`: positive lease TTL and present
  `allow_stale_ms`, with a note that the current server ignores the field.
- `lease_complete_request`: non-empty lease token, metadata, and value.

Response fixtures:

- `hit_response`: byte value.
- `stale_response`: byte value.
- `miss_response`.
- `stored_response`: nonzero and zero evicted counts should both be covered
  either as separate fixtures or with one representative fixture plus unit
  tests.
- `deleted_response`: removed true and false.
- `invalidated_response`: removed count.
- `batch_get_response`: hit, stale, and miss items in one response.
- `lease_granted_response_empty`: lease token without stale value.
- `lease_granted_response_stale`: lease token with stale value.
- `lease_denied_response`.
- `error_response`: representative structured error, such as
  `InvalidNamespace` with a human diagnostic message.

Malformed or negative fixtures:

- bad magic.
- unsupported version.
- response kind where a request is expected.
- unknown command id.
- nonzero flags.
- nonzero reserved field.
- oversized payload length.
- trailing payload bytes.
- invalid namespace.
- invalid tag.
- invalid boolean.
- empty batch get.
- zero lease TTL.
- empty lease token.

## Fixture Consumption Plan

- Keep Rust as the fixture generator and reference verifier.
- Check fixture files into the repository so non-Rust clients can test without
  compiling Rust.
- Add Rust tests that load every fixture and verify encode/decode behavior.
- Add Python codec tests that load the same fixtures once the native Python
  codec exists.
- Use the same fixture set for a future TypeScript client.
- Treat fixture changes as protocol changes that require deliberate review.

## Recommended Next Milestone

Proceed to Milestone 2 by adding the shared fixture directory and Rust fixture
tests before rewriting the Python package. That gives the native Python codec a
stable compatibility target and reduces the risk of copying protocol mistakes
into multiple language clients.
