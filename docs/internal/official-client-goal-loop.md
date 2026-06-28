# Official Client Goal Loop Prompt

Use this prompt to create Cachebox's first official client library in small,
checkpointed loops. The goal is to make the native protocol easy to adopt
without making every user hand-write framing, request IDs, metadata encoding,
lease handling, and error mapping.

## Role

You are extracting and strengthening Cachebox's client surface while preserving
the existing single-binary server architecture. Treat protocol encoding,
transport behavior, ergonomic APIs, and AI helpers as client concerns. Keep the
server focused on cache execution, listener startup, protocol handling, memory
limits, metrics, expiration, tags, leases, and eviction.

Prefer a shared Rust foundation for official clients. The Rust client should be
the first official client crate, and the Python client should use the Rust
implementation underneath rather than reimplementing the binary protocol from
scratch.

## Target State

Cachebox should provide official clients through:

- A shared protocol crate that owns native frame types, constants, encode/decode
  logic, metadata types, response states, and structured error codes.
- A Rust client crate under `clients/rust/` that exposes an ergonomic API over
  TCP and Unix domain sockets.
- Client-side AI helpers in the Rust client crate for deterministic prompt keys,
  embedding keys, lease decisions, and conservative stream capture.
- A Python package under `clients/python/` that wraps the Rust client with
  Python-friendly types and errors.
- Golden codec tests that prove byte-level compatibility with the native
  protocol.
- Integration tests that run clients against the real `cachebox` server binary.
- Documentation that presents supported behavior without requiring users to read
  protocol internals.

The first client work should establish API conventions that future TypeScript,
Go, or other official clients can follow. Keep the initial surface small,
correct, and well-tested before adding convenience behavior.

## Design Principles

- Share one protocol implementation where practical.
- Keep protocol types independent from server API modules.
- Preserve raw byte keys and values.
- Make TTL, stale TTL, tags, cost, and content type explicit metadata.
- Expose leases in a way that makes stampede-safe flows natural.
- Return structured client errors instead of stringly typed failures.
- Keep reconnection and connection-close behavior documented and unsurprising.
- Avoid product behavior that is not backed by server semantics.
- Keep Python packaging native-wheel aware from the start.
- Prefer synchronous Python bindings first unless async support is required for
  the milestone.

## Operating Loop

Start from an up-to-date `main` branch and create a dedicated feature branch
before editing. Do not do milestone work directly on `main`.

For every milestone:

1. Restate the target behavior and the current implementation gap.
2. Inspect current code, tests, and docs before editing.
3. Make the smallest coherent change that advances the client target.
4. Add or update tests when behavior or package boundaries change.
5. Run formatting, tests, clippy, and milestone-specific checks.
6. Verify docs and examples match implemented behavior and supported
   limitations.
7. Record what works, what is deferred, and the next risk.
8. Commit the checkpoint with a Conventional Commit message and push the branch
   before starting the next milestone.

Use these standard checks after Rust code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

Run the spawned-binary smoke tests when changing listener startup, shutdown,
protocol transport, client/server process behavior, or integration test
fixtures:

```sh
cargo test --test spawned_client -- --ignored
```

For Python client milestones, also run the Python package's local test command
defined by that package once it exists.

Use `cargo fmt` before committing if formatting fails. Commit messages should
use Conventional Commits and name the user-visible client capability or
documentation checkpoint. Use `docs:` for documentation-only checkpoints,
`feat:` for new user-visible client behavior, `fix:` for bug fixes, `perf:` for
performance improvements, and non-release types such as `test:`, `ci:`,
`build:`, `chore:`, `style:`, or `refactor:` when appropriate.

Before opening the PR, run the local contribution checks:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Open a small pull request from the feature branch at the end of the loop. The
PR title must use Conventional Commits, and the PR description should summarize
the implemented milestone scope, validation commands, and any deferred follow-up
work. Do not edit generated release metadata manually; release PRs, version
bumps, changelog updates, SemVer tags, GitHub Releases, binary assets, and GHCR
images are handled by automation.

## Milestone 0: Goal Loop Baseline

Goal:

- Capture the official-client target state, milestone sequence, checkpoint
  discipline, and first-client strategy in a project document.

Implementation:

- Add this goal loop prompt under `docs/internal/`.
- Keep the prompt specific to Cachebox and appropriate to commit.
- Avoid copying private user or machine rules into repository docs.

Checkpoint:

- The prompt is actionable without relying on private context.
- The prompt says to work on a feature branch, checkpoint, test, verify, commit,
  push, and open a PR.
- Documentation-only checks pass.

## Milestone 1: Package Boundary Audit

Goal:

- Identify the exact server, protocol, Rust client, AI helper, benchmark, test,
  and documentation boundaries before moving code.

Implementation:

- Audit `src/protocol.rs`, `src/client.rs`, `src/ai.rs`, `src/api.rs`,
  `src/server.rs`, `tests/spawned_client.rs`, `src/bin/cachebox-bench.rs`, and
  public docs that import `cachebox::client` or `cachebox::ai`.
- List protocol dependencies that currently point back into server-facing API
  modules.
- Decide the least disruptive workspace layout for the first extraction.

Checkpoint:

- The migration plan names files that move, files that import moved modules,
  and compatibility risks.
