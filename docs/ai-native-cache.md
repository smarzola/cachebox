# AI-Native Cache

Cachebox should be useful for ordinary web caches, but AI workloads create
specific pressure: expensive recomputation, large values, streaming outputs,
model/version sensitivity, token cost, and repeated prompt or embedding work.

This document captures future AI-native features. These are not required for the
MVP.

## Design Goals

- Reduce repeated model, embedding, and retrieval work.
- Make expensive cache entries survive longer than cheap entries when memory is
  tight.
- Coordinate refresh so many clients do not recompute the same expensive result.
- Keep cache keys deterministic across languages.
- Support raw bytes and streamed responses.
- Avoid becoming a vector database or model gateway.

## Existing Foundation

The MVP already has several primitives that AI-native features should build on:

- Raw-byte keys and values, so clients can store model responses, embeddings,
  serialized documents, and binary artifacts without forcing JSON encoding.
- Fresh TTL and stale TTL metadata for serving known-good values while refresh
  work happens.
- Tag invalidation for model, prompt, document, collection, workspace, and
  policy changes.
- Lease start and completion for stampede protection around expensive
  generation or embedding work.
- `Cachebox-Cost`, which is parsed on writes and reserved for future policy.
- Prometheus-style metrics for cache outcomes, leases, errors, memory, and
  evictions.

AI-specific work should first make these primitives easier to use from clients
before adding new server behavior.

## Prompt and Result Cache

Applications should be able to cache model outputs using normalized request
metadata.

Cache key inputs:

- Provider.
- Model name.
- Model version or deployment id.
- Prompt or message list.
- System prompt.
- Tool schema.
- Temperature and sampling parameters.
- Retrieval context hash.
- Output format.
- Application namespace.

The server should not need to understand prompt semantics in the MVP. Official
clients can provide helpers that generate stable cache keys from structured
model requests.

Metadata:

- Token input count.
- Token output count.
- Estimated or user-provided cost. The server already accepts `Cachebox-Cost`
  as a reserved unsigned integer, but the current eviction policy does not use
  it.
- Latency saved.
- Safety or policy version.
- Model fingerprint.

## Embedding Cache

Embedding generation is a strong cache candidate because inputs are often
repeated across indexing, RAG, and local automation workflows.

Cache key inputs:

- Embedding model.
- Model version.
- Input text or content hash.
- Normalization settings.
- Chunking strategy.
- Dimension count.

Stored value:

- Raw embedding bytes.
- Optional content type for common encodings.
- Metadata for dimensions, dtype, and model.

Non-goal:

- Similarity search. Cachebox can cache embeddings, but it should not become a
  vector database.

## Streaming Response Cache

Many AI responses are streamed. Cachebox can eventually support storing and
replaying stream chunks.

Possible behavior:

- Client opens a lease for a prompt key.
- Client streams generated chunks through Cachebox while returning them to the
  caller.
- Later clients can replay the cached stream.
- If generation fails, Cachebox can discard the partial value or retain it only
  when explicitly allowed.

The conservative first version should be client-side buffer-then-commit:
capture streamed chunks in the client, return them to the caller as they arrive,
and complete the Cachebox lease with one raw-byte value only after generation
succeeds.

Open questions:

- Should partial streams ever be served?
- How should chunk boundaries be represented?
- Should stream metadata be separate from value bytes?
- How should stale stream replay interact with refresh leases?

## Partial Result Caching

Some AI workflows have reusable intermediate steps.

Examples:

- Parsed documents.
- Chunked documents.
- Retrieval results.
- Reranker outputs.
- Tool call results.
- Prompt template expansions.

Cachebox should support these with ordinary keys, tags, and cost hints rather
than hardcoding every AI workflow.

## Content-Addressed Blob Dedupe

Large AI-adjacent values often repeat:

- Documents.
- Chunks.
- Images.
- Audio snippets.
- Prompt context blobs.
- Model responses copied across namespaces.

Future dedupe could store value bytes by content hash while keys point to blob
references.

Requirements:

- Reference counting or safe garbage collection.
- Namespace memory accounting that remains understandable.
- Protection against hash collision assumptions.
- Clear operator visibility into dedupe savings.

## Token-Cost-Aware Eviction

AI entries can be expensive to recompute. Eviction should eventually consider
the cost saved by keeping an entry.

Possible cost fields:

- Input tokens.
- Output tokens.
- Provider price estimate.
- Local model compute estimate.
- Wall-clock generation time.
- User-provided cost score.

Eviction score can combine:

- Recency.
- Frequency.
- Value size.
- Fresh TTL.
- Stale TTL.
- Recompute cost.
- Namespace quota pressure.

The default should be simple and safe. Cost-aware policies should be opt-in until
they are well understood.

## AI-Specific Tags

Tags make invalidation practical when upstream inputs change.

Useful tags:

- `model:{name}`
- `model-version:{id}`
- `workspace:{id}`
- `user:{id}`
- `document:{id}`
- `collection:{id}`
- `prompt-template:{id}`
- `retriever:{id}`
- `policy:{id}`

Examples:

- Invalidate all cached responses for a changed prompt template.
- Invalidate embeddings for a re-chunked document.
- Invalidate results when a model deployment changes.
- Invalidate RAG responses when a collection is rebuilt.

## Client Helpers

AI-native behavior should mostly arrive through client helpers and metadata,
not server-side model integration.

Potential helpers:

- Stable prompt cache key builder.
- Embedding cache key builder.
- Token cost metadata wrapper.
- Lease helper for generation.
- Stream capture and replay helper.
- Tag builder for documents, models, and workspaces.

This keeps Cachebox independent from specific model providers.

Lease helpers should match currently supported server behavior. The lease start
request parses `allow_stale_ms`, but the server does not apply that field yet;
helpers should treat stale serving as controlled by the entry's stored stale TTL
until request-scoped stale controls are implemented.

## Non-Goals

- Running model inference.
- Acting as an LLM gateway.
- Performing semantic similarity search.
- Owning prompt templates.
- Replacing vector databases.
- Guaranteeing durable storage of generated outputs.

## First AI Feature Candidates

After the general MVP is hardened, the best AI-specific first features are:

1. Client-side prompt cache key helpers.
2. Token-cost metadata on writes.
3. Cost-aware eviction policy experiment.
4. Streaming response capture behind an experimental endpoint.
5. Embedding cache key helpers.

These build directly on existing Cachebox primitives instead of widening the
server into a model platform.
