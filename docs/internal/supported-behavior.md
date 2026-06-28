# Supported Behavior

This document describes the current Cachebox MVP surface.

## Runtime

- Single server binary: `cachebox`.
- Primary cache transport: native socket protocol.
- Default native TCP data-plane address: `127.0.0.1:7401`.
- Optional native Unix socket data plane with `--native-unix <path>`.
- Admin HTTP address: `127.0.0.1:7400`.
- Values are stored and returned as raw bytes.

## Native Commands

| Command | Behavior |
| --- | --- |
| `Get` | Get a raw-byte value by namespace and byte key. |
| `Put` | Store a raw-byte value with metadata. |
| `Delete` | Delete a value by namespace and byte key. |
| `BatchGet` | Get multiple byte keys in one request. |
| `TagInvalidate` | Delete values with a tag. |
| `LeaseStart` | Start stampede-protection lease flow. |
| `LeaseComplete` | Complete a lease with refreshed bytes. |

Namespaces are ASCII letters, numbers, `-`, and `_`. Keys are byte vectors and
do not require percent encoding in the native protocol.

## Admin HTTP

| Method | Path | Behavior |
| --- | --- | --- |
| `GET` | `/healthz` | Health check. |
| `GET` | `/metrics` | Prometheus-style process metrics. |

Admin HTTP is not a cache operation surface. Cache routes under
`/v1/namespaces/...` are rejected.

## Metadata

Put and lease completion accept:

- `ttl`: fresh lifetime in milliseconds.
- `stale_ttl`: stale lifetime after fresh TTL expires.
- `tags`: ASCII tags for invalidation.
- `cost`: optional aggregate cost-score metadata reserved for future policy.
- `content_type`: contract metadata. Values remain raw bytes.

## AI Helpers

- `cachebox_client::ai::prompt_cache_key` builds deterministic ASCII byte keys for
  prompt/result cache entries.
- Prompt key normalization includes provider, model, optional model version,
  message list, optional system prompt, optional tool schema, sampling
  parameters, optional output format, optional retrieval context hash, and
  application namespace.
- `cachebox_client::ai::embedding_cache_key` builds deterministic ASCII byte keys for
  embedding cache entries.
- Embedding key normalization includes model, optional model version, input
  content hash, normalization settings, chunking strategy, dimensions, and
  application namespace.
- `cachebox_client::ai::generation_lease_action` maps lease start states into client
  actions: return cached bytes, generate with a lease token, or retry later.
- `cachebox_client::ai::StreamCapture` supports experimental client-side
  buffer-then-commit capture for streamed generation bytes.
- The helper is provider-neutral and does not call model APIs.

## Memory

- `--max-body-bytes` limits accepted native frame payload size.
- `--max-memory-bytes` limits approximate in-memory cache size.
- `--max-value-bytes` limits a single cached value.
- `--cleanup-interval-ms` controls background expiration; `0` disables it.
- `--cleanup-max-entries-per-tick` caps background expiration work per tick.
- Eviction policy is approximate LRU.
- Expired entries are reclaimed by cache access paths or before live entries are
  evicted, and by the bounded background cleanup worker when enabled.

## Metrics

- `/metrics` includes `cachebox_cost_score_total`, the sum of cost values for
  currently accounted entries. Scraping metrics does not reclaim expired
  entries or increment request counters.
- Cache operation counters are updated by native requests.
- Admin HTTP health requests increment admin request counters.
- Cost score is observational only; it does not affect eviction policy yet.

## Leases

Lease start returns native response states:

- `Hit`: a fresh value already exists.
- `Stale`: a stale value is available.
- `LeaseGranted`: this client may recompute and complete refresh.
- `LeaseDenied`: another active lease already protects the key.

Lease state is in-memory and process-local.

## Unsupported

- HTTP cache operations.
- Redis/RESP compatibility.
- Persistence, replication, clustering, scripting, Lua, streams, and modules.
- Authentication and authorization.
- Namespace-specific quotas.
- Durable metrics across restart.
- Packaged official clients beyond the in-repo Rust native client.
