# Official Client Package Boundary Audit

This audit is Milestone 1 for the official client goal loop. It records the
current server, protocol, Rust client, AI helper, benchmark, test, and
documentation boundaries before moving code into separate official client
packages.

Status note: this document intentionally describes the pre-extraction boundary
state. Milestone 2 begins moving protocol code into `crates/cachebox-protocol`;
use the current source tree as authoritative for post-extraction paths.

## Target Behavior

Cachebox should keep the server binary focused on cache execution and native
listener behavior while exposing official clients through package boundaries
that users can depend on directly.

The first extraction should create:

- A server-independent protocol crate.
- A Rust client crate under `clients/rust/`.
- A later Python package under `clients/python/` that wraps the Rust client.

## Current Implementation Gap

The current crate is a single Rust package. It exposes server, engine,
protocol, client, and AI helper modules from the same library root:

- `src/lib.rs` exports `api`, `config`, `engine`, `protocol`, `client`, `ai`,
  and `server`.
- `src/client.rs` already implements a real native client, but imports
  protocol types from `crate::protocol`.
- `src/ai.rs` contains provider-neutral client helper behavior, but it is
  exported as part of the server crate.
- `src/protocol.rs` is transport-independent in design, but imports
  `crate::api::{ContentType, Ttl}`.
- `src/api.rs` currently mixes admin HTTP route constants with protocol-facing
  metadata primitives.

The main dependency knot is small: `ContentType` and `Ttl` need to move out of
`api` before `protocol` can become an independent crate.

## Current Boundaries

### Server And Engine

Files:

- `src/main.rs`
- `src/config.rs`
- `src/server.rs`
- `src/engine.rs`
- `src/api.rs`

Responsibilities:

- Parse configuration and start listeners.
- Run the admin HTTP surface for health and metrics.
- Accept native TCP and Unix socket connections.
- Decode native request frames.
- Execute cache operations against the engine.
- Encode native response frames.
- Own cache memory limits, expiration, tag indexes, leases, metrics, and
  eviction.

Migration impact:

- `src/server.rs` should import protocol types from a protocol crate instead
  of `crate::protocol`.
- `src/engine.rs` currently uses `crate::api::Ttl`; either move `Ttl` into the
  protocol crate and update the engine import, or introduce a small shared
  metadata crate. The protocol crate is the least disruptive first step.
- `src/api.rs` should keep only admin HTTP route constants after `Ttl` and
  `ContentType` move.

### Protocol

File:

- `src/protocol.rs`

Responsibilities:

- Native frame constants and header validation.
- Command IDs.
- Request and response frame types.
- Owned and borrowed request decode paths.
- Response payload view encoding for hot get paths.
- Metadata encoding and decoding.
- Structured error codes.
- Namespace and tag validation.

Current coupling:

- Imports `ContentType` and `Ttl` from `crate::api`.

Proposed boundary:

- Move `src/protocol.rs` to `crates/cachebox-protocol/src/lib.rs`.
- Move `ContentType` and `Ttl` into the protocol crate alongside `Metadata`.
- Keep `Metadata`, `RequestFrame`, `ResponseFrame`, `RequestPayload`,
  `ResponsePayload`, `BatchItem`, `Command`, `ErrorCode`, and `DecodeError`
  public.
- Keep wire bytes unchanged.

Compatibility risks:

- Any docs or examples using `cachebox::api::Ttl` must switch to the protocol
  crate path.
- If the server crate re-exports protocol types for compatibility, make that an
  explicit short-term choice rather than an accidental dependency.
- Golden byte fixtures should be added during extraction to prove the move did
  not alter the protocol.

### Rust Client

File:

- `src/client.rs`

Responsibilities:

- Connect over TCP.
- Connect over Unix domain sockets on Unix platforms.
- Encode request frames and decode response frames.
- Maintain monotonically wrapping request IDs.
- Support sequential requests and pipelined batches.
- Expose get, put, delete, batch get, tag invalidation, lease start, and lease
  completion helpers.
- Map I/O, decode, server, and unexpected response failures into `ClientError`.

Current coupling:

- Imports all native protocol types and codec functions from `crate::protocol`.

Proposed boundary:

- Move `src/client.rs` to `clients/rust/cachebox-client/src/lib.rs` or
  `clients/rust/cachebox-client/src/client.rs`.
- Depend on `cachebox-protocol`.
- Export the official Rust client as a public crate.
- Consider keeping `NativeClient` as the initial public type to minimize churn,
  then add a friendlier `Client` alias or wrapper in the ergonomic API
  milestone.

