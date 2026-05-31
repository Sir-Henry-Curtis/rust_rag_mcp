# Roadmap — rust-rag-mcp

## Current State (v0.4.0)

7-crate Cargo workspace with four completed milestones. `rag-core` owns the durable domain model and trait surface. `rag-store-pgvector` provides durable PostgreSQL+pgvector storage. `rag-connectors` ships fully-tested `SharePointConnector` and `FilesystemConnector`. `rag-extension-protocol` provides versioned `rag.extension.v1` JSON envelopes including `RerankRequest/Response`. `rag-zenoh` implements the full Zenoh extension bus: `ExtensionRegistry` (announce/heartbeat/watchdog), `ZenohCaller` (request/reply for load_document, embed_texts, rerank), `EventPublisher` (6 indexing lifecycle events), `ZenohEmbedder`/`ZenohReranker`/`ZenohDocumentLoader` rag-core trait implementations, `ZenohConfig` with mTLS and explicit TCP endpoint support. `python/rag_worker_sdk` provides base classes for Python extension workers with a 50-line quickstart, plus working PDF (PyMuPDF) and DOCX (python-docx) loader examples. Two crates are stubbed: `rag-mcp`, `rag-server`. 47 tests pass across the workspace (12 zenoh unit+integration, 21 connector, 7 pgvector, 3 core, 4 config). Apache-2.0 licensed.

**See also:** [README.md](README.md), [docs/understanding-rag.md](docs/understanding-rag.md), [docs/using-rag-in-larger-systems.md](docs/using-rag-in-larger-systems.md), [docs/vectorstore-backend-comparison.md](docs/vectorstore-backend-comparison.md), [docs/multimodal-indexing-design.md](docs/multimodal-indexing-design.md), [docs/query-rewriting-and-conversation-retrieval.md](docs/query-rewriting-and-conversation-retrieval.md), [docs/federated-search-design.md](docs/federated-search-design.md), [docs/ephemeral-chat-rag-mode.md](docs/ephemeral-chat-rag-mode.md), [docs/rag-parity-checking.md](docs/rag-parity-checking.md), [rust-rag-mcp-design-roadmap.md](rust-rag-mcp-design-roadmap.md) (architecture rationale).

**Completed milestones:** M1 (v0.1.0), M2 (v0.2.0), M3 (v0.3.0), M4 (v0.4.0).

---

## Milestone 1 — Core Scaffold ✅ (v0.1.0)

**Goal:** Establish the workspace, define every domain model and core trait, and prove the full indexing and retrieval pipeline works end-to-end without any external dependencies.

| Task | File | Status |
|------|------|--------|
| 7-crate Cargo workspace with workspace-level deps | `Cargo.toml` | ✅ |
| Domain models: `SourceId`, `DocumentId`, `ChunkId`, `Source`, `DocumentRef`, `Document`, `DocumentMetadata`, `Chunk`, `ChunkMetadata`, `ScoredChunk`, `SearchResult`, `Citation`, `CallerContext`, `SearchFilter` | `crates/rag-core/src/models.rs` | ✅ |
| Core traits: `Connector`, `Chunker`, `Embedder`, `VectorStore`, `Retriever`, `PermissionFilter`, `Reranker` | `crates/rag-core/src/traits.rs` | ✅ |
| `ParagraphChunker`: overlap-aware paragraph splitting with configurable `max_chars` and `overlap_chars` | `crates/rag-core/src/chunker.rs` | ✅ |
| `MockEmbedder`: deterministic hash-based vectors for tests; wrapping multiply fix | `crates/rag-core/src/embedder.rs` | ✅ |
| `MemoryVectorStore`: thread-safe in-memory cosine similarity with `SearchFilter` support | `crates/rag-core/src/store.rs` | ✅ |
| `Indexer`: chunk → embed → store orchestration with per-document delete | `crates/rag-core/src/indexer.rs` | ✅ |
| `StandardRetriever`: embed query → overfetch → permission filter → citation builder | `crates/rag-core/src/retriever.rs` | ✅ |
| `rag-extension-protocol`: `rag.extension.v1` envelopes, capability descriptors, heartbeat, typed `LoadDocument` and `EmbedTexts` payloads | `crates/rag-extension-protocol/src/lib.rs` | ✅ |
| Stubs with implementation notes: `rag-store-pgvector`, `rag-connectors`, `rag-zenoh`, `rag-mcp`, `rag-server` | `crates/*/src/lib.rs` | ✅ |
| 3 integration tests: index-and-search, delete-document, citation builder | `crates/rag-core/tests/integration.rs` | ✅ |
| `.cargo/config.toml` Windows SDK path fix; `.gitattributes` cross-platform LF normalisation | root | ✅ |
| `LICENSE` (Apache-2.0), `NOTICE` (full third-party attribution), `README.md`, `ROADMAP.md`, `docs/understanding-rag.md` | root | ✅ |

Acceptance criteria:

- A Rust program can index in-memory text through `Indexer` and retrieve citation-ready `SearchResult` records through `StandardRetriever` with no external services.
- `cargo test -p rag-core` passes on Linux, macOS, and Windows.
- `cargo check --workspace` passes on all three platforms without a VS Developer Prompt.
- Parity check: compare the core trait and pipeline shape with Haystack, LlamaIndex, and LangChain; document any missing core abstractions as deferred or out of scope.

---

## Milestone 2 — pgvector Store ✅ (v0.2.0)

**Goal:** Indexed content survives process restarts. Replace `MemoryVectorStore` with a durable PostgreSQL/pgvector backend.

