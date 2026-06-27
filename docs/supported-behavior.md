# Supported Behavior

This document describes the current Cachebox MVP surface.

## Runtime

- Single server binary: `cachebox`.
- Primary transport: HTTP/2 cleartext on the configured bind address.
- Local tooling transport: HTTP/1.1 is also accepted.
- Default bind address: `127.0.0.1:7400`.
- Values are stored and returned as raw bytes.

## Endpoints

| Method | Path | Behavior |
| --- | --- | --- |
| `GET` | `/healthz` | Health check. |
| `GET` | `/metrics` | Prometheus-style process metrics. |
| `GET` | `/v1/namespaces/{namespace}/keys/{key}` | Get a raw-byte value. |
| `PUT` | `/v1/namespaces/{namespace}/keys/{key}` | Store a raw-byte value. |
| `DELETE` | `/v1/namespaces/{namespace}/keys/{key}` | Delete a value. |
| `POST` | `/v1/namespaces/{namespace}/batch/get` | Batch get percent-encoded keys from JSON. |
| `POST` | `/v1/namespaces/{namespace}/tags/{tag}/invalidate` | Delete values with a tag. |
| `POST` | `/v1/namespaces/{namespace}/leases/{key}` | Start stampede-protection lease flow. |
| `PUT` | `/v1/namespaces/{namespace}/leases/{key}/complete` | Complete a lease with refreshed bytes. |

Path keys and tags are percent-decoded. Namespaces are ASCII letters, numbers,
`-`, and `_`.

## Metadata

PUT and lease completion accept:

- `Cachebox-TTL`: fresh lifetime, such as `300s`, `5m`, or `100ms`.
- `Cachebox-Stale-TTL`: stale lifetime after fresh TTL expires.
- `Cachebox-Tags`: comma-separated ASCII tags.
- `Cachebox-Cost`: parsed and reserved for future policy.
- `Content-Type`: preserved only as contract metadata; values remain raw bytes.

## AI Helpers

- `cachebox::ai::prompt_cache_key` builds deterministic ASCII byte keys for
  prompt/result cache entries.
- Prompt key normalization includes provider, model, optional model version,
  message list, optional system prompt, optional tool schema, sampling
  parameters, optional output format, optional retrieval context hash, and
  application namespace.
- `cachebox::ai::embedding_cache_key` builds deterministic ASCII byte keys for
  embedding cache entries.
- Embedding key normalization includes model, optional model version, input
  content hash, normalization settings, chunking strategy, dimensions, and
  application namespace.
- `cachebox::ai::generation_lease_action` maps lease start states into client
  actions: return cached bytes, generate with a lease token, or retry later.
- The helper is provider-neutral and does not call model APIs.

## Memory

- `--max-body-bytes` limits accepted request body size.
- `--max-memory-bytes` limits approximate in-memory cache size.
- `--max-value-bytes` limits a single cached value.
- Eviction policy is approximate LRU.
- Expired entries are reclaimed before live entries are evicted.

## Leases

Lease start returns structured JSON states:

- `hit`: a fresh value already exists.
- `stale`: a stale value is served while a refresh lease is active.
- `lease_granted`: this client may recompute and complete refresh.
- `lease_denied`: another active lease already protects the key.

Lease state is in-memory and process-local.

## Unsupported

- Redis/RESP compatibility.
- Persistence, replication, clustering, scripting, Lua, streams, and modules.
- Authentication and authorization.
- Namespace-specific quotas.
- Durable metrics across restart.
- A packaged official client library.
