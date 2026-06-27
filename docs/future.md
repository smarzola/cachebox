# Future Product Direction

The MVP proves the core shape: HTTP-first cache operations, raw-byte values,
TTL, stale TTL, tag invalidation, memory limits, approximate LRU eviction,
metrics, and lease-based stampede protection.

Future work should preserve that cache-native boundary. Cachebox should become a
coordination layer for expensive recomputation, not a generic database.

## Product Pillars

### Cache Coordination

Cachebox should keep moving beyond key/value storage into coordination features
that application teams usually rebuild around locks and ad hoc metadata.

Planned capabilities:

- Lease renewal and cancellation.
- Stale-if-error responses.
- Negative caching for known misses.
- Refresh hints for background workers.
- Request coalescing metrics by key and namespace.
- Configurable behavior for wait, stale, lease, and miss outcomes.

### Policy-Driven Namespaces

Namespaces should become the main operator-facing boundary.

Planned namespace policy:

- Default fresh TTL and stale TTL.
- Memory quota.
- Max key size and max value size.
- Eviction policy.
- Stale serving policy.
- Admission policy for large or low-value entries.
- Tag limits.
- Lease defaults.
- Auth scope.

The goal is to let one Cachebox process safely serve multiple apps, teams, or
tenants without one workload evicting everything else.

### Smarter Eviction

MVP approximate LRU is only a starting point. Cachebox should make eviction
decisions using cache-specific signals.

Potential policy inputs:

- Last access time.
- Access frequency.
- Value size.
- Fresh TTL and stale TTL.
- Recompute cost.
- Namespace priority.
- Token cost saved.
- Admission score.
- Whether a value has active leases or recent stampede events.

Potential policies:

- Approximate LFU.
- TinyLFU-style admission.
- Cost-aware LRU.
- Size-aware eviction.
- Namespace quota eviction.
- Expired-first reclamation.

### Multi-Tier Cache

Cachebox can remain a cache while using more than RAM.

Possible tiers:

- Hot: RAM.
- Warm: local NVMe.
- Cold: optional S3-compatible object storage.

Rules:

- RAM remains the fastest and most predictable path.
- Disk/object tiers are best-effort cache layers, not durability promises.
- Missing cold entries must not be correctness failures.
- Operators must be able to cap each tier independently.
- Promotion and demotion should be observable.

### Observability and Diagnostics

Self-hosters need simple answers without a full observability stack.

Future diagnostics:

- Hot keys.
- Largest keys.
- Top missed keys.
- Top stale responses.
- Lease contention by key.
- Stampede events.
- Evictions by reason.
- Hit rate by namespace and tag.
- Memory use by namespace and tag.
- Estimated recomputation cost saved.
- Estimated token cost saved for AI workloads.

The admin surface should expose these through metrics, structured JSON, and
eventually a small web UI.

### Official Clients

The HTTP API keeps third-party client requirements low, but official clients can
make cache semantics easier to use correctly.

Initial client targets:

- TypeScript.
- Python.
- Rust.
- Go.

Client responsibilities:

- Percent-encode byte keys correctly.
- Carry TTL, stale TTL, tags, and cost hints.
- Provide ergonomic lease helpers.
- Support binary values.
- Surface structured errors.
- Add optional single-flight behavior inside the client.

### Admin UI

A small admin UI could help self-hosters understand behavior quickly.

Possible views:

- Health and uptime.
- Memory usage.
- Namespace quotas.
- Hit and miss rates.
- Eviction reasons.
- Lease contention.
- Hot keys and large keys.
- Recent errors.

The UI should be optional and read-only by default.

### Transport Evolution

HTTP/2 is the default transport for the product because it is mature, debuggable,
and widely supported.

Future transports may include:

- HTTP/3 for lossy networks, edge deployments, or direct internet-facing
  workloads.
- Unix domain sockets for same-host deployments.
- A compact binary batch protocol if HTTP overhead becomes measurable in real
  benchmarks.
- A Redis adapter only if real adoption needs justify it.

The cache engine must stay transport-independent so these are adapters, not
architectural rewrites.

### Policy Hooks

Custom behavior may be useful later, but extension points should not compromise
the performance or safety story.

Possible hooks:

- Key normalization.
- Admission scoring.
- Eviction scoring.
- Tag derivation.
- Namespace routing.

WASM may be considered, but only after the built-in policy model is mature.

## Roadmap Sequence

### Phase 1: Harden the MVP

- Add more integration tests.
- Add concurrency stress tests.
- Improve benchmark coverage.
- Make startup configuration file-based as well as CLI-based.
- Validate behavior behind a local reverse proxy.

### Phase 2: Improve Cache Semantics

- Lease renewal and cancellation.
- Stale-if-error.
- Negative caching.
- Per-namespace policy.
- Better eviction and admission.

### Phase 3: Build Ecosystem Pieces

- TypeScript client.
- Python client.
- Rust client.
- Example self-hosted app integration.
- Deployment examples for systemd and containers.

### Phase 4: AI-Native Capabilities

- Prompt/result cache helpers.
- Embedding cache helpers.
- Token-cost-aware eviction.
- Streaming response cache.
- Content-addressed blob dedupe.

See [ai-native-cache.md](ai-native-cache.md) for the AI-specific design.

### Phase 5: Multi-Tier and Advanced Operations

- Local disk tier.
- Optional object-storage tier.
- Admin UI.
- Advanced diagnostics.
- Optional HTTP/3 transport.

## Guardrails

- Do not turn Cachebox into a durable database.
- Do not add complex compatibility layers before the native product is strong.
- Do not make performance claims without a reproducible benchmark.
- Do not make custom policy hooks part of the hot path until built-in policy is
  insufficient.
- Keep defaults safe for small self-hosted deployments.
