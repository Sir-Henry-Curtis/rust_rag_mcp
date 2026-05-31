# Federated Search Design

Federated search lets one query search multiple independent RAG instances and
return one merged set of citation-ready results.

Each RAG instance remains responsible for its own corpus, vector store,
permissions, indexing, and citations. The federation layer coordinates across
instances; it does not replace the normal retrieval pipeline inside each
instance.

---

## Goal

The goal is to support deployments where knowledge is split across boundaries:

- different tenants
- different departments
- different source systems
- different security domains
- different vector store backends
- separate regional deployments
- separate production and archive corpora

Example:

```text
User asks:
"What policies affect customer travel reimbursement for field engineers?"

Federated search queries:
- finance RAG
- HR policy RAG
- engineering handbook RAG
- legal/compliance RAG

Final result:
One ranked list of cited passages from all eligible systems.
```

---

## Non-Goals

Federated search should not:

- merge all source documents into one central database
- replace each backend's permission checks
- require all backends to use the same `VectorStore`
- require all backends to use the same embedding model
- make `rag-core` responsible for remote service orchestration
- hide which backend produced a result

Federation is an orchestration problem, not a vector store feature.

---

## High-Level Architecture

```text
User / Agent / MCP Client
        |
        v
Federated Search Layer
        |
        |-- finance-rag      -> rag_search(...)
        |-- engineering-rag  -> rag_search(...)
        |-- legal-rag        -> rag_search(...)
        |-- support-rag      -> rag_search(...)
        |
        v
Result normalizer
        |
        v
Result merger / deduper / reranker
        |
        v
Final citation-ready results
```

Each backend can be:

- another `rust-rag-mcp` deployment
- an HTTP `rag-server` instance
- an MCP-accessible RAG server
- eventually, a compatible third-party search service wrapped by an adapter

The first implementation should target `rust-rag-mcp` HTTP endpoints because
HTTP is simpler for service-to-service fan-out than MCP stdio.

---

## Suggested Crate Layout

Federation should live in a new crate rather than inside `rag-core`.

```text
crates/rag-federation/
  Cargo.toml
  src/
    lib.rs
    config.rs
    client.rs
    retriever.rs
    merger.rs
    models.rs
    error.rs
```

Suggested responsibilities:

| Module | Responsibility |
|---|---|
| `config.rs` | Backend registry, routing rules, weights, timeouts |
| `client.rs` | HTTP/MCP clients for remote RAG instances |
| `retriever.rs` | Parallel fan-out and high-level search orchestration |
| `merger.rs` | Score normalization, dedupe, ranking, diversity |
| `models.rs` | Federated request/response models and backend status |
| `error.rs` | Federation-specific error types |

This keeps `rag-core` focused on single-corpus indexing and retrieval.

---

## Configuration

Example `rag.toml` section:

```toml
[federation]
enabled = true
per_backend_k = 10
final_k = 12
timeout_ms = 2500
merge_strategy = "relative_score"
partial_results = true
rerank = false

[[federation.backends]]
id = "finance"
name = "Finance SharePoint"
url = "https://finance-rag.internal"
kind = "rust-rag-http"
weight = 1.0
enabled = true

[[federation.backends]]
id = "engineering"
name = "Engineering Docs"
url = "https://engineering-rag.internal"
kind = "rust-rag-http"
weight = 1.0
enabled = true

[[federation.backends]]
id = "legal"
name = "Legal Policies"
url = "https://legal-rag.internal"
kind = "rust-rag-http"
weight = 1.2
enabled = true
```

Useful backend options:

| Option | Meaning |
|---|---|
| `id` | Stable backend identifier |
| `name` | Human-readable backend name |
| `url` | Remote server base URL |
| `kind` | Client implementation to use |
| `weight` | Optional ranking bias |
| `enabled` | Whether to query this backend |
| `timeout_ms` | Optional backend-specific timeout |
| `tenant_id` | Optional tenant routing metadata |
| `source_types` | Optional source categories exposed by this backend |

---

## Request Flow