Compatibility risks:

- Existing examples import `cachebox::client::NativeClient`.
- Existing internal tests import `cachebox::client::{ClientError,
  NativeClient}`.
- `src/bin/cachebox-bench.rs` imports `cachebox::client::NativeClient as
  OfficialNativeClient` and should move to the client crate once it exists.

### AI Helpers

File:

- `src/ai.rs`

Responsibilities:

- Deterministic prompt cache key normalization.
- Deterministic embedding cache key normalization.
- Provider-neutral generation lease decision helpers.
- Generation completion helper types.
- Conservative stream capture helpers.

Current coupling:

- Uses `serde_json::Value`.
- Does not depend on server, engine, or protocol modules.

Proposed boundary:

- Move to `clients/rust/cachebox-client/src/ai.rs`.
- Keep helper behavior provider-neutral and client-side.
- Keep tests with the client crate.
- Later expose the same behavior through Python bindings without duplicating
  normalization logic.

Compatibility risks:

- Docs currently describe these helpers as `cachebox::ai`.
- Internal behavior docs reference the helpers as part of the Rust crate.
- Python bindings should preserve exact normalization output, including JSON
  canonicalization and key prefixes.

### Tests

Files:

- Protocol unit tests in `src/protocol.rs`.
- Client unit tests in `src/client.rs`.
- AI helper unit tests in `src/ai.rs`.
- Server native listener tests in `src/server.rs`.
- Spawned binary smoke tests in `tests/spawned_client.rs`.

Migration impact:

- Protocol unit tests should move with `cachebox-protocol`.
- Client and AI unit tests should move with `cachebox-client`.
- Server listener tests should stay with the server crate and import protocol
  types from `cachebox-protocol`.
- Spawned binary tests should become cross-crate integration tests that use the
  Rust client crate against the real `cachebox` binary.

### Benchmarks

File:

- `src/bin/cachebox-bench.rs`

Current role:

- Benchmarks engine, protocol encode/decode, native socket transport, and the
  current official native client.
- Contains a separate local benchmark client and imports the current library
  client as `OfficialNativeClient`.

Migration impact:

- Keep the benchmark binary in the server package initially.
- Change protocol imports to `cachebox-protocol`.
- Change official client imports to `cachebox-client`.
- Leave the local benchmark client in place so protocol/client performance can
  still be compared.

### Documentation

Public docs currently importing `cachebox::client`, `cachebox::ai`, or
`cachebox::protocol` include:

- `README.md`
- `docs/quickstart.md`
- `docs/usage.md`
- `docs/native-sockets.md`
- `docs/ai-helpers.md`

Internal docs currently referencing these paths include:

- `docs/internal/ai-native-cache.md`
- `docs/internal/supported-behavior.md`
- `docs/internal/native-socket-performance-hardening.md`

Migration impact:

- Update public docs when the new crates exist.
- Prefer examples that import client-facing types from the Rust client crate.
- Keep low-level protocol examples in native socket documentation, but point
  them at the protocol crate.
- Update AI docs to clarify that AI helpers are client-side behavior.

## Least Disruptive Workspace Layout

Use a workspace with the root package remaining the `cachebox` server package:

```text
Cargo.toml
crates/
  cachebox-protocol/
clients/
  rust/
    cachebox-client/
  python/
src/
  main.rs
  server.rs
  engine.rs
  config.rs
  api.rs
```

The root package can remain named `cachebox` during the extraction. This keeps
the binary target, release automation, Docker build, and current server
architecture recognizable while official client crates are added around it.

## Recommended Migration Sequence

1. Add a workspace definition while keeping the root package as a member.
2. Create `crates/cachebox-protocol`.
3. Move `src/protocol.rs` and protocol tests into `cachebox-protocol`.
4. Move `Ttl` and `ContentType` from `src/api.rs` into `cachebox-protocol`.
5. Update server, engine, tests, benchmark, and docs imports for protocol
   types.
6. Add golden encode/decode fixtures before or during the protocol move.
7. Create `clients/rust/cachebox-client`.
8. Move `src/client.rs` and its tests into the Rust client crate.
9. Move `src/ai.rs` and its tests into the Rust client crate.
10. Update spawned tests, benchmark, and public docs to use `cachebox-client`.
11. Add the Python package after the Rust client boundary is stable.

## Next Risk

The first code-moving milestone should be intentionally narrow. Move protocol
and metadata primitives first, prove wire compatibility, and only then move the
client and AI helpers. Moving protocol, client, AI helpers, docs, tests, and
Python bindings in one patch would make regressions hard to isolate.
