# Native Python Client Goal Loop Prompt

Use this prompt to pivot Cachebox's Python client toward a native Python
implementation while keeping the Rust client as the official Rust client and
reference implementation. The goal is to make Cachebox feel like a real Python
caching library, not only a protocol driver, while preserving byte-level
compatibility across future official clients such as TypeScript.

## Role

You are designing and implementing Cachebox's official Python client surface.
Treat Python runtime integration, decorators, serialization, async behavior,
gevent compatibility, stampede protection, and package distribution as first
class client concerns.

Use Rust for the server, protocol reference implementation, conformance fixture
generation, and official Rust client. Do not require Rust bindings as the
default Python runtime dependency unless a later milestone proves that native
Python cannot satisfy the client requirements.

## Target State

Cachebox should provide official clients through:

- A shared protocol specification with golden request and response frame
  fixtures that every official client must pass.
- A Rust protocol crate and Rust client crate that remain the reference
  implementation for native protocol behavior.
- A native Python package under `clients/python/` that implements the wire
  protocol directly in Python.
- A synchronous Python client that works naturally with normal Python sockets
  and gevent monkey-patched sockets.
- An asynchronous Python client that uses `asyncio` without blocking the event
  loop.
- Optional synchronous and asynchronous connection pools for applications with
  concurrent threads, greenlets, coroutines, or repeated short operations.
- A high-level Python caching API with `Cachebox`, `AsyncCachebox`,
  `get_or_set`, decorators, key builders, serializers, stale handling, and
  dogpile protection backed by Cachebox leases.
- No public Python pipelining API in the first native-client release. Keep the
  internals compatible with future request ID matching and pipelining, but rely
  on batching, pooling, and high-level caching behavior first.
- A future-compatible client contract that TypeScript and other official
  clients can implement without depending on Rust native extensions.
- Python packaging that can publish pure Python wheels and source
  distributions without requiring a Rust toolchain for normal installation.

This loop continues the existing official-client branch and pull request. Do
not open a replacement PR for this pivot unless the maintainer explicitly asks
for a separate PR; update the current branch and PR as milestones land.

## Current Consensus Decisions

- The default Python client should be native Python, not Rust bindings. Rust
  remains the protocol reference, server implementation, fixture authority, and
  official Rust client, but the Python package should install and run without a
  Rust toolchain.
- Native Python is the better default for `asyncio`, gevent-monkey-patched
  socket usage, source distributions, universal wheels, and future TypeScript
  parity. Since a TypeScript client will need to reimplement the protocol
  anyway, protocol fixtures and conformance tests are the right shared
  foundation across clients.
- The low-level Python layer must be a complete driver, but the public client
  should not stop at driver primitives. The expected Python user experience is
  an opinionated caching library with decorators, serialization, key building,
  invalidation helpers, stale behavior, and dogpile protection.
- Optional connection pooling is first-class for both sync and async clients.
  A single client connection may remain the simplest default, but applications
  with concurrent threads, greenlets, coroutines, or repeated short operations
  need a documented pool with max size, acquisition timeout, close behavior,
  error handling, and cancellation behavior for async callers.
- Do not expose a public Python pipelining API in the first native-client
  release. Pipelining adds ordering, request matching, partial failure,
  timeout, cancellation, backpressure, reconnect, and gevent interaction
  questions that are not required for a strong caching-library surface.
- Keep protocol and driver internals compatible with future request ID
  matching. Revisit pipelining only after batching, pooling, decorators,
  serialization, dogpile protection, and async support are stable.
- Dogpile protection is foundational, not a nice-to-have. High-level
  `get_or_set` and decorator flows should use Cachebox leases so concurrent
  misses do not stampede the origin, and lease denial should lead to waiting,
  stale return, or another documented policy rather than duplicate recompute by
  default.
