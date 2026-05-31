# Parity Check — Milestones 1–4

Performed after v0.4.0 against the reference set defined in
[docs/rag-parity-checking.md](rag-parity-checking.md).

---

## M1 — Core Scaffold
**References:** Haystack, LlamaIndex, LangChain

### What we have

```
Connector → Chunker → Embedder → VectorStore
                                     ↓
Retriever ← PermissionFilter ← StandardRetriever
                                     ↓
                               SearchResult + Citation
```

All seven core traits (`Connector`, `Chunker`, `Embedder`, `VectorStore`,
`Retriever`, `PermissionFilter`, `Reranker`) are defined. One implementation of
each ships: `ParagraphChunker`, `MockEmbedder`, `MemoryVectorStore`,
`StandardRetriever`.

### Gaps found

| Gap | Reference | Decision |
|-----|-----------|----------|
| `ParagraphChunker` splits only on `\n\n`; no sentence, recursive, or token-aware splitting | LangChain `RecursiveCharacterTextSplitter`, Haystack `SentenceSplitter` | **Defer** — add `RecursiveChunker` and `MarkdownChunker` to parking lot |
| `ChunkMetadata.section` field exists but is never populated by `ParagraphChunker`; section must come from extension workers | LlamaIndex `TextNode.relationships` | **Adopt** — `ParagraphChunker` should detect `## Heading` lines and populate `section` |
| `SearchFilter` only supports `source_ids`, `document_ids`, `content_types`; no field-level metadata operators | LlamaIndex `MetadataFilters` (eq/gt/lt/in/and/or) | **Defer to M5** — MCP tools will expose a small subset; full operator tree can follow in M7 |
| `embed_query` defaults to `embed_texts(&[query])`; asymmetric embedding models use different models for query vs document | Haystack `SentenceTransformersTextEmbedder` supports asymmetric models | **Adapt in M6** — add `embed_query_mode: QueryMode` to `Embedder` config, defaulting to symmetric |
| No hybrid (vector + keyword) search path | Haystack `BM25Retriever + EmbeddingRetriever` fusion, txtai weighted hybrid | **Defer** — parking lot item; pgvector supports `ts_rank` alongside `<=>` |
| Pipeline is linear; complex branching (multi-source fan-out, conditional routing) requires custom code | Haystack DAG pipeline | **Out of scope** — linear pipeline matches the MCP tool surface; complex routing is for orchestrators |
| `Reranker` trait present but no concrete implementation until M6 | Haystack `CohereRanker`, LlamaIndex `LLMRerank` | **On track** — Zenoh `ZenohReranker` ships in M4; production cross-encoder in M6 |

### Confirmed strengths over references

- **`PermissionFilter` is first-class**: Haystack and LlamaIndex have no built-in per-caller ACL enforcement. Ours runs post-retrieval with overfetch to guarantee `k` results after filtering.
- **`Citation` is a named type**: All three reference projects return raw `Document` objects; formatting citations for LLM responses is left to the caller. Our `Citation.label` and `Citation.url` are ready to embed in prompts immediately.
- **`CallerContext`** with `user_id`, `groups`, `tenant_id` is more structured than anything in the reference set.

### Action items

- Add `section`-aware detection to `ParagraphChunker` (detect `## heading` lines, set `ChunkMetadata.section`).
- Add to parking lot: `RecursiveChunker`, `MarkdownChunker`.
- Note in M6 plan: `Embedder` config needs an `embed_query_mode` field for asymmetric models.

---

## M2 — pgvector Store
**References:** Haystack, LlamaIndex, LangChain, txtai

### What we have

