# Changelog

All notable changes to this project are documented here.

---

## [0.2.0] — pgvector Store

### Added

- **`crates/rag-store-pgvector/migrations/0001_initial.sql`** — base schema migration:
  - `rag_meta` table: key/value store for runtime configuration; used to persist the embedding dimension and detect dimension mismatches at startup before any insert is attempted.
  - `rag_chunks` table: `id`, `document_id`, `source_id`, `chunk_index`, `text`, `metadata JSONB`, `created_at`. The `embedding` column is absent from the migration because its type (`vector(N)`) encodes the dimension, which is only known at runtime.
  - Indexes on `document_id` and `source_id` for efficient delete operations.

- **`crates/rag-store-pgvector/src/lib.rs`** — `PgVectorStore` implementing `rag_core::traits::VectorStore`:
  - `PgVectorStore::connect(url, dimension)` — creates the pool, runs embedded migrations, creates the pgvector extension if absent, writes/validates the dimension in `rag_meta` (returns `RagError::Store` on mismatch), adds the `embedding vector(N)` column via `ALTER TABLE ... ADD COLUMN IF NOT EXISTS`, and creates the HNSW index with `vector_cosine_ops` operator class.
  - Vectors are encoded as pgvector text literals (`[1.0,0.0,…]`) and cast in SQL via `$1::vector(N)`. This avoids version coupling between the `pgvector` crate's sqlx feature and the `sqlx` minor version in use.
  - `upsert_chunks` — wraps all inserts for a batch in a single transaction; uses `ON CONFLICT (id) DO UPDATE` to overwrite stale chunks on re-index.
  - `search(embedding, k, filter)` — cosine distance query `embedding <=> $1::vector(N)` with `ORDER BY … LIMIT k`; score returned as `1.0 - cosine_distance` (range -1 to 1). Optional `SearchFilter` applied via `= ANY($n::text[])` with an `array_length IS NULL` guard to skip empty filters.
  - `delete_by_document` / `delete_by_source` — `DELETE WHERE` on the relevant column; no orphan rows.
  - `count_chunks` — `SELECT COUNT(*)::bigint`.
  - `chunk_counts_by_source()` — `GROUP BY source_id` aggregate for index inspection.
  - `last_indexed_at(source_id)` — `MAX(created_at)` for a source; used to report last-sync time.

- **`crates/rag-store-pgvector/tests/integration.rs`** — 7 integration tests; all are silently skipped when `TEST_DATABASE_URL` is not set:
  - `upsert_and_count` — inserts 3 chunks across 2 documents, asserts `count_chunks >= 3`.
  - `search_returns_closest_chunk` — one-hot vectors; asserts the aligned chunk scores ≈ 1.0 and ranks first.
  - `search_with_source_filter` — two sources with identical embeddings; filter returns only the requested source.
  - `delete_by_document_removes_only_that_document` — deletes one document's chunks; the other document's chunk remains.
  - `upsert_updates_existing_chunk` — same `ChunkId` re-upserted with new text and embedding; search reflects the update.
  - `dimension_mismatch_is_rejected` — second `connect()` with a different dimension returns `Err` containing "mismatch".
  - `chunk_counts_by_source_and_last_indexed` — verifies `chunk_counts_by_source()` and `last_indexed_at()`.

- **`docker-compose.yml`** — `pgvector/pgvector:pg16` service with healthcheck, named volume, and default credentials (`rag`/`rag_password`/`rag_dev`). Run with `docker compose up -d` before integration tests.

- **`crates/rag-mcp/src/config.rs`** — `RagConfig` struct loaded from `rag.toml` at server startup:
  - `StoreConfig { backend: StoreBackend, database_url: Option<String> }` — backends: `memory` (default), `pgvector`.
  - `EmbedderConfig { provider: EmbedderProvider, dimension: usize, openai_model, model_path }` — providers: `mock` (default), `openai`, `local-onnx`.
  - `RagConfig::validate()` — returns a human-readable `Err(String)` for missing `database_url` when backend is pgvector, missing `OPENAI_API_KEY` env var when provider is openai, and missing `model_path` when provider is local-onnx. 3 unit tests cover the validation logic.