- Stale behavior must be explicit. The high-level API should distinguish
  normal hits, stale reads, lease-granted refreshes, lease-denied waits,
  wrapped-function failures, serialization failures, connection failures, and
  server errors.
- Keep working on the existing `feat/official-client-foundation` branch and
  existing official-client pull request. Do not create a new branch or PR for
  this pivot unless the maintainer explicitly requests it.

## Design Principles

- Prefer runtime-native clients for language ecosystem fit.
- Keep Rust as the protocol reference, not as a mandatory runtime dependency
  for every language.
- Use golden frames and conformance tests to control protocol drift.
- Keep the Python package pure Python unless a measured hot path justifies an
  optional accelerator later.
- Make sync, async, and gevent behavior explicit in tests and docs.
- Treat connection pooling as a first-class optional capability for sync and
  async clients.
- Defer public pipelining until after batching, pooling, decorators, and
  dogpile protection are stable.
- Provide low-level driver primitives, but make the primary user experience an
  opinionated caching library.
- Make dogpile protection easy to use and difficult to misuse.
- Keep cache keys deterministic and inspectable.
- Make serialization explicit and extensible.
- Preserve byte keys and byte values at the low-level protocol boundary.
- Expose leases without making application code hand-roll stampede control.
- Document failure behavior for cache server errors, connection failures,
  lease denials, stale reads, serialization errors, and wrapped function
  exceptions.
- Avoid behavior that is not backed by server semantics.
- Keep release metadata managed by automation.

## Operating Loop

Continue working on the existing official-client feature branch and existing
official-client PR. Keep the branch rebased or merged with `main` as needed,
but do not start a new branch or open a new PR for this goal loop unless the
maintainer explicitly requests it.

For every milestone:

1. Restate the target behavior and the current implementation gap.
2. Inspect current code, tests, docs, and packaging before editing.
3. Make the smallest coherent change that advances the native Python target.
4. Add or update tests when behavior, packaging, or public API changes.
5. Run formatting, tests, clippy, and milestone-specific checks.
6. Verify docs and examples match implemented behavior and supported
   limitations.
7. Record what works, what is deferred, and the next risk.
8. Commit the checkpoint with a Conventional Commit message.
9. Push the existing branch before starting the next milestone when the
   checkpoint is stable.
10. Update the existing PR description or comments when milestone scope,
    validation, or deferred work changes.

Use these standard checks after Rust code changes:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Run the spawned-binary smoke tests when changing listener startup, shutdown,
protocol transport, client/server process behavior, or integration test
fixtures:

```sh
cargo test --test spawned_client -- --ignored
```

For Python client milestones, define and run package-local checks. The target
commands should include unit tests, spawned-server integration tests, and source
distribution or wheel checks once packaging exists.

Before opening the PR, run the local contribution checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Keep using the existing pull request for this branch. The PR title must use
Conventional Commits, and the PR description should summarize the implemented
milestone scope, validation commands, packaging checks, and any deferred
follow-up work. Do not edit generated release metadata manually; version
bumps, changelog updates, SemVer tags, GitHub Releases, binary assets, and
GHCR images are handled by automation.

## Milestone 0: Native Python Decision Record

Goal:

- Capture the decision to prefer a native Python client over Rust-backed
  Python bindings for the default Python package.

Implementation:

- Add this goal loop prompt under `docs/internal/`.
- Document the reasons for the pivot: `asyncio`, gevent, source distribution,
  pure Python wheels, and future TypeScript protocol implementation.
- Document what remains valuable from the Rust work: the protocol crate, Rust
  client crate, golden fixtures, and conformance tests.
- Decide whether any existing PyO3 package code should be removed, parked as
  experimental, or replaced in a follow-up milestone.

Checkpoint:

- The architectural direction is clear without relying on private context.
- The prompt says to continue on the existing feature branch and PR,
  checkpoint, test, verify, commit, push, and update the PR.
- Documentation-only checks pass.

## Milestone 1: Protocol Contract And Fixture Audit