- No behavior changes are made unless they are documentation-only.
- The next milestone can start from a concrete crate-boundary plan.

## Milestone 2: Shared Protocol Crate

Goal:

- Move native protocol definitions and codec logic into a server-independent
  crate.

Implementation:

- Create a protocol crate for frame constants, command IDs, request/response
  frame types, metadata, content type, TTL values, error codes, and
  encode/decode functions.
- Remove dependencies from protocol code back into server API modules.
- Update the server to depend on the shared protocol crate.
- Keep wire compatibility unchanged.

Checkpoint:

- Existing protocol unit tests still pass after the move.
- Golden encode/decode tests cover known request and response byte fixtures.
- Server code compiles without owning protocol type definitions.
- Native protocol docs still match the implemented byte format.

## Milestone 3: Rust Client Crate

Goal:

- Make the existing Rust native client the first official client crate.

Implementation:

- Create `clients/rust/` as a Rust crate that depends on the shared protocol
  crate.
- Move the current native client implementation into the client crate.
- Expose clear public types such as client, options, metadata, response states,
  lease states, and client errors.
- Keep TCP support and Unix domain socket support where platform-supported.
- Update internal tests, benches, examples, and docs to import the new client
  crate.

Checkpoint:

- A Rust user can connect, get, put, delete, batch get, invalidate tags, start
  leases, and complete leases without importing server modules.
- Existing spawned-client workflows pass against the real server binary.
- Public examples use the new crate path.

## Milestone 4: Client AI Helpers

Goal:

- Move AI-oriented helpers into the official Rust client crate.

Implementation:

- Move deterministic prompt key helpers, embedding key helpers, generation
  lease decision helpers, completion helpers, and stream capture helpers into a
  client-side AI module.
- Keep helpers provider-neutral and independent from server execution.
- Document cross-language normalization rules that the Python wrapper must
  preserve.

Checkpoint:

- Existing AI helper unit tests pass in the client crate.
- Equivalent structured inputs still produce identical keys.
- Docs no longer imply AI helpers are server behavior.

## Milestone 5: Ergonomic Rust API

Goal:

- Shape the Rust API around official client usage rather than internal smoke
  testing.

Implementation:

- Add high-level result types for hit, stale, miss, lease granted, lease denied,
  stored, deleted, invalidated, and batch results.
- Add metadata builder or options types for TTL, stale TTL, tags, cost, and
  content type.
- Add a conservative compute-on-miss or get-or-lease helper only if it maps
  directly to server-backed lease semantics.
- Document connection close, reconnect expectations, pipelining behavior, and
  structured errors.

Checkpoint:

- Common workflows do not require matching low-level protocol payload enums.
- Lease helpers prevent duplicate generation without hiding lease denial.
- Tests cover success, miss, stale, server error, unexpected response, and
  connection failure paths.

## Milestone 6: Python Package Foundation

Goal:

- Create a Python package that uses the Rust client underneath.

Implementation:

- Add a Python package under `clients/python/`.
- Use Rust bindings for protocol and transport behavior.
- Start with a synchronous Python API backed by the Rust implementation.
- Map Rust client errors into Python exception classes.
- Provide Python-friendly metadata, response, and lease result types.

Checkpoint:

- A Python user can connect to a running Cachebox server and perform get, put,
  delete, batch get, tag invalidation, lease start, and lease completion.
- Python tests verify core workflows against a spawned server.
- Python packaging instructions are documented for local development.

## Milestone 7: Python AI Helpers

Goal:

- Expose AI helper behavior in Python without duplicating normalization logic.

Implementation:

- Wrap Rust prompt key, embedding key, lease decision, completion, and stream
  capture helper behavior.
- Accept Python-native dictionaries, lists, strings, bytes, and numbers while
  preserving deterministic normalization.
- Add tests that compare Python helper outputs to Rust fixture outputs.

Checkpoint:

- Python and Rust produce identical AI cache keys for the same structured
  inputs.
- Python helper docs show provider-neutral usage.
- No model-provider gateway behavior is introduced.

## Milestone 8: Client CI And Release Readiness

Goal:

- Make official clients maintainable in normal development and release flows.

Implementation:

- Add CI coverage for the workspace, Rust client crate, protocol crate, and
  Python package.
- Ensure spawned server integration tests run where appropriate.
- Document local client development commands.
- Keep generated release metadata managed by automation.

Checkpoint:

- CI exercises protocol compatibility and client workflows.
- Local commands are documented and reproducible.
- The repository is ready for follow-up package publishing work.

## Definition Of Done

The official client work is done when:

- Protocol code is server-independent and shared by the server and official
  Rust client.
- The Rust client is a separate official client crate under `clients/rust/`.
- Client-side AI helpers live with the official client surface.
- The Python package uses Rust bindings rather than reimplementing native
  protocol framing.
- Rust and Python tests verify core workflows against a real Cachebox server.
- Golden codec tests protect wire compatibility.
- Docs show installable-client usage and clearly distinguish client helpers
  from server behavior.
- The implementation lives on a dedicated feature branch and is submitted as a
  pull request with a Conventional Commit title.
- The PR summary lists the completed milestones, validation commands, and any
  intentionally deferred work.