| Task | Notes | Status |
|------|-------|--------|
| Add `sqlx` (postgres + runtime-tokio-rustls) and `pgvector` crate | `crates/rag-store-pgvector/Cargo.toml` | ✅ |
| Create migration: `rag_meta` + `rag_chunks` base schema; embedding column added dynamically at connect time | `crates/rag-store-pgvector/migrations/0001_initial.sql` | ✅ |
| Implement `VectorStore` using `<=>` cosine-distance operator and text-literal vector binding (`$1::vector(N)`) | `crates/rag-store-pgvector/src/lib.rs` | ✅ |
| Add HNSW index (`vector_cosine_ops`) created by `connect()` after the dimension is known | `crates/rag-store-pgvector/src/lib.rs` | ✅ |
| Implement `delete_by_document` and `delete_by_source` | `crates/rag-store-pgvector/src/lib.rs` | ✅ |
| Expose `PgVectorStore::connect(url: &str, dimension: usize)` async constructor with dimension-mismatch guard | `crates/rag-store-pgvector/src/lib.rs` | ✅ |
| Add inspection helpers: `chunk_counts_by_source()`, `last_indexed_at(source_id)` | `crates/rag-store-pgvector/src/lib.rs` | ✅ |
| Add Docker Compose file with `pgvector/pgvector:pg16` | `docker-compose.yml` | ✅ |
| 7 integration tests (skipped when `TEST_DATABASE_URL` is unset) | `crates/rag-store-pgvector/tests/integration.rs` | ✅ |
| `rag-mcp` config module: `StoreBackend`, `EmbedderProvider`, `RagConfig::validate()` | `crates/rag-mcp/src/config.rs` | ✅ |

Acceptance criteria:

- `PgVectorStore` satisfies the `VectorStore` trait and passes the same integration tests as `MemoryVectorStore`.
- Chunks written in one process are retrieved correctly after restart with no data loss.
- `delete_by_document` and `delete_by_source` leave no orphan rows in any table.
- A local dev environment requires only `docker compose up` before `cargo test`.
- Parity check: compare `VectorStore` behavior with Haystack, LlamaIndex, LangChain, and txtai document/vector store patterns, especially metadata filters, deletes, and score semantics.

---

## Milestone 3 — SharePoint Connector + Document Parsing ✅ (v0.3.0)

**Goal:** A SharePoint document library can be discovered, extracted, and indexed end-to-end, with incremental sync driven by change tokens.

| Task | Notes | Status |
|------|-------|--------|
| Add `sharepoint_rest_api` as a path dependency | `crates/rag-connectors/Cargo.toml` | ✅ |
| Implement `list_documents` via `get_folder_files_recursive` + extension/size filters | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Implement `load_document` via `get_file_content` (base64 decode) + content-type routing | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Implement `changes_since` via `get_list_changes` with change token persistence | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Route `.txt` and `.md` directly; route `.pdf`, `.docx`, `.xlsx`, `.pptx` to extension workers | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Attach file metadata to `DocumentMetadata` (modified time, content-type, size) | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Generate stable `DocumentId` from `sha256(site_url + "::" + server_relative_url)` | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Add `max_file_bytes` guard in connector config; skip and log oversized files | `crates/rag-connectors/src/sharepoint.rs` | ✅ |
| Add `FilesystemConnector` for local paths — useful for tests and self-hosted document stores | `crates/rag-connectors/src/filesystem.rs` | ✅ |
| Integration tests against a mock SharePoint REST server using `wiremock` | `crates/rag-connectors/tests/sharepoint_mock.rs` | ✅ |

Notes:
- Permission hints (`sp_get_user_effective_permissions`) deferred to Phase 5 (MCP layer) where the caller context is available per-request. The `DocumentMetadata.permissions` field and `PermissionFilter` trait are already in place.

Acceptance criteria:

- A configured `SharePointConnector` can list, load, and index all files in a SharePoint library in a single `index_all()` call.
- Running `changes_since(token)` a second time returns only changed files, not the full library.
- Search results include `source_url` pointing back to the SharePoint file and `citation.label` with the document title.
- Files over `max_file_bytes` are skipped with a `WARN` log entry, not a panic.
- 5 wiremock integration tests pass without a live SharePoint instance.
- Parity check: compare connector and document parsing behavior with RAGFlow, LlamaIndex, and Dify; record gaps in metadata fidelity, layout handling, and chunk/citation quality.

---

## Milestone 4 — Zenoh Extension Bus ✅ (v0.4.0)

**Goal:** Out-of-process workers in any language can serve `load_document`, `embed_texts`, and `rerank` requests from the Rust runtime over Zenoh pub/sub.

| Task | Notes | Status |
|------|-------|--------|
| Verify Zenoh license (EPL-2.0 OR Apache-2.0); use Apache-2.0 | `crates/rag-zenoh/Cargo.toml` | ✅ |
| Add `zenoh = "1"` dependency | `crates/rag-zenoh/Cargo.toml` | ✅ |
| Implement `ExtensionRegistry`: subscribe to announce/heartbeat wildcards; watchdog evicts stale workers | `crates/rag-zenoh/src/registry.rs` | ✅ |
| Implement heartbeat watchdog: evict workers that miss N consecutive beats | `crates/rag-zenoh/src/registry.rs` | ✅ |
| Implement request/reply for `load_document`, `embed_texts`, `rerank` via `ZenohCaller` | `crates/rag-zenoh/src/call.rs` | ✅ |
| Implement indexing lifecycle event publishing (6 events: index_started/progress/document_indexed/failed, sync_started/completed) | `crates/rag-zenoh/src/events.rs` | ✅ |
| Add mTLS support for Zenoh transport via `TlsConfig` in `ZenohConfig` | `crates/rag-zenoh/src/config.rs` | ✅ |
| Implement `ZenohEmbedder` (Embedder trait), `ZenohReranker` (Reranker trait), `ZenohDocumentLoader` helper | `crates/rag-zenoh/src/lib.rs` | ✅ |
| Add `listen_endpoints` + `multicast_scouting` to `ZenohConfig` for explicit TCP peer pairing | `crates/rag-zenoh/src/config.rs` | ✅ |
| Write `rag_worker_sdk` Python package: `DocumentLoaderWorker`, `EmbedderWorker`, `RerankerWorker` base classes | `python/rag_worker_sdk/` | ✅ |
| Write example Python PDF loader using `pdfplumber` (MIT) — pymupdf is AGPL-excluded | `python/examples/pdf_loader.py` | ✅ |
| Write example Python DOCX loader using `python-docx` | `python/examples/docx_loader.py` | ✅ |
| 7 in-process Zenoh integration tests (explicit TCP loopback, no router required) | `crates/rag-zenoh/tests/integration.rs` | ✅ |
| Add `RerankRequest/Response/Candidate/RankedChunk` to extension protocol | `crates/rag-extension-protocol/src/lib.rs` | ✅ |