Goal:

- Make the native protocol safe to reimplement in Python and TypeScript.

Implementation:

- Audit `docs/protocol.md`, `docs/internal/native-socket-protocol.md`,
  `crates/cachebox-protocol`, server protocol handling, and spawned-client
  tests.
- Identify any protocol behavior that exists in code but is unclear or missing
  in docs.
- Identify request and response frames that need golden fixtures, including
  errors, metadata, stale values, leases, batch get, and tag invalidation.
- Decide how golden fixtures are generated, stored, and consumed by Rust,
  Python, and future TypeScript clients.

Checkpoint:

- The audit lists the exact fixture set needed for first-class client
  conformance.
- Protocol docs are precise enough for an implementer who has not read the Rust
  code.
- Known ambiguity is captured as follow-up work before native client coding
  depends on it.

## Milestone 2: Golden Frame Conformance Harness

Goal:

- Establish byte-level compatibility tests before reimplementing the protocol
  in Python.

Implementation:

- Add checked-in golden fixtures for representative native requests and
  responses.
- Cover `Get`, `Put`, `Delete`, `BatchGet`, `TagInvalidate`, `LeaseStart`,
  `LeaseComplete`, `Hit`, `Stale`, `Miss`, `Stored`, `Deleted`, `Invalidated`,
  `LeaseGranted`, `LeaseDenied`, and structured errors.
- Add Rust tests that verify the protocol crate still encodes and decodes the
  fixtures.
- Document how other clients should consume the fixtures.

Checkpoint:

- Golden fixture tests fail on wire format drift.
- Fixtures include enough metadata cases to protect TTL, stale TTL, tags, cost,
  and content type encoding.
- The fixture layout is suitable for Python and TypeScript test suites.

## Milestone 3: Native Python Package Skeleton

Goal:

- Replace the default Python package foundation with a pure Python package.

Implementation:

- Create or reshape `clients/python/` around a pure Python package.
- Remove maturin/PyO3 as the default build backend if the package no longer
  needs a native extension.
- Configure package metadata, test commands, and local development docs.
- Keep the package import path stable as `cachebox`.
- Add a minimal no-network test that imports the package and verifies version
  metadata.
- Build an sdist and wheel locally when packaging tools are available.

Checkpoint:

- `pip`/`uv` can install the package without compiling Rust.
- The wheel is universal Python when no native extension is present.
- The source distribution contains the files needed for normal Python
  installation and tests.
- Docs explain that Rust is not required for Python client installation.

## Milestone 4: Native Protocol Codec In Python

Goal:

- Implement enough native protocol encode/decode behavior in Python to talk to
  Cachebox directly.

Implementation:

- Add Python types for command IDs, frame headers, request payloads, response
  payloads, metadata, TTL, content type, and error codes.
- Implement request encoding and response decoding using structured binary
  APIs, not ad hoc string manipulation.
- Validate payload sizes and malformed frames consistently with the protocol
  docs.
- Run Python codec tests against the golden fixtures from Milestone 2.

Checkpoint:

- Python can encode and decode the golden fixture set.
- Codec errors are typed and documented.
- The implementation is small, readable, and independent from server internals.

## Milestone 5: Synchronous Python Driver

Goal:

- Provide a low-level synchronous Python client that works with normal sockets
  and gevent monkey patching.

Implementation:

- Implement `Client.connect_tcp` and, where supported, Unix socket connection
  helpers.
- Add `get`, `put`, `delete`, `batch_get`, `invalidate_tag`, `start_lease`,
  and `complete_lease`.
- Use Python socket APIs so gevent-patched sockets can cooperate naturally.
- Add timeouts, close behavior, context manager support, and clear connection
  error mapping.
- Add an optional synchronous connection pool for multi-threaded callers and
  repeated short operations. The default client may remain a single connection,
  but the high-level cache API should be able to use a pool when configured.