### Changed

- `crates/rag-store-pgvector/Cargo.toml` — added `sqlx = "0.8"` (postgres, runtime-tokio-rustls, uuid, chrono, json, migrate features) and `pgvector = "0.4"` (text-format helper only; sqlx feature intentionally omitted to avoid version coupling).
- `crates/rag-mcp/src/lib.rs` — added `pub mod config` declaration.
- Version bumped to 0.2.0.

---

## [0.1.0] — Core Scaffold

### Added

- **Cargo workspace** — 7-crate workspace (`crates/rag-core`, `crates/rag-store-pgvector`, `crates/rag-connectors`, `crates/rag-extension-protocol`, `crates/rag-zenoh`, `crates/rag-mcp`, `crates/rag-server`) with workspace-level dependency pinning for `tokio`, `async-trait`, `serde`, `serde_json`, `anyhow`, `thiserror`, `chrono`, `tracing`, `tracing-subscriber`, and `uuid`.

- **`crates/rag-core` — domain models** (`src/models.rs`):
  - ID newtypes: `SourceId`, `DocumentId`, `ChunkId` — each wraps `String`, generates via `Uuid::new_v4()`, implements `Display`, `Hash`, `Eq`, `Serialize`, `Deserialize`.
  - `Source` — registered document source with `id`, `name`, `kind`, `base_url`, and `config: serde_json::Value`.
  - `DocumentRef` — lightweight discovery reference (id, source\_id, title, url, modified\_at, content\_type, size\_bytes) returned by connector list passes before full loading.
  - `Document` + `DocumentMetadata` — fully loaded document with extracted plain-text `content`, author, timestamps, version, file type, page count, ACL permission hints, and `#[serde(flatten)]` extra fields.
  - `Chunk` + `ChunkMetadata` — text span with zero-based `chunk_index`, optional `embedding: Vec<f32>`, document title, URL, page number, section, and permission hints.
  - `ScoredChunk` — chunk paired with cosine similarity score from vector search.
  - `SearchResult` — citation-ready result with chunk\_id, document\_id, source\_id, title, source\_url, snippet, score, page, chunk\_index, modified\_at, and a `Citation` struct.
  - `Citation::build(title, page, url)` — produces `"Title, p. N"` label when a page number is present.
  - `CallerContext` — user\_id, groups, tenant\_id, and extra fields passed to permission filters.
  - `SearchFilter::by_source(source_id)` convenience constructor; filters on source\_ids, document\_ids, and content\_types.

- **`crates/rag-core` — core traits** (`src/traits.rs`):
  - `Connector` — `source_id()`, `kind()`, `list_documents()`, `load_document(doc_ref)`, `changes_since(token)` returning `(Vec<ChangeEvent>, new_token)`. `ChangeEvent` carries `DocumentRef`, `ChangeKind` (Created/Modified/Deleted), and `occurred_at`.
  - `Chunker` — `name()`, `chunk(document)`.
  - `Embedder` — `name()`, `dimension()`, `embed_texts(texts)`, `embed_query(query)` (default impl calls `embed_texts` with single element).
  - `VectorStore` — `upsert_chunks`, `search(embedding, k, filter)`, `delete_by_document`, `delete_by_source`, `count_chunks`.
  - `Retriever` — `search(query, k, filter, caller)`.
  - `PermissionFilter` — `filter(caller, chunks)`.
  - `Reranker` — `rerank(query, candidates)`.
  - All traits are `async_trait`, `Send + Sync`.

