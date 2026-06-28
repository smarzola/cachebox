# Cachebox Native Protocol v1 Fixtures

`frames.json` contains shared byte fixtures for the Cachebox native protocol.
The fixtures are checked in so official clients can verify protocol
compatibility without compiling the Rust reference implementation.

Each fixture has:

- `name`: stable fixture id.
- `direction`: `request`, `response`, `malformed_request`, or
  `malformed_response`.
- `command` and `request_id` for valid request and response fixtures.
- `hex`: the full frame bytes, including the 24-byte header.
- `summary`: human-readable semantic fields for valid fixtures.
- `expected_error` for malformed fixtures.

Client conformance tests should:

1. Decode every valid request and response fixture.
2. Re-encode valid fixtures and compare the exact `hex` bytes.
3. Reject malformed fixtures with the named error class or the closest
   language-specific equivalent.
4. Treat fixture changes as protocol contract changes.

The first native Python client should load this same file for codec tests.
Future TypeScript or other official clients should do the same.