- Document whether one connection is safe for concurrent use. If it is not,
  guard it with a lock, fail clearly, or route concurrent work through the
  optional pool.
- Do not implement public request pipelining in this milestone. The consensus
  is to ship a complete driver through batching, pooling, and clear concurrency
  behavior first.
- Add spawned-server integration tests for core workflows.

Checkpoint:

- Sync Python can perform core cache operations against a real Cachebox server.
- Lease start and completion work through the sync client.
- Optional pooling is either implemented and tested or explicitly deferred with
  the API shape documented.
- Pipelining remains out of the public sync API for the first release.
- Tests document gevent expectations even if gevent-specific CI is deferred.
- Public low-level APIs are stable enough for the high-level cache layer.

## Milestone 6: Asyncio Python Driver

Goal:

- Provide a first-class async client that does not block the Python event loop.

Implementation:

- Implement `AsyncClient` using `asyncio.open_connection` and async stream
  reads/writes.
- Mirror the low-level sync API with `await`-based methods.
- Add async context manager support.
- Add an optional async connection pool for concurrent coroutine workloads.
  Define pool sizing, acquisition timeout, close behavior, and cancellation
  behavior before implementation.
- Add tests for concurrent operations, cancellation behavior, connection close,
  and spawned-server workflows.
- Do not implement public request pipelining in this milestone. Keep async
  internals structured so request ID matching and pipelining can be added later
  without reshaping the public driver.

Checkpoint:

- Async Python callers can use Cachebox without `run_in_executor` or blocking
  PyO3 calls.
- Optional async pooling is either implemented and tested or explicitly
  deferred with the API shape documented.
- Cancellation and timeout behavior is documented.
- Tests prove multiple coroutines can use the client safely according to the
  documented concurrency model.

## Milestone 6A: Pipelining Decision Record

Goal:

- Record the initial Python client pipelining decision and preserve room for a
  later advanced API.

Implementation:

- Consensus: do not expose public pipelining in the first native Python client
  release.
- Rely on `batch_get`, optional connection pooling, and high-level caching
  helpers for the initial complete driver experience.
- Keep protocol internals capable of future request ID matching so pipelining
  can be added without rewriting the codec.
- If pipelining is revisited later, prefer async-first or internal-first
  implementation before adding a sync/gevent public API.
- Before any future public pipelining API is added, define response ordering
  guarantees, request ID matching, partial failure behavior, cancellation
  behavior, timeout handling, backpressure, maximum in-flight requests, and
  interaction with reconnects.
- Treat future public pipelining as an advanced low-level feature, not as a
  prerequisite for decorator-based caching.

Checkpoint:

- The initial release has no public Python pipelining API.
- The driver remains complete through batching, pooling, and clear concurrency
  behavior.
- Future pipelining work has explicit decision criteria before it can become
  public API.

## Milestone 7: Serialization And Key Building

Goal:

- Add Python-native cache value and key policies above the byte-level driver.

Implementation:

- Add serializer abstractions for bytes, JSON, and optionally pickle.
- Make unsafe or Python-specific serializers explicit.
- Add deterministic key builders for function calls, explicit key templates,
  namespace prefixes, versions, and custom key functions.
- Support tags and cost metadata from the high-level API.
- Add tests for stable keys across positional args, keyword args, defaults, and
  unsupported values.

Checkpoint:

- High-level APIs do not require users to manually convert every value to
  bytes.
- Key generation is deterministic, documented, and overrideable.
- Serializer errors are distinct from cache server errors and wrapped function
  errors.

## Milestone 8: High-Level Cache API

Goal:

- Make the Python client feel like a caching library.

Implementation:

- Add `Cachebox` for sync usage and `AsyncCachebox` for async usage.
- Add `get_or_set`, `set`, `get`, `delete`, `invalidate_tag`, and explicit
  invalidation helpers.