- **`crates/rag-core` — `ParagraphChunker`** (`src/chunker.rs`) — splits documents on `\n\n` paragraph boundaries, accumulates paragraphs up to `max_chars` (default 1500), and carries the last `overlap_chars` (default 200) from the previous chunk into the next to preserve cross-boundary context. Flushes a final chunk after the last paragraph.

- **`crates/rag-core` — `MockEmbedder`** (`src/embedder.rs`) — deterministic 384-dimensional embedder using `DefaultHasher` with LCG mixing constant (`wrapping_mul(6364136223846793005)`) to avoid integer overflow in debug builds. Produces L2-normalised vectors; same text always yields the same vector, enabling reproducible test assertions.

- **`crates/rag-core` — `MemoryVectorStore`** (`src/store.rs`) — thread-safe in-memory vector store backed by `Arc<RwLock<Vec<Entry>>>`. Cosine similarity computed as `dot / (norm_a * norm_b)`. `search` applies `SearchFilter` before scoring, sorts descending by score, and truncates to `k`. `upsert_chunks` evicts any existing entry with the same `ChunkId` before inserting.

- **`crates/rag-core` — `Indexer`** (`src/indexer.rs`) — orchestrates `Chunker::chunk` → `Embedder::embed_texts` (batch) → `VectorStore::upsert_chunks`. Exposes `index_document(doc) -> usize` returning chunk count, `delete_document(id)`, and `delete_source(id)`.

- **`crates/rag-core` — `StandardRetriever`** (`src/retriever.rs`) — embeds the query, overfetches `k * 3` candidates when a `PermissionFilter` is attached (to compensate for filtered-out results), applies the filter when `caller` is provided, takes the first `k` after filtering, and maps each `ScoredChunk` to a `SearchResult` via `Citation::build`.

- **`crates/rag-core` — `RagError`** (`src/error.rs`) — `thiserror`-derived enum with variants `Connector`, `Chunker`, `Embedder`, `Store`, `Retriever`, `Permission`, `Extension`, `Serialization(#[from] serde_json::Error)`, and `Other(#[from] anyhow::Error)`.

- **`crates/rag-core` — 3 integration tests** (`tests/integration.rs`):
  - `index_and_search_returns_results` — indexes a 7-paragraph document through the full pipeline and asserts non-empty results with a populated citation label, source URL, and score in `[-1.0, 1.0]`.
  - `delete_document_removes_chunks` — indexes then deletes; asserts `count_chunks()` returns 0.
  - `citation_includes_page_when_present` — unit test for `Citation::build` label formatting with and without a page number.

- **`crates/rag-extension-protocol`** (`src/lib.rs`) — transport-neutral `rag.extension.v1` protocol:
  - `CapabilityDescriptor` — extension\_id, protocol\_version, `Vec<ExtensionCapability>`, content\_types, max\_payload\_bytes, supports\_streaming.
  - `ExtensionCapability` enum — `LoadDocument`, `EmbedTexts`, `Rerank`, `ApplyAcl`, `SummarizeContext`, `Transform`; serde `rename_all = "snake_case"`.
  - `RequestEnvelope::new(operation, payload, context)` — auto-generates `request_id` via `Uuid::new_v4()` and stamps `protocol = "rag.extension.v1"`.
  - `ResponseEnvelope::ok(request_id, payload)` and `ResponseEnvelope::err(request_id, message)` constructors.
  - `RequestContext` — tenant\_id, user\_id, trace\_id forwarded to workers for audit and ACL use.
  - `Heartbeat` + `WorkerStatus` (Ready/Busy/Draining).
  - `LoadDocumentRequest` / `LoadDocumentResponse` / `DocumentSection` — typed payloads for the `load_document` operation; raw bytes passed as `data_base64`.
  - `EmbedTextsRequest` / `EmbedTextsResponse` — typed batch embedding payloads.
  - Zenoh keyspace convention documented in module-level doc comment.

