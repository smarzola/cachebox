# AI-Native Cache Goal Loop Prompt

Use this prompt to evolve Cachebox's AI-native cache capabilities in small,
checkpointed loops. The goal is to make AI workloads easier to cache without
turning Cachebox into a model gateway, vector database, or durable store.

## Role

You are extending Cachebox, a Rust cache server for self-hosted applications.
Treat AI-native caching as cache semantics, client helpers, and metadata layered
on the existing byte-value cache model. Prefer features that reuse keys, TTLs,
stale TTLs, tags, leases, raw bytes, metrics, and cost hints before adding new
server concepts.

## Target State

Cachebox should support AI workloads through:

- Deterministic client-side key builders for prompts, model responses, and
  embeddings.
- Metadata that captures recomputation cost without requiring model-provider
  integration.
- Lease helpers that prevent repeated generation of the same expensive result.
- Clear tag conventions for invalidating model, prompt, document, retriever,
  policy, and workspace-derived values.
- An experimental, explicitly scoped path for stream capture and replay.
- Documentation that distinguishes implemented behavior from future design.

The server must remain a cache. It should not run inference, own prompts,
perform semantic search, proxy model APIs, or promise durable generated output
storage.

## Operating Loop

For every milestone:

1. Restate the target behavior and the current implementation gap.
2. Inspect the current docs and code before editing.
3. Make the smallest coherent change that advances the milestone.
4. Add or update tests when behavior changes.
5. Run formatting, tests, and any milestone-specific checks.
6. Verify the docs match implemented behavior and supported limitations.
7. Record what works, what is deferred, and the next risk.
8. Commit the checkpoint and push it before starting the next milestone.

Use these standard checks after code changes:

```sh
cargo fmt --check
cargo test
cargo clippy --all-targets -- -D warnings
```

For documentation-only milestones, run at least:

```sh
cargo fmt --check
cargo test
```

Use `cargo fmt` before committing if formatting fails. Commit messages should
name the user-visible capability or documentation checkpoint.

## Milestone 0: Goal Loop Baseline

Goal:

- Capture the AI-native target state, definition of done, milestone sequence,
  and checkpoint discipline in a project document.

Implementation:

- Add the goal loop prompt under `docs/`.
- Link it from the README repository layout or future-work references.
- Keep the prompt specific to Cachebox and appropriate to commit.

Checkpoint:

- The prompt is actionable without relying on private context.
- The prompt says to checkpoint, test, verify, commit, and push each milestone.
- Documentation checks pass.

## Milestone 1: Align AI Docs With Existing Primitives

Goal:

- Make `docs/ai-native-cache.md` accurately describe which AI-relevant
  primitives already exist and which remain future work.

Implementation:

- Call out existing raw-byte values, TTLs, stale TTLs, tags, leases, and
  reserved `Cachebox-Cost` metadata.
- Clarify that `allow_stale_ms` is parsed but not yet applied by the server if
  lease behavior is discussed.
- Keep provider-specific prompt semantics in client helpers, not server code.

Checkpoint:

- The AI-native doc no longer implies unsupported server behavior.
- Existing primitives are presented as the foundation for AI support.
- Tests still pass.

## Milestone 2: Make The AI Roadmap Implementation-Ready

Goal:

- Turn the first AI feature candidates into a sequence that minimizes server
  surface area and implementation risk.

Implementation:

- Prioritize documented cost metadata, prompt key helpers, embedding key
  helpers, generation lease helpers, cost-aware policy experiments, and only
  then streaming capture.
- Define the first streaming experiment as a conservative path before any
  chunked append protocol is introduced.
- Identify measurable success criteria for each early feature.

Checkpoint:

- The roadmap is ordered by dependency and risk.
- The next implementable feature can be picked up directly from the doc.
- Tests still pass.

## Milestone 3: Prompt And Result Key Helpers

Goal:

- Provide deterministic prompt/result cache key helpers in the first official
  client or a small shared helper module.

Implementation:

- Normalize structured prompt request fields.
- Include provider, model, model version or deployment id, message list,
  system prompt, tool schema, sampling parameters, output format, retrieval
  context hash, and application namespace.
- Hash normalized bytes into stable key bytes.
- Document cross-language normalization rules.

Checkpoint:

- Equivalent structured requests produce identical keys.
- Meaningfully different model, prompt, tool, sampling, or retrieval inputs
  produce different keys.
- Unit tests cover ordering, optional fields, Unicode, and binary-safe output.

## Milestone 4: Embedding Key Helpers

Goal:

- Provide deterministic embedding cache keys without adding vector search.

Implementation:

- Normalize embedding model, model version, input content hash, normalization
  settings, chunking strategy, and dimension count.
- Store embeddings as raw bytes with optional content-type and metadata
  conventions documented for clients.

Checkpoint:

- Equivalent embedding inputs produce identical keys.
- Chunking and dimension changes invalidate the key.
- Documentation keeps similarity search out of scope.

## Milestone 5: Generation Lease Helpers

Goal:

- Make stampede protection ergonomic for expensive AI generation.

Implementation:

- Add client helper flow for start lease, serve stale if available, generate
  only when granted, and complete lease with refreshed bytes.
- Document retry behavior for lease denial and expired tokens.
- Avoid adding model-provider-specific gateway logic.

Checkpoint:

- Helper tests cover hit, stale, lease granted, lease denied, and completion
  failure paths.
- Server lease behavior remains transport-level cache behavior.

## Milestone 6: Cost-Aware Policy Experiment

Goal:

- Explore cost-aware retention without replacing the default approximate LRU
  policy prematurely.

Implementation:

- Preserve the simple default eviction behavior.
- Store or surface enough cost metadata to compare policy variants.
- Add benchmark scenarios for cheap large values, expensive small values, and
  mixed TTL pressure.

Checkpoint:

- Any policy claim has a reproducible benchmark command.
- Cost-aware behavior is opt-in or experimental.
- Metrics make policy effects visible.

## Milestone 7: Streaming Capture Experiment

Goal:

- Add the smallest safe streaming cache experiment.

Implementation:

- Prefer a buffer-then-commit flow first: clients stream to callers, buffer
  locally, and complete the lease with one value only after successful
  generation.
- Defer server-side chunk append until chunk boundaries, partial replay,
  failure handling, and stale replay semantics are specified.
- Mark any endpoint or helper as experimental.

Checkpoint:

- Failed generations do not publish partial values by default.
- Replay behavior is deterministic.
- The design still preserves raw-byte cache semantics.

## Definition Of Done

The AI-native cache work is done when:

- Existing docs accurately distinguish implemented behavior from future
  capabilities.
- The first client helpers produce deterministic keys across supported
  languages.
- AI generation can use leases without duplicate recomputation under common
  contention.
- Cost metadata is documented, accepted, and measured before it influences
  eviction policy.
- Any cost-aware policy remains opt-in until benchmarked and understood.
- Streaming cache support has conservative failure semantics and does not serve
  partial values unless explicitly enabled.
- Non-goals remain enforced in docs, API shape, and implementation.

## Commit Discipline

Each milestone must end with:

- A checkpoint note in the final response or PR summary.
- Passing required checks for the milestone.
- A local commit with a concise message.
- A push to the current branch before starting the next milestone.

If a push or check is blocked, stop the milestone, report the exact blocker, and
do not start the next milestone until the checkpoint state is clear.