```text
1. Receive query, k, filters, and caller context.
2. Select eligible backends.
3. Send query to selected backends in parallel.
4. Each backend runs its own retrieval and permission filtering.
5. Collect responses and backend statuses.
6. Normalize result scores.
7. Dedupe overlapping results.
8. Apply backend weights and diversity rules.
9. Optionally rerank merged candidates.
10. Return final top k with citations and backend metadata.
```

The federation layer should preserve the original query. It may later integrate
with query rewriting and expansion, but the first version should be simple:
one query fans out to many backends.

---

## Response Shape

Federated results should extend normal `SearchResult` data with origin
metadata.

Example:

```json
{
  "query": "customer travel reimbursement field engineers",
  "results": [
    {
      "backend_id": "legal",
      "backend_name": "Legal Policies",
      "chunk_id": "chunk_456",
      "document_id": "doc_123",
      "source_id": "legal-sharepoint",
      "title": "Travel Reimbursement Policy",
      "snippet": "Field engineers attending customer workshops may claim...",
      "score": 0.84,
      "normalized_score": 0.91,
      "page": 4,
      "citation": {
        "label": "Travel Reimbursement Policy, p. 4",
        "url": "https://sharepoint.example.com/legal/travel-policy.pdf"
      }
    }
  ],
  "backends": [
    {
      "backend_id": "legal",
      "status": "ok",
      "result_count": 8,
      "latency_ms": 93
    },
    {
      "backend_id": "engineering",
      "status": "timeout",
      "result_count": 0,
      "latency_ms": 2500,
      "error": "backend timed out"
    }
  ],
  "partial": true
}
```

Backend status metadata is important. Users and agents need to know whether the
answer was based on all available corpora or only a subset.

---

## Permission Model

Each backend should enforce its own permissions.

```text
caller_context -> finance RAG -> finance permission filter
caller_context -> legal RAG   -> legal permission filter
caller_context -> eng RAG     -> eng permission filter
```

The federation layer passes caller context through but does not become the
global permission authority.

This is safer because:

- each backend may have different source-native ACL rules
- tenants may use different identity systems
- some backends may have stricter filtering than others
- remote instances can evolve their permission logic independently

The federation layer can still enforce coarse routing:

- which backends the caller is allowed to query
- which tenants the caller can access
- whether mutation tools are blocked
- whether partial results are allowed

But fine-grained document and chunk visibility should stay with the backend.

---

## Score Normalization

Raw scores from different backends are not directly comparable.

Reasons:

- backends may use different vector stores
- backends may use different embedding models
- some stores return cosine similarity while others return distance
- one corpus may be much narrower than another
- hybrid search scores may combine BM25 and vector similarity

A raw score of `0.82` from one backend may not mean the same thing as `0.82`
from another backend.

### First Strategy: Rank-Based Normalization

Simple and robust:

```text
normalized_score = 1.0 - ((rank - 1) / per_backend_k)
```

If a backend returns 10 results:

| Rank | Normalized score |
|---|---|
| 1 | 1.0 |
| 2 | 0.9 |
| 5 | 0.6 |
| 10 | 0.1 |

This avoids trusting incompatible raw score scales.

### Second Strategy: Relative Score Normalization

Use the score range within each backend response:

```text
normalized_score = (score - min_backend_score) / (max_backend_score - min_backend_score)
```

This can work well, but it is sensitive when a backend returns a narrow score
range.

### Later Strategy: Reranking

The best long-term approach is to retrieve candidates from each backend, then
run a shared reranker over the merged set. That reranker can compare all
candidates against the query using the same scoring model.

---

## Result Merging

The merger should:

1. group results by backend
2. normalize scores per backend
3. apply backend weights
4. dedupe repeated results
5. prefer document/source diversity when scores are close
6. optionally rerank the final candidate pool
7. return the final top `k`

### Dedupe Keys

Possible dedupe keys:

- `backend_id + chunk_id`
- `backend_id + document_id + chunk_index`
- source URL + page + text hash
- canonical document URL + chunk text hash

The first implementation should dedupe exact backend/chunk IDs and optionally
dedupe obvious cross-backend duplicates by URL and text hash.

### Diversity

Without diversity rules, one backend or one long document can dominate the
result list. The merger can improve usefulness by limiting near-duplicate
results:

- max results per backend
- max results per document
- small bonus for new backend/source coverage
- small penalty for repeated document IDs

These should be configurable.

---

## Failure Behavior

Federated search should be partial-result friendly.

| Situation | Behavior |
|---|---|
| One backend times out | Return other results with backend warning metadata |
| One backend returns an error | Mark that backend failed and continue if `partial_results = true` |
| Caller has no access to backend | Treat as skipped or zero results |
| Backend returns malformed data | Drop backend result, mark failed, log details |
| All backends fail | Return an error |
| Backend is slow | Respect per-backend timeout |

The response should clearly indicate whether results are partial.

---

## API Surface

There are two reasonable MCP approaches.

### Option A: New Tool

Add:

```text
rag_federated_search
```

Pros:

- clear behavior
- no surprise fan-out from normal `rag_search`
- easier to add federation-specific metadata

Cons:

- one more tool for clients to learn

### Option B: Extend `rag_search`

Add optional input:

```json
{
  "query": "travel reimbursement field engineers",
  "k": 12,
  "federation": true,
  "backend_ids": ["finance", "legal", "engineering"]
}
```

Pros:

- smaller tool surface
- clients can switch behavior with one flag

Cons:

- `rag_search` becomes more complex
- response shape may need backend status metadata

Recommendation: start with `rag_federated_search` so the behavior is explicit.
Later, `rag_search` can route to federation based on config or request flags.

---

## Security and Operations

Federation introduces service-to-service trust.

Required controls:

- backend allowlist
- per-backend authentication
- TLS for remote calls
- request timeouts
- max fan-out limits
- caller context forwarding rules
- audit logs for which backends were queried
- trace IDs across the federation call and backend calls
- clear handling for partial results

The federation layer should never forward secrets or raw credentials unless a
backend explicitly requires them and the deployment has approved that model.
Prefer signed service credentials plus structured caller context.

---

## Observability

Federated search should emit:

- total request latency
- per-backend latency
- per-backend result count
- per-backend status
- timeout count
- error count
- partial-result count
- final merge count
- reranker latency when enabled

Trace IDs should connect:

```text
MCP call -> federated retriever -> backend request -> backend retriever -> vector store
```

This is essential when users ask why a result was missing.

---

## Implementation Plan

### Milestone A: Models and config

- Add `rag-federation` crate.
- Define backend config.
- Define federated request/response models.
- Add config validation.

### Milestone B: HTTP client

- Implement a client for `rag-server` search endpoints.
- Support caller context forwarding.
- Support per-backend timeout.
- Add tests with mock HTTP servers.

### Milestone C: Parallel fan-out

- Query selected backends concurrently.
- Collect results and backend statuses.
- Support `partial_results = true`.

### Milestone D: Merge and normalize

- Add rank-based normalization.
- Add relative-score normalization.
- Add backend weights.
- Add dedupe by backend/chunk ID and URL/text hash.

### Milestone E: MCP exposure

- Add `rag_federated_search`.
- Return merged results and backend status metadata.
- Add integration tests against in-memory or mock backends.

### Milestone F: Reranking and routing

- Add optional reranker over merged candidates.
- Add backend routing rules by tenant, source type, or request filters.
- Add query expansion support after the basic federation path is stable.

---

## Open Questions

- Should federation require the HTTP API, or should it also support MCP clients?
- Should the federation layer live inside `rag-mcp`, `rag-server`, or a new
  binary?
- How should backend authentication be configured?
- Should backends advertise their embedding model and score semantics?
- Should cross-backend dedupe use URL, document hash, or canonical source IDs?
- Should federation support streaming partial results?
- Should failures be visible to end users or only to operators?

---

## Summary

Federated search fans one query out to multiple independent RAG systems, then
normalizes, dedupes, and merges their citation-ready results.

The clean implementation is a new orchestration layer:

```text
single query -> parallel backend searches -> normalize -> dedupe -> optional rerank -> final results
```

Each backend keeps control of its own data, indexing, vector store, and
permissions. The federation layer only coordinates, merges, and explains which
backends contributed to the answer.