- **Stub crates with implementation notes:**
  - `rag-store-pgvector` — Phase 2 placeholder; doc comment details `sqlx` dependency, migration schema, HNSW index, and `<->` cosine-distance operator.
  - `rag-connectors` — `SharePointConnector` struct with config fields (`site_url`, `library_path`, `include_extensions`, `max_file_bytes`); all three `Connector` methods return `Err(RagError::Connector("Phase 3"))` with inline notes on which SharePoint REST tools to call; feature-gated behind `sharepoint` Cargo feature. `FilesystemConnector` planned.
  - `rag-zenoh` — Phase 4 placeholder; doc comment describes Zenoh keyspace (`rag/extensions/{id}/announce`, `rag/call/{id}/load`, `rag/events/**`) and planned `ZenohExtensionRegistry`.
  - `rag-mcp` — `[[bin]] rag-mcp` entry point with `tracing-subscriber` initialisation; lib.rs documents all 7 planned MCP tools (`rag_search`, `rag_get_document`, `rag_get_context`, `rag_index_source`, `rag_sync_source`, `rag_list_sources`, `rag_explain_match`) and `RAG_READ_ONLY` guard.
  - `rag-server` — Phase 7 placeholder; doc comment notes `axum` and migration path from Python FastAPI RAG service.

- **`.cargo/config.toml`** — sets `LIB` and `INCLUDE` environment variables for MSVC 14.51 and Windows SDK 10.0.26100.0 when building outside a VS Developer Prompt. Variables are only applied when not already set in the environment; harmless on Linux and macOS where the MSVC toolchain is not used.

- **`.gitattributes`** — `* text=auto` with `eol=lf` overrides for `.rs`, `.toml`, `.md`, and `.sh` files; binary attributes for `.png`, `.jpg`, `.ico`, `.wasm`.

- **`LICENSE`** — Apache License, Version 2.0.

- **`NOTICE`** — full third-party attribution for ~100 transitive dependencies organised by license family: MIT OR Apache-2.0, MIT-only, Apache-2.0 WITH LLVM-exception, MIT OR Apache-2.0 OR LGPL-2.1-or-later (`r-efi`; Apache-2.0 chosen), Unlicense OR MIT, (MIT OR Apache-2.0) AND Unicode-3.0 (`unicode-ident`), and Zlib (`foldhash`). Includes a planned future dependency table with anticipated licenses for `sqlx`, `zenoh`, `axum`, `rmcp`, `ort`, `PyO3`, `opentelemetry`, and `prometheus`.

- **`README.md`** — ASCII architecture diagram, crate overview table with phase status, quick-start `main.rs` example, MCP tool reference table with descriptions, platform support matrix (Linux/macOS Tier 1, Windows Tier 2), Windows build note, test commands, license summary, and `cargo about` attribution instructions.

- **`ROADMAP.md`** — 10-milestone plan to v1.0.0 matching the sharepoint\_rest\_api-rs milestone format: current state summary, per-milestone goal + task table (Task | Notes | Status) + acceptance criteria, deferred/out-of-scope section, 14-item parking lot (hybrid search, cross-encoder reranking, multi-modal indexing, Graph connector, alternative vector store adapters, real-time webhook indexing, streaming search, federated search, Git connector, S3 connector, and more), and summary timeline table.

- **`docs/understanding-rag.md`** — primer for developers new to LLMs: what RAG is, the research assistant analogy, text embedding and semantic vs. keyword search, side-by-side MCP-server-only vs. MCP-server-plus-RAG comparison table, full indexing and query pipeline ASCII diagram, and an explanation of why specific connectors are built in rather than relying on a fully generic interface.

- **`rust-rag-mcp-design-roadmap.md`** — original architecture design document covering the library-first direction, MCP as a layer, SharePoint as the first serious connector, Zenoh for external extensions, Python extensions as out-of-process workers, PyO3 for Python consumer bindings, stable protocol before transport optimisation, citation-ready result shape, and permission-aware retrieval design.