`PgVectorStore` implements `VectorStore` via PostgreSQL + pgvector:
- HNSW index (`vector_cosine_ops`) created automatically at connect time
- Upsert via `ON CONFLICT (id) DO UPDATE`
- Dimension-mismatch guard in `rag_meta`
- Source-scoped and document-scoped delete
- Inspection helpers: `chunk_counts_by_source`, `last_indexed_at`
- Cosine similarity returned as score in `[−1, 1]` (clamp to `[0, 1]` is caller's responsibility)

### Gaps found

| Gap | Reference | Decision |
|-----|-----------|----------|
| `SearchFilter` has no general metadata field operators; filtering by `author`, `modified_at`, `content_type` requires a manual SQL join on the JSON `metadata` column | LlamaIndex `PGVectorStore` with MetadataFilters; LangChain `PGVector` filter dict | **Defer to M7** — add metadata columns for `modified_at`, `content_type`, `source_id` as indexed columns in a new migration; keep the JSON blob for extensible extra fields |
| Score range is `[−1, 1]`; callers expect `[0, 1]` for similarity | LlamaIndex, LangChain both normalize to `[0, 1]` | **Fix in M5** — clamp to `max(0, score)` in `PgVectorStore.search` to prevent negative scores confusing MCP tool users; pure cosine scores below 0 signal orthogonal/opposite content, which is equivalent to "no match" |
| No Maximal Marginal Relevance (MMR) diversification | LangChain `PGVector.max_marginal_relevance_search` | **Defer** — parking lot item |
| No sparse (BM25/keyword) retrieval path | txtai weighted hybrid, Haystack `BM25Retriever` | **Defer** — parking lot; pgvector + `ts_rank` can implement this without a separate service |
| `count_chunks()` is a full-table count with no filter; slow on large corpora | Haystack `count_documents(filters=…)` | **Defer** — add filtered count to `VectorStore` trait in M7 |

### Confirmed strengths over references

- **Automatic HNSW index creation**: LangChain's `PGVector` does not create an HNSW index by default; users must run DDL manually. We create it at `connect()` time.
- **Dimension guard**: Connecting with the wrong dimension is an error, not a silent type mismatch. None of the reference stores do this.
- **`last_indexed_at`**: LlamaIndex and LangChain have no built-in "when was this source last synced?" query.

### Action items

- Clamp score to `max(0.0, score)` in `PgVectorStore.search` before returning.
- Add migration 0002 in M7 with indexed columns for `modified_at`, `content_type` to enable efficient metadata filtering.
- Add to parking lot: MMR diversification, filtered `count_chunks`.

---

## M3 — SharePoint Connector + Document Parsing
**References:** RAGFlow, LlamaIndex, Dify

### What we have

`SharePointConnector`: `list_documents`, `load_document` (base64 decode, binary routing to extension workers), `changes_since` (change token), `max_file_bytes` guard, sha256 stable IDs.

`FilesystemConnector`: recursive walk, UTF-8 files, binary routing.

`rag-extension-protocol`: `LoadDocumentResponse` returns `text`, `sections[]` (each with `title?`, `text`, `page?`), `page_count?`, `metadata`.

### Gaps found

| Gap | Reference | Decision |
|-----|-----------|----------|
| `DocumentSection` has no layout metadata: no bounding box, column hint, visual type (table, figure, header, footer) | RAGFlow detects columns, tables, figures from PDF layout; returns bounding-box per block | **Adapt** — add `layout_hints: Option<LayoutHints>` to `DocumentSection` in `rag-extension-protocol`; extension workers can populate it; callers that don't need it ignore it |
| No chunk quality scoring: all text returned by extension workers is accepted; boilerplate (headers, footers, running titles) is not filtered | RAGFlow discards low-quality blocks; Dify lets users enable/disable segments | **Defer to M7** — add a quality threshold to `Indexer` config; filter out chunks below a length/entropy threshold |
| `Connector.list_documents` returns all files in one batch; no pagination or cursor for very large libraries | LlamaIndex `SimpleDirectoryReader` iterates lazily | **Defer** — add `list_documents_paginated(cursor)` to the trait in M7; the sync path via `changes_since` is already incremental |
| `DocumentMetadata` is not populated from SharePoint list item fields (author, created_at, version) | LlamaIndex SharePointReader attaches Graph API user metadata | **Defer to M5** — Graph API user metadata requires additional SharePoint REST calls; add as optional enrichment in M5 |
| No LlamaIndex-style node relationships (PREVIOUS / NEXT / SOURCE) — adjacent chunks cannot be linked | LlamaIndex `TextNode.relationships[NEXT/PREV]` | **Defer** — add `prev_chunk_id: Option<ChunkId>` and `next_chunk_id: Option<ChunkId>` to `ChunkMetadata` in M5; callers can use them for context window expansion |
| No Dify-style per-segment enabled/disabled toggle; all indexed content is always searchable | Dify segment enable/disable | **Defer** — add `active: bool` to `ChunkMetadata` and a filter on `VectorStore.search` in M7 |

### Confirmed strengths over references

- **Incremental sync via change tokens**: RAGFlow re-indexes entire folders on update; Dify has no source-native change feed. Our `changes_since(token)` is the only production-grade incremental sync in the comparison set.
- **Stable `DocumentId` from sha256**: Re-indexing the same file always upserts the same chunks. None of the reference projects guarantee idempotent IDs.
- **Permission hints at index time**: ACL strings are attached to each chunk's `ChunkMetadata.permissions` at index time and enforced at retrieval time by `PermissionFilter`. Dify and AnythingLLM have no per-document ACL model.
- **Binary format routing is explicit**: The connector returns a clear error for PDF/DOCX rather than silently returning empty content, making it obvious when an extension worker is needed.

### Action items

- Add `layout_hints: Option<LayoutHints>` struct to `rag-extension-protocol` for M5 extension worker payloads.
- Add `prev_chunk_id` / `next_chunk_id` to `ChunkMetadata` in M5.
- Add to parking lot: per-segment active flag, chunk quality scoring, paginated `list_documents`.

---

## M4 — Zenoh Extension Bus
**References:** Haystack components, LangChain Runnables, Dify external knowledge API, Flowise

### What we have

`rag-zenoh`: `ExtensionRegistry` (auto-discover via announce/heartbeat, watchdog eviction), `ZenohCaller` (request/reply for load_document/embed_texts/rerank), `EventPublisher` (6 lifecycle events), `ZenohEmbedder`/`ZenohReranker`/`ZenohDocumentLoader` trait implementations.

`python/rag_worker_sdk`: `DocumentLoaderWorker`, `EmbedderWorker`, `RerankerWorker` base classes; subclass + implement one method + call `run()`.

### Gaps found

| Gap | Reference | Decision |
|-----|-----------|----------|
| Single worker per capability type; no load-balancing across multiple registered workers for the same content type | Haystack runs N parallel component instances; LangChain batch routing | **Defer to M7** — add round-robin selection in `ExtensionRegistry.find_loader_for` when multiple workers handle the same content type |
| `supports_streaming: bool` is in `CapabilityDescriptor` but Zenoh transport does not implement streaming replies | LangChain Runnable `stream()` method; SSE in LangServe | **Defer** — add streaming via Zenoh subscriber on a reply key (`rag/call/{id}/load/stream/{req_id}`) in M7 |
| No batch routing: embed_texts sends one batch per call; there is no fan-out to split a large batch across multiple embedder workers | LangChain `RunnableParallel`, Haystack pipeline parallelism | **Defer** — add batch-split routing in `ZenohCaller.embed_texts` in M6 when production embedder is wired in |
| Python SDK relies on synchronous `zenoh.open()` and a background thread for heartbeats; no `async`/`asyncio` support | Haystack async components; LangChain async `Runnable.ainvoke()` | **Adapt in M8** — add an async variant of each base class using `asyncio` with zenoh's Python async API |
| No payload size enforcement before sending: `max_payload_bytes` is declared but not checked by the caller | Haystack validates component input schemas with Pydantic | **Fix in M5** — add a check in `ZenohCaller.load_document` that `data_base64.len() ≤ descriptor.max_payload_bytes` before sending |
| Flowise comparison: Flowise nodes are in-process, UI-configurable, JavaScript/TypeScript — fundamentally incompatible architecture | Flowise | **Out of scope** — our extension workers are out-of-process, transport-neutral, language-agnostic; this is an intentional architectural difference |

### Why this project's worker bus is architecturally distinct

The reference projects embed all processing in the same process and runtime:
- **Haystack**: Python components run in the pipeline process; remote components require custom code.
- **LangChain Runnables / LangServe**: Deploy as HTTP services; no auto-discovery.
- **Flowise**: JavaScript nodes run in the Node.js server process.

This project makes different tradeoffs deliberately:
- **Language-agnostic**: A Python PDF worker and a Rust embedder coexist on the same bus.
- **Auto-discovery**: Workers register and deregister without restarting the runtime.
- **Fault isolation**: A crashing PDF worker does not crash the Rust indexer.
- **Scale-out**: Multiple workers of the same type can run on separate machines.

Dify's external knowledge API is the closest conceptual match (an HTTP endpoint the platform calls), but it only supports search, not document parsing or embedding. Our MCP layer (M5) will expose a similar clean API to external callers on top of the bus.

### Confirmed strengths over references

- **Zero-config worker discovery**: Unlike LangChain (explicit URL) or Haystack (explicit pipeline config), workers announce themselves and are immediately usable.
- **Watchdog eviction**: A crashed worker is removed from the registry within `max_missed_heartbeats × heartbeat_interval_secs` without any operator intervention.
- **mTLS transport security**: Haystack, LangChain, Dify — none offer built-in mTLS for inter-component calls.
- **Python SDK requires ~15 lines to write a working PDF loader**: Lower than any comparable framework.

### Action items

- Add payload size check in `ZenohCaller` before sending (max_payload_bytes guard).
- Add to parking lot: async Python SDK, round-robin worker selection, streaming reply transport.
- Mark `Flowise` as out of scope for future parity checks.

---

## Cross-Cutting Findings

These gaps appeared in multiple milestone checks and should be tracked together:

| Finding | Affects | Action |
|---------|---------|--------|
| `ChunkMetadata` needs `prev_chunk_id` / `next_chunk_id` for context window expansion | M1 (chunks), M3 (connector), M5 (MCP tools) | Add in M5 |
| `SearchFilter` needs metadata field operators for production queries | M1 (trait design), M2 (store), M5 (MCP tools) | Add minimal operators (date range, content_type list) in M5; full operator tree in M7 |
| Score normalization to `[0, 1]` needed | M2 (pgvector), M5 (MCP tool results) | Fix `PgVectorStore.search` score clamp in M5 |
| Indexing job status model needed | M3 (connector), M4 (events), M5 (MCP tools) | Add `IndexJobStatus` enum and `Indexer.status()` query in M5 |
| `DocumentSection.layout_hints` for rich parsing | M3 (connector), M4 (extension protocol) | Add struct to extension protocol in M5 |

---

## Parking Lot Additions (from this review)

These items do not fit the current milestone but are worth building:

- **`RecursiveChunker`** — tries paragraph → sentence → word boundaries in sequence; better than `\n\n` split for varied document formats
- **`MarkdownChunker`** — splits on heading levels, preserves section tree in `ChunkMetadata.section`
- **Hybrid search** (BM25 + vector, pgvector `ts_rank` + `<=>`) — already in parking lot; confirmed as real gap by M2 review
- **MMR diversification** — penalizes near-duplicate results; relevant for corpora with many similar documents
- **Filtered `count_chunks(filter)`** — operational monitoring needs per-source counts efficiently
- **Streaming worker replies** — `supports_streaming` is declared; wire transport
- **Round-robin worker selection** — multiple workers for same content type, load-balanced
- **Async Python SDK** — asyncio-native base classes for high-throughput workers
- **Chunk quality scoring** — filter boilerplate at index time using length + entropy heuristics
- **Per-chunk `active` flag** — allow operators to disable specific chunks without re-indexing