- Add `@cache.memoize(...)` for function-argument-derived keys.
- Add `@cache.cached(key=...)` for explicit key templates or key callables.
- Support TTL, stale TTL, tags, cost, serializer choice, namespace, key
  version, and negative caching policy where appropriate.
- Allow high-level cache clients to accept either a low-level client instance
  or pool configuration, so applications can choose single-connection or pooled
  behavior without changing decorators.

Checkpoint:

- Common Python usage is decorator-first and does not expose protocol framing.
- Low-level driver methods remain available for advanced users.
- Docs show sync examples, async examples, and explicit invalidation examples.

## Milestone 9: Dogpile Protection And Stale Policy

Goal:

- Provide stampede-safe caching behavior backed by Cachebox leases.

Implementation:

- Implement decorator and `get_or_set` flows using `start_lease` and
  `complete_lease`.
- Support policy options for lease TTL, stale TTL, stale return, wait/retry,
  backoff, jitter, lease denial timeout, and failure fallback.
- Decide and document behavior when the wrapped function raises after a lease
  is granted.
- Add tests where many concurrent sync callers, async callers, or greenlet-like
  callers request the same missing key and only one computation runs.
- Add tests for stale-while-refresh behavior when a stale value exists.

Checkpoint:

- Dogpile protection is available by default or by a clearly named option.
- Lease denial does not force every caller to recompute.
- Stale behavior is predictable and backed by server semantics.
- Missing server support, such as ignored `allow_stale_ms` or absent lease
  abort behavior, is either fixed or explicitly documented before release.

## Milestone 10: Gevent Compatibility Check

Goal:

- Verify the sync client behaves correctly in gevent-style applications.

Implementation:

- Add optional tests that run with gevent installed.
- Verify the sync socket path cooperates with monkey-patched sockets.
- Document import-order expectations for gevent monkey patching.
- Decide whether to include gevent in CI or keep it as an optional integration
  check.

Checkpoint:

- gevent support is tested or clearly documented as best-effort.
- No Rust/Tokio runtime dependency bypasses gevent's cooperative scheduling in
  the default Python client.
- Users know what setup is required.

## Milestone 11: Packaging And Release Readiness

Goal:

- Make the Python package straightforward to publish and install.

Implementation:

- Verify source distribution and wheel contents.
- Add packaging checks to CI.
- Ensure package metadata, README examples, Python version support, license,
  and classifiers match implemented behavior.
- Document local install, editable install, test commands, and release
  expectations.
- Keep generated release metadata managed by automation.

Checkpoint:

- The package can be installed from a wheel without Rust.
- The package can be installed from an sdist with standard Python packaging
  tools.
- CI exercises sync, async, protocol fixture, and spawned-server behavior.
- Docs clearly distinguish driver APIs from high-level caching APIs.

## Definition Of Done

The native Python client work is done when:

- The Python package is pure Python by default and does not require Rust for
  normal installation.
- The Python protocol codec passes the shared golden frame fixtures.
- The sync client supports core Cachebox operations against a real server.
- The async client supports core Cachebox operations without blocking the event
  loop.
- Optional sync and async connection pooling is implemented and documented.
- Public Python pipelining is intentionally deferred for the first release, and
  the future API criteria are recorded before any public API is added for it.
- The sync client is compatible with gevent's socket monkey patching or clearly
  documents any limits.
- High-level `Cachebox` and `AsyncCachebox` APIs provide decorator-based
  caching.
- Dogpile protection uses server-backed leases and is covered by contention
  tests.
- Serialization, key building, stale policy, lease denial, and invalidation are
  documented and tested.
- The Rust client remains the official Rust client and protocol reference.
- The protocol contract is strong enough for a future TypeScript client to
  implement independently.
- The implementation continues on the existing official-client feature branch
  and existing pull request with a Conventional Commit title.
- The PR summary lists completed milestones, validation commands, packaging
  checks, and intentionally deferred work.