Acceptance criteria:

- A standalone Python worker can register itself, serve `load_document` requests, and pass extracted text back to the Rust runtime with no Rust changes. ✅ (demonstrated by PDF/DOCX examples)
- A worker that stops sending heartbeats is evicted within 3× the heartbeat interval. ✅ (watchdog_evicts test)
- All Zenoh transport can be secured with mTLS via `TlsConfig` in `ZenohConfig` alone. ✅
- 7 integration tests pass without an external router, using in-process TCP loopback pairs. ✅
- Parity check: compare the Zenoh worker-bus and Python SDK ergonomics with Haystack components, LangChain loaders/tools, Dify external knowledge workflows, and Flowise pipeline nodes; document why this project keeps extension workers transport-neutral and out-of-process.

---

## Milestone 5 — MCP Layer (v0.5.0)

**Goal:** An MCP-compatible host (Claude Desktop, Cursor) can search a live corpus, trigger indexing, and receive citation-ready responses from all 7 tools.

| Task | Notes | Status |
|------|-------|--------|
| Verify `rmcp` license before adding as dependency — expected MIT (Apache-2.0 compatible); confirm at implementation time | `crates/rag-mcp/Cargo.toml` | [ ] |
| Add `rmcp` dependency | `crates/rag-mcp/Cargo.toml` | [ ] |
| Load RAG components from `rag.toml` config file at startup | `crates/rag-mcp/src/config.rs` | [ ] |
| Implement `rag_search(query, k, source_ids?, caller_context?)` → `SearchResult[]` | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_get_document(document_id)` → document metadata + chunk list | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_get_context(query, k, token_budget?)` → ranked passages string + citation list | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_index_source(source_id)` → job status | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_sync_source(source_id)` → incremental sync status | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_list_sources()` → source list with index status and chunk count | `crates/rag-mcp/src/tools.rs` | [ ] |
| Implement `rag_explain_match(query, chunk_id)` → score, similarity breakdown, metadata | `crates/rag-mcp/src/tools.rs` | [ ] |
| Guard `rag_index_source` and `rag_sync_source` with `RAG_READ_ONLY=true` check | `crates/rag-mcp/src/tools.rs` | [ ] |
| Add guided next-step hints to search tool responses | `crates/rag-mcp/src/tools.rs` | [ ] |
| Support stdio transport (Claude Desktop) | `crates/rag-mcp/src/main.rs` | [ ] |
| Support HTTP/SSE transport (Cursor, remote clients) | `crates/rag-mcp/src/main.rs` | [ ] |
| Integration tests: all 7 tools against in-memory store | `crates/rag-mcp/tests/` | [ ] |
| Add `prev_chunk_id` and `next_chunk_id` to `ChunkMetadata` for context-window expansion | `crates/rag-core/src/models.rs` | [ ] |
| Add `IndexJobStatus` enum and tracking to `Indexer` so `rag_index_source` can report progress | `crates/rag-core/src/indexer.rs` | [ ] |
| Add `SearchFilter.modified_after: Option<DateTime<Utc>>` for date-range filtering | `crates/rag-core/src/models.rs` | [ ] |
| Enrich `DocumentMetadata` with SharePoint list item fields (author, created\_at, version) via additional REST calls | `crates/rag-connectors/src/sharepoint.rs` | [ ] |
| Add `max_payload_bytes` guard to `ZenohCaller.load_document` before sending | `crates/rag-zenoh/src/call.rs` | [ ] |

Acceptance criteria:

- Claude Desktop can connect to `rag-mcp` via stdio, call `rag_search`, and receive citation-labelled results pointing to SharePoint source URLs.
- `rag_index_source` and `rag_sync_source` return a descriptive error when `RAG_READ_ONLY=true`.
- All 7 tools return `CallToolResult::success` on the happy path and `CallToolResult::error` on failures, never panicking.
- `rag_get_context` respects `token_budget` by truncating passages to fit.
- Parity check: compare tool/API behavior with Dify knowledge APIs, AnythingLLM workspace search, LlamaIndex query engines, and LangChain retriever tools; document any intentional MCP-specific differences.

---

## Milestone 6 — Real Embedding Providers (v0.6.0)

**Goal:** Replace `MockEmbedder` with production-quality embedding suitable for semantic search against real corpora.

| Task | Notes | Status |
|------|-------|--------|
| Implement `OpenAIEmbedder` supporting `text-embedding-3-small` and `text-embedding-3-large` | `crates/rag-core/src/embedder.rs` | [ ] |
| Batch requests up to 2048 texts per API call | `crates/rag-core/src/embedder.rs` | [ ] |
| Add token counting guard: skip chunks exceeding 8191 tokens with a `WARN` log | `crates/rag-core/src/embedder.rs` | [ ] |
| Add retry with exponential backoff on 429/5xx from OpenAI | `crates/rag-core/src/embedder.rs` | [ ] |
| Read API key from environment (`OPENAI_API_KEY`); never log the key | `crates/rag-core/src/embedder.rs` | [ ] |
| Implement `LocalOnnxEmbedder` using the `ort` crate (ONNX Runtime) | `crates/rag-core/src/embedder.rs` | [ ] |
| Support `all-MiniLM-L6-v2` and `bge-m3` model configurations out of the box | `crates/rag-core/src/embedder.rs` | [ ] |
| Detect and enable CUDA, CoreML, and DirectML acceleration when available | `crates/rag-core/src/embedder.rs` | [ ] |
| Support loading model files from a local path or by HuggingFace Hub ID | `crates/rag-core/src/embedder.rs` | [ ] |
| Add `[embedder]` section to `rag.toml` with `provider = "openai" \| "local-onnx" \| "mock"` | `crates/rag-mcp/src/config.rs` | [ ] |
| Validate that `PgVectorStore` dimension matches the selected embedder's dimension at startup | `crates/rag-mcp/src/config.rs` | [ ] |
| Add `embed_query_mode` to `Embedder` trait/config for asymmetric models (separate model for query vs document embedding) | `crates/rag-core/src/traits.rs` | [ ] |
| Fan out large `embed_texts` batches across multiple registered worker instances in `ZenohCaller` | `crates/rag-zenoh/src/call.rs` | [ ] |

Acceptance criteria:

- A corpus indexed with `OpenAIEmbedder` returns semantically relevant results for paraphrased queries that keyword search would miss.
- `LocalOnnxEmbedder` produces embeddings without a network call and runs on all three platforms.
- Switching embedding providers without re-indexing is blocked at startup with a clear error message explaining why dimensions must match.
- The OpenAI API key is absent from all log output at every log level.
- Parity check: compare embedding provider configuration, batching, retry, and local-model behavior with Haystack, LlamaIndex, LangChain, and txtai.

---

## Milestone 7 — Production Hardening + Observability (v0.7.0)

**Goal:** The system runs safely and unattended; operators can diagnose failures and measure performance from telemetry alone.

| Task | Notes | Status |
|------|-------|--------|
| OpenTelemetry tracing: instrument `Indexer`, `Retriever`, `SharePointConnector`, `PgVectorStore` | `crates/rag-core/src/indexer.rs`, `retriever.rs`, `crates/rag-connectors/src/sharepoint.rs` | [ ] |
| Export OTLP traces to a configurable collector endpoint | `crates/rag-mcp/src/main.rs` | [ ] |
| Add Prometheus-compatible metrics: indexing throughput (docs/min), search latency (P50/P95/P99), error rate, queue depth | `crates/rag-mcp/src/metrics.rs` | [ ] |
| Graceful shutdown: drain in-flight requests before exit on SIGTERM/SIGINT | `crates/rag-mcp/src/main.rs` | [ ] |
| Validate all `rag.toml` fields at startup; exit with a clear error on missing required fields | `crates/rag-mcp/src/config.rs` | [ ] |
| Rate limiting wrapper for embedding provider calls (token bucket; configurable RPS) | `crates/rag-core/src/embedder.rs` | [ ] |
| Circuit breaker for `SharePointConnector` and Zenoh worker calls | `crates/rag-connectors/src/sharepoint.rs`, `crates/rag-zenoh/src/call.rs` | [ ] |
| Retry with exponential backoff for `SharePointConnector` on 429/503 | `crates/rag-connectors/src/sharepoint.rs` | [ ] |
| Add `cargo audit` as required CI check; fix all advisory hits before merging | `.github/workflows/ci.yml` | [ ] |
| Add `cargo deny check licenses` as required CI check | `.github/workflows/ci.yml` | [ ] |
| Add `cargo bench` benchmarks: chunker throughput, cosine similarity, full retriever pipeline | `crates/rag-core/benches/` | [ ] |
| Add load test: 50,000 chunks in `PgVectorStore`, 100 concurrent search queries | `benches/load_test.rs` | [ ] |
| Migration 0002: add indexed `modified_at` and `content_type` columns to `rag_chunks` for efficient metadata filtering | `crates/rag-store-pgvector/migrations/` | [ ] |
| Add chunk quality filter to `Indexer`: discard chunks below a configurable minimum length or entropy threshold | `crates/rag-core/src/indexer.rs` | [ ] |
| Add `list_documents_paginated(cursor) → (Vec<DocumentRef>, Option<String>)` to `Connector` trait | `crates/rag-core/src/traits.rs` | [ ] |
| Add round-robin selection across multiple registered workers for the same content type in `ExtensionRegistry` | `crates/rag-zenoh/src/registry.rs` | [ ] |

Acceptance criteria:

- Every request carries an OpenTelemetry trace ID from the MCP tool call through the store query.
- P99 search latency is below 200ms at a corpus of 50,000 chunks under 100 concurrent queries.
- Throttling and transient 5xx responses from SharePoint or the embedding provider do not break indexing workflows; they are retried and logged.
- `cargo audit` and `cargo deny` pass on every PR targeting `main`.
- Parity check: compare observability, retry, rate-limit, and operational behavior with Dify, RAGFlow, and Haystack; document production gaps before release.

---

## Milestone 8 — Python Extension SDK (v0.8.0)

**Goal:** Python developers can write custom document loaders, embedders, and rerankers without touching Rust or understanding Zenoh internals.

| Task | Notes | Status |
|------|-------|--------|
| Finalise `rag_worker_sdk` Python package with `DocumentLoaderWorker`, `EmbedderWorker`, `RerankerWorker` base classes | `python/rag_worker_sdk/` | [ ] |
| Publish `rag_worker_sdk` to PyPI (or internal registry) | `python/` | [ ] |
| Write and test a production-quality PDF loader using `pymupdf` | `python/examples/pdf_loader.py` | [ ] |
| Write and test a production-quality DOCX loader using `python-docx` | `python/examples/docx_loader.py` | [ ] |
| Write and test an XLSX loader using `openpyxl` with sheet-aware chunking | `python/examples/xlsx_loader.py` | [ ] |
| Write extension author guide covering worker lifecycle, error handling, and deployment | `docs/extension-authors.md` | [ ] |
| Decide whether Python search consumers need PyO3 bindings or Zenoh client SDK only | `docs/extension-authors.md` | [ ] |
| If PyO3: expose coarse-grained `RagClient` API with `maturin`; publish to PyPI | `crates/rag-py/` | [ ] |
| CI: run Python loader integration tests in the same pipeline as Rust tests | `.github/workflows/ci.yml` | [ ] |
| Add async Python SDK: `asyncio`-native base classes for all three worker types | `python/rag_worker_sdk/async_worker.py` | [ ] |

Acceptance criteria:

- A Python developer can write and register a custom document loader in under 50 lines using `rag_worker_sdk`.
- The PDF, DOCX, and XLSX loaders correctly extract text from real-world files and pass it to the Rust indexing pipeline.
- Extension workers can fail and restart without affecting the running RAG server.
- Parity check: compare Python extension author experience with Haystack custom components, LangChain loaders/tools, and LlamaIndex readers; document setup friction and missing helper APIs.

---

## Milestone 9 — Security Audit + Release Gates (v1.0.0)

**Goal:** Lock the public API at `1.0.0`, pass a security audit, validate the system under load, and publish release artifacts for all supported platforms.

| Task | Notes | Status |
|------|-------|--------|
| `cargo audit` clean with zero high/critical advisories | CI | [ ] |
| `cargo deny check licenses` clean with explicit allow-list | CI | [ ] |
| Fuzz `rag-extension-protocol` deserialization with `cargo fuzz` | `fuzz/` | [ ] |
| Review and lock the `rag-core` public API surface; no breaking changes without a major version bump | `crates/rag-core/src/lib.rs` | [ ] |
| Full rustdoc on all public APIs in `rag-core` and `rag-extension-protocol` | all crates | [ ] |
| `cargo clippy -- -D warnings` clean on stable Rust | CI | [ ] |
| `cargo fmt --check` enforced in CI | CI | [ ] |
| 80% line coverage on `rag-core` and `rag-store-pgvector` | CI | [ ] |
| Deployment guide: Docker, systemd, bare-metal, environment variables reference | `docs/deployment.md` | [ ] |
| SharePoint connector setup guide: Azure AD app registration, auth modes, library config | `docs/sharepoint-setup.md` | [ ] |
| `CHANGELOG.md` maintained from v0.1.0 onward | `CHANGELOG.md` | [ ] |
| Semver stability guarantee documented; versioning policy in `README.md` | `README.md` | [ ] |
| Binary releases for Linux x86\_64, Linux aarch64, macOS arm64, macOS x86\_64, Windows x86\_64 | `.github/workflows/release.yml` | [ ] |
| Docker image published to container registry on merge to `main` | `.github/workflows/release.yml` | [ ] |
| `cargo about generate` third-party license report included in release artifacts | `.github/workflows/release.yml` | [ ] |
| Load test passed: P99 search < 200ms at 50,000 chunks under sustained load | `benches/load_test.rs` | [ ] |

Acceptance criteria:

- No high or critical CVEs in the dependency tree at release time.
- `rag-core 1.0.0` semver is stable: any breaking change requires `2.0.0`.
- A developer who has never used the project can go from zero to a running RAG server in under 30 minutes using the deployment guide.
- Release binaries and Docker image are built from the same CI pipeline and tagged with the same version.
- Parity check: run a final v1.0 comparison against Dify, AnythingLLM, RAGFlow, Haystack, and LlamaIndex for deployment docs, API stability, document ingestion expectations, and retrieval/citation behavior.

---

## Milestone 10 — HTTP API (v1.1.0)

**Goal:** Provide an optional HTTP interface matching the MCP tool surface, enabling non-MCP clients and migration from existing REST-based RAG services.

| Task | Notes | Status |
|------|-------|--------|
| Add `axum` and `tower-http` to `rag-server` | `crates/rag-server/Cargo.toml` | [ ] |
| Implement REST endpoints mirroring all 7 MCP tools | `crates/rag-server/src/routes.rs` | [ ] |
| Generate OpenAPI spec via `utoipa` or `aide` | `crates/rag-server/src/openapi.rs` | [ ] |
| Add API key authentication middleware | `crates/rag-server/src/auth.rs` | [ ] |
| Add `RAG_HTTP_READ_ONLY` guard for mutation endpoints | `crates/rag-server/src/routes.rs` | [ ] |
| Write migration guide: endpoint mapping from existing Python FastAPI RAG service | `docs/migration.md` | [ ] |
| Integration tests for all HTTP routes | `crates/rag-server/tests/` | [ ] |

Acceptance criteria:

- An HTTP client that previously called a Python FastAPI RAG service can switch to `rag-server` with changes only to base URL and auth header.
- The OpenAPI spec is accurate and can be imported into API clients without modification.
- Parity check: compare REST/API shape with Dify external knowledge APIs, AnythingLLM API patterns, and LlamaIndex service integrations; document migration gaps and compatibility decisions.

---

## Milestone 11 — Complete Document Loader Library (v1.2.0)

**Goal:** Every common enterprise file format has a production-quality extension worker. Each format goes through a documented research spike before implementation to select the best language and library — quality of extraction takes priority over language preference, and **Apache-2.0 license compatibility is a hard requirement for all distributed code**.

> **Priority note:** This milestone can be brought forward ahead of M10 (HTTP API). The library is not practically useful for real document corpora until most of these loaders exist.

### License constraints

Every library used in a loader that is committed to this repository must be Apache-2.0 compatible. The following candidates from the initial design have **incompatible licenses and must not be used**:

| Library | License | Problem | Apache-2.0 compatible alternative |
|---------|---------|---------|-----------------------------------|
| `pymupdf` / PyMuPDF | AGPL-3.0 OR Artifex Commercial | AGPL is copyleft; commercial license required for proprietary distribution | `pdfplumber` (MIT) — adopted in M4 `pdf_loader.py`; uses `pdfminer.six` (MIT) internally. `pypdf` (BSD-3) is a separate compatible alternative evaluated in the PDF advanced spike but not adopted. |
| `html2text` | GPL-3.0 | Copyleft | `markdownify` (MIT) or plain `beautifulsoup4` text extraction |
| `extract-msg` | GPL-3.0 | Copyleft | `olefile` (BSD-2-Clause) with custom MSG stream parsing — lower-level, more work |
| `ebooklib` | AGPL-3.0 | Copyleft | Custom Rust EPUB reader via `zip` (MIT) + `quick-xml` (MIT) + `scraper` (MIT/Apache-2.0); EPUB is a ZIP of HTML files |
| `epub` crate (Rust) | GPL-3.0 | Copyleft | Same custom Rust approach as above |

**LibreOffice** (for `.doc`/`.ppt` legacy conversion) is LGPL-2.1 / Mozilla Public License. Using it as a system subprocess that users install separately is acceptable; **do not link to or bundle LibreOffice**. Document this deployment dependency clearly.

**`odfpy`** is triple-licensed (Apache-2.0 / GPL / LGPL). Pin to the Apache-2.0 variant explicitly in `pyproject.toml`; the spike document must confirm this is verifiable at install time.

**pymupdf note for operators:** If an operator holds an Artifex commercial license, pymupdf provides significantly better PDF quality than pdfplumber on complex layouts. The extension worker architecture supports this cleanly — an operator can run their own `pymupdf`-based worker without it being part of this repository. This should be documented in `docs/loader-registry.md`.

### Formats covered

| Format group | File types | Approach |
|---|---|---|
| Text/Markup | `.html`, `.htm`, `.xml`, `.csv`, `.json` | Spike — Rust candidates exist |
| PDF (text + layout) | `.pdf` | Python: `pdfplumber` (MIT) — done in M4 |
| PDF (advanced) | Tables, forms, scanned pages / OCR | Python: `pdfplumber` (MIT) + `pytesseract` (Apache-2.0) or `easyocr` (Apache-2.0) |
| Modern Office | `.docx`, `.xlsx`, `.pptx` | Python: `python-docx` (MIT), `openpyxl` (MIT), `python-pptx` (MIT) — partially done in M4/M8 |
| Legacy Office | `.doc`, `.xls`, `.ppt` | Spike — LibreOffice headless (LGPL, system dep) + `xlrd` (BSD); see license note above |
| OpenDocument | `.odt`, `.ods`, `.odp` | Python: `odfpy` (Apache-2.0 variant) — confirm license at install time |
| Email | `.eml`, `.msg` | EML: Python `mail-parser` (Apache-2.0); MSG: `olefile` (BSD-2) + custom parsing |
| Code files | `.py`, `.js`, `.ts`, `.rs`, `.java`, `.go`, `.cpp`, `.cs`, `.rb`, `.php`, … | Rust: `tree-sitter` (MIT) |
| Specialised | `.epub`, `.rtf`, `.mhtml`, scanned `.tiff` | EPUB: custom Rust (`zip`+`quick-xml`+`scraper`); RTF: `striprtf` (BSD-3); MHTML: Python stdlib; TIFF: `easyocr` (Apache-2.0) |

### Research spike tasks

Each spike produces a decision document in `docs/loader-research/{group}.md`.  
**Required sections in every spike document:** candidate libraries, SPDX license, Apache-2.0 compatibility verdict, text-extraction quality, table handling, metadata fidelity, error resilience, performance, maintenance status, and final library recommendation. No implementation may begin without a completed spike document.

| Task | Output | Status |
|------|--------|--------|
| Spike: HTML / XML loaders — compare Rust (`scraper` MIT/Apache-2.0, `quick-xml` MIT) vs Python (`beautifulsoup4` MIT, `lxml` BSD-3); verify `html2text` GPL-3 is excluded; confirm `markdownify` MIT as fallback for Markdown output | `docs/loader-research/html-xml.md` | [ ] |
| Spike: Spreadsheet loaders — compare Rust `calamine` (MIT) vs Python `openpyxl` (MIT) / `xlrd` (BSD) on cell text, merged cells, multi-sheet indexing, and legacy `.xls` coverage; all candidates are Apache-2.0 compatible | `docs/loader-research/spreadsheet.md` | [ ] |
| Spike: PDF advanced — compare `pdfplumber` (MIT) table API vs `pypdf` (BSD-3) for complex layouts; evaluate scanned-page detection and OCR library choice between `pytesseract` (Apache-2.0) and `easyocr` (Apache-2.0); **pymupdf is excluded (AGPL)** but document the quality gap and the commercial-license operator path | `docs/loader-research/pdf-advanced.md` | [ ] |
| Spike: Legacy Office formats (`.doc`, `.ppt`) — evaluate LibreOffice headless subprocess (LGPL, system dep — acceptable) vs pure-Python tools (`python-docx`/`python-pptx` cannot read binary formats); document LibreOffice installation requirement; evaluate `xlrd` (BSD) for `.xls` | `docs/loader-research/legacy-office.md` | [ ] |
| Spike: Email formats — `mail-parser` (Apache-2.0) for EML; for MSG, compare `olefile` (BSD-2) low-level approach vs documenting `extract-msg` as **GPL-excluded** and describing a custom `olefile`-based parser; evaluate quality tradeoff | `docs/loader-research/email.md` | [ ] |
| Spike: Code files — evaluate `tree-sitter` (MIT) Rust bindings for AST-aware chunking; confirm language grammar licenses (Python/JS/TS/Rust/Java/Go/C++/C#/Ruby/PHP grammars are MIT); compare against plain-text splitting for retrieval quality | `docs/loader-research/code.md` | [ ] |
| Spike: Specialised formats — EPUB: compare custom Rust (`zip` MIT + `quick-xml` MIT + `scraper` MIT) vs documenting `ebooklib` as **AGPL-excluded**; RTF: `striprtf` (BSD-3); MHTML: Python `email` stdlib; scanned TIFF: `easyocr` (Apache-2.0) | `docs/loader-research/specialised.md` | [ ] |

### Implementation tasks

Tasks are sequenced: spike approved → implementation → test with real-world files → register in extension registry.

| Task | Notes | Status |
|------|-------|--------|
| License audit: run `cargo deny check licenses` and `pip-licenses` against all loader dependencies before any implementation merges | CI gate — fails on non-compatible SPDX identifiers | [ ] |
| HTML loader | Rust crate `crates/rag-loaders/` with `html` feature; `scraper` (MIT/Apache-2.0); strip tags, detect `<h1>`–`<h6>` for `DocumentSection.title`, preserve `<table>` as pipe-formatted text | [ ] |
| XML loader | Rust (`quick-xml` MIT); extract text nodes, use element names as section titles; configurable path-to-section mapping | [ ] |
| CSV loader | Rust (`csv` MIT/Unlicense); header row → section title per column group; configurable row-range chunking | [ ] |
| JSON loader | Rust (`serde_json` MIT/Apache-2.0, already a dep); flatten nested objects to readable text; configurable depth limit | [ ] |
| PPTX loader | Python: `python-pptx` (MIT); one `DocumentSection` per slide with slide title and speaker notes | [ ] |
| PDF table extraction | Extend existing `pdf_loader.py` (`pdfplumber` MIT); already partially implemented in M4; add configurable table extraction mode | [ ] |
| Scanned PDF / OCR | Python: detect scanned pages by extractable-text coverage; route to `easyocr` (Apache-2.0) or `pytesseract` (Apache-2.0); return per-page sections | [ ] |
| XLSX loader (production) | Promote M8 example to production; use `openpyxl` (MIT); sheet-level sections, merged cell flattening, named range extraction | [ ] |
| DOC / PPT legacy loader | Python: LibreOffice headless subprocess (`soffice --headless --convert-to docx`); convert to modern format, delegate to DOCX/PPTX loader; document LibreOffice as a required system dependency | [ ] |
| XLS legacy loader | Rust `calamine` (MIT) if spike confirms adequate coverage; Python `xlrd` (BSD) as fallback | [ ] |
| ODF loader (.odt, .ods, .odp) | Python: `odfpy` (Apache-2.0 variant — must be pinned explicitly); paragraph-style → section title for text; sheet/page aware for Calc and Impress | [ ] |
| EML loader | Python: `mail-parser` (Apache-2.0) + stdlib `email`; extract plain-text and HTML body, thread subject as title, strip quoted replies | [ ] |
| MSG loader (Outlook) | Python: `olefile` (BSD-2-Clause) + custom MSG property-stream parser; extract body, subject, sender, date; flag attachments as child documents; document quality limitations vs GPL `extract-msg` | [ ] |
| RTF loader | Python: `striprtf` (BSD-3-Clause); extract plain text; detect RTF section markers where present | [ ] |
| EPUB loader | Rust: custom reader using `zip` (MIT) + `quick-xml` (MIT) + `scraper` (MIT/Apache-2.0); one `DocumentSection` per chapter from OPF spine; `ebooklib` (AGPL) is excluded | [ ] |
| MHTML / MHT loader | Python: stdlib `email` (unpacks MIME archive); extract primary HTML part, delegate to HTML loader | [ ] |
| Code loader (Rust native) | Rust crate `crates/rag-loader-code/` using `tree-sitter` (MIT) with grammars for top 10 languages (all MIT); chunk on function/class/top-level boundaries; include signature + docstring in `DocumentSection.title` | [ ] |
| Scanned TIFF / image OCR | Python: `easyocr` (Apache-2.0) preferred; `pytesseract` (Apache-2.0) as fallback; one section per text block; page metadata from TIFF IFD tags | [ ] |
| Register all loaders in extension registry docs | Document `content_type`, `extension_id`, required system deps, and known limitations per loader | `docs/loader-registry.md` | [ ] |
| Test suite: real-world sample files | One test file per format in `tests/fixtures/`; integration test verifying non-empty `text`, correct `page_count`, and non-empty `sections` | `tests/loader_integration/` | [ ] |
| Loader comparison matrix | Document extraction quality, table fidelity, metadata completeness, known limitations, and the quality gap where commercial-only libraries would improve output | `docs/loader-research/comparison-matrix.md` | [ ] |

### Acceptance criteria

- Every format in the table above has a working extension worker returning non-empty `LoadDocumentResponse.text`.
- All loaders preserve section structure: `DocumentSection.title` is populated where the source format has headings, slides, sheets, or chapters.
- All loaders populate `DocumentSection.page` where the source format has discrete pages.
- PDF and Office loaders populate `DocumentMetadata.author` and `DocumentMetadata.page_count` where the source format stores these.
- The code loader chunks on AST boundaries (function/class/top-level block), not arbitrary character counts.
- The scanned PDF and TIFF loaders route pages to OCR only when they contain no selectable text.
- **All loader dependencies are Apache-2.0 compatible**; `cargo deny check licenses` and `pip-licenses` pass with no unlisted identifiers in CI.
- The MSG loader documents the quality gap vs `extract-msg` (GPL-excluded) so operators understand the tradeoff and can supply their own compliant worker.
- `docs/loader-registry.md` documents any loader that requires a non-default system dependency (LibreOffice, Tesseract).
- Each loader has a completed spike document justifying the library choice and confirming the license verdict.
- All loaders are tested against real-world (not synthetic) sample files.
- Parity check: compare loader quality and breadth with RAGFlow's parser suite, LlamaIndex `SimpleDirectoryReader`/`LlamaParse`, and Unstructured; document any formats covered by those systems that remain missing or lower quality here, and note where commercial-license libraries would close the quality gap.

---

## Deferred or Explicitly Out of Scope

- **Microsoft Graph API parity** — Graph endpoints overlap with SharePoint REST for document access but serve a broader Microsoft 365 surface (Teams, Outlook, OneDrive personal). Not in scope unless a specific SharePoint-adjacent Graph convenience workflow justifies it.
- **SaaS or hosted service components** — Multi-tenant hosting, billing, usage metering, and control planes are outside the scope of this library.
- **UI or dashboard** — No web UI for managing sources, viewing index status, or browsing search results. Operators use `rag_list_sources` and observability tooling.
- **Training or fine-tuning embedding models** — This library uses pre-trained embedding models. Training infrastructure is a separate concern.
- **Browser-side JavaScript / WASM embedding** — The Rust core targets server-side deployment. A browser JS SDK is not planned.
- **Synchronous (non-async) API surface** — All `rag-core` traits are async. A sync wrapper is not planned; callers that need sync can use `tokio::runtime::Runtime::block_on`.
- **SharePoint Add-in packaging or browser cross-domain libraries** — The connector targets `/_api` protocol-level REST, not SharePoint Add-in or CSOM abstractions.
- **DAG pipeline composition** — The indexing pipeline is intentionally linear (Connector → Chunker → Embedder → VectorStore). Complex branching, conditional routing, and multi-source fan-out belong in the orchestrator layer (e.g., the MCP tool or a workflow engine), not in rag-core. *(Confirmed out of scope by M1 parity vs Haystack pipeline graphs.)*
- **Flowise / visual pipeline builders** — Flowise nodes are in-process, JavaScript/TypeScript, and UI-configured. The architecture is fundamentally different from this project's out-of-process, transport-neutral extension workers. No compatibility layer is planned. *(Confirmed out of scope by M4 parity.)*

---

## Parking Lot

Genuine improvements that should not displace the milestones above. Revisit after v1.0.0.

- **Ephemeral chat RAG mode** — Run a short-lived MCP server per chat/session for uploaded documents that are too large for the model context. Index into memory or temporary storage, expose search/context tools during the session, and delete all chunks and extracted text on shutdown, explicit clear, or TTL expiry. This runs alongside the persistent SharePoint-backed RAG instance rather than replacing it.
- **Hybrid search (BM25 + vector)** — Combine keyword scoring with vector similarity for better results on exact-match queries. pgvector supports this via `ts_rank` + `<->` combined scoring. *(Confirmed gap by M1 and M2 parity reviews.)*
- **Cross-encoder reranking** — A cross-encoder model reading (query, passage) pairs together produces better relevance scores than bi-encoder cosine similarity alone. Could run as a Zenoh extension worker.
- **Multi-modal indexing** — Extract and index diagrams, charts, and images from PDFs alongside the text. Requires a vision model extension worker.
- **Query rewriting and expansion** — Automatically expand a short user query into multiple semantic variants before retrieval. Improves recall on ambiguous or terse questions.
- **Conversation-aware retrieval** — Use recent conversation history as additional retrieval context so follow-up questions ("what about the Q4 version?") resolve correctly.
- **Microsoft Graph connector** — Index SharePoint Online content via Graph `/drives` and `/sites` endpoints for organisations where Graph is preferred over the `/_api` surface.
- **Alternative VectorStore adapters** — Elasticsearch/OpenSearch dense vector, Qdrant, Weaviate, Pinecone. The `VectorStore` trait is the extension point; each would be a new crate.
- **Real-time indexing via SharePoint webhooks** — Subscribe to SharePoint list webhooks to trigger incremental sync immediately on document change rather than on a polling schedule.
- **Streaming search responses** — Return search results incrementally via SSE as chunks are scored, rather than buffering the full result set. Reduces time-to-first-result. *(Supported by `CapabilityDescriptor.supports_streaming`; Zenoh transport deferred.)*
- **Automated chunk quality evaluation (advanced)** — Post-M7 enhancements: LLM-based informativeness scoring, semantic deduplication across chunks, layout-aware boilerplate detection using `LayoutHints`. The basic length/entropy threshold ships in M7; this covers anything beyond that.
- **Federated search** — Fan a single query out to multiple independent RAG instances (different tenants, different source types) and merge ranked results.
- **Git repository connector** — Index Markdown documentation, READMEs, and code comments from Git repositories. Useful for internal developer knowledge bases.
- **Web crawler connector** — Index public-facing documentation sites. Lower priority than SharePoint for the primary use case.
- **S3 connector** — Index documents stored in AWS S3 or S3-compatible object storage (MinIO, Cloudflare R2).
- **`RecursiveChunker`** — Splits on paragraph → sentence → word → character boundaries in sequence; better than `\n\n`-only splitting for varied document formats. *(From M1 parity vs LangChain `RecursiveCharacterTextSplitter`.)*
- **`MarkdownChunker`** — Splits on heading levels and preserves the section hierarchy in `ChunkMetadata.section`. *(From M1 parity vs Haystack `MarkdownDocumentSplitter`.)*
- **MMR diversification** — Maximal Marginal Relevance penalises near-duplicate results; useful when a corpus has many similar documents. *(From M2 parity vs LangChain.)*
- **Filtered `count_chunks(filter)`** — Per-source chunk counts needed for operational monitoring without a full table scan. *(From M2 parity.)*
- **Advanced worker routing** — Post-M7 enhancements beyond basic round-robin: weighted routing, health-score-based selection, sticky routing for stateful workers. Basic round-robin ships in M7. *(From M4 parity.)*
- **Async Python SDK** — Moved to M8 task list.
- **Per-chunk `active` flag** — Allow operators to disable specific chunks without re-indexing the full document. Dify supports per-segment enable/disable. *(From M3 parity.)*

---

## Summary Timeline

| Milestone | Version | Status |
|-----------|---------|--------|
| 1 — Core Scaffold | v0.1.0 | ✅ |
| 2 — pgvector Store | v0.2.0 | ✅ |
| 3 — SharePoint Connector + Document Parsing | v0.3.0 | ✅ |
| 4 — Zenoh Extension Bus | v0.4.0 | ✅ |
| 5 — MCP Layer | v0.5.0 | [ ] |
| 6 — Real Embedding Providers | v0.6.0 | [ ] |
| 7 — Production Hardening + Observability | v0.7.0 | [ ] |
| 8 — Python Extension SDK | v0.8.0 | [ ] |
| 9 — Security Audit + Release Gates | v1.0.0 | [ ] |
| 10 — HTTP API | v1.1.0 | [ ] |
| 11 — Complete Document Loader Library | v1.2.0 | [ ] |
