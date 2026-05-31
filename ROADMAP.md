# rust-rag-mcp — Roadmap to v1.0

This document covers the path from the current Phase 1 scaffold to a commercially viable, production-ready v1.0 release.

See [rust-rag-mcp-design-roadmap.md](rust-rag-mcp-design-roadmap.md) for the original architecture decisions and design rationale.

---

## What "Production-Ready v1.0" Means

A v1.0 release must satisfy all of the following:

**Functional completeness**
- [ ] At least one production-grade embedding provider (OpenAI API and a local ONNX fallback)
- [ ] Document parsing for PDF, DOCX, XLSX, PPTX, TXT, and Markdown
- [ ] SharePoint connector: full-sync and incremental sync via change tokens
- [ ] MCP server with all 7 tools working against a live corpus
- [ ] Permission-aware retrieval that respects SharePoint ACLs
- [ ] Zenoh extension bus: out-of-process Python workers for document loading and embedding

**Production hardening**
- [ ] Durable storage: PostgreSQL/pgvector with migrations and schema versioning
- [ ] Retry with exponential backoff for all external calls (embedding, SharePoint, Zenoh)
- [ ] Rate limiting on embedding provider calls
- [ ] Graceful shutdown with in-flight request draining
- [ ] Configuration via TOML file and environment variables (no hardcoded values)

**Observability**
- [ ] Structured logging via `tracing` with configurable levels
- [ ] OpenTelemetry traces exported via OTLP
- [ ] Prometheus-compatible metrics: index throughput, search latency (P50/P95/P99), queue depth

**Security**
- [ ] No secrets or tokens in log output
- [ ] mTLS support for Zenoh transport (Phase 4+)
- [ ] API key or token auth for the HTTP API (Phase 7)
- [ ] Zero known high/critical CVEs in the dependency tree (automated via `cargo audit` in CI)
- [ ] Input validation at all system boundaries (MCP tool parameters, HTTP endpoints)

**Quality**
- [ ] 80%+ line coverage on `rag-core` and `rag-store-pgvector`
- [ ] Integration tests against a real pgvector instance (Docker Compose in CI)
- [ ] Load test: 10,000 chunks in pgvector, P99 search latency < 200ms
- [ ] `cargo clippy -- -D warnings` clean on stable Rust
- [ ] `cargo audit` clean

**Documentation and licensing**
- [ ] Full rustdoc on all public APIs in `rag-core` and `rag-extension-protocol`
- [ ] Deployment guide (Docker, systemd, bare-metal)
- [ ] SharePoint connector setup guide
- [ ] Extension author guide (Python SDK for custom document loaders)
- [ ] `cargo about` third-party attribution file generated and included in releases
- [ ] `CHANGELOG.md` maintained from this commit forward

**Release mechanics**
- [ ] Semver-stable public API (`rag-core` ≥ 1.0.0, no breaking changes without major bump)
- [ ] Published to crates.io (or private registry)
- [ ] Docker image published to a container registry
- [ ] GitHub/CI release pipeline producing versioned binaries for Linux (x86\_64 + aarch64), macOS (arm64 + x86\_64), and Windows (x86\_64)

---

## Phases

### Phase 0 + 1 — Core Scaffold ✅ DONE

- Workspace with 7 crates
- Domain models, all core traits
- ParagraphChunker, MockEmbedder, MemoryVectorStore
- Indexer and StandardRetriever orchestration
- Extension protocol envelope types
- 3 integration tests passing
- Apache-2.0 license and attribution

**Exit criteria met:** A Rust program can index in-memory text and return citation-ready search results.

---

### Phase 2 — pgvector Store

**Goal:** Indexed content survives process restarts.

Tasks:
- Add `sqlx` (postgres + `runtime-tokio-rustls`) and the `pgvector` crate to `rag-store-pgvector`
- Create migration: `rag_sources`, `rag_documents`, `rag_chunks` (with `embedding vector(N)` column)
- Implement `VectorStore` using `<->` cosine-distance operator and an `ivfflat` or `hnsw` index
- Support `delete_by_document` and `delete_by_source` with proper cascades
- Expose index inspection: chunk count per source, source list, last-indexed timestamp
- Add Docker Compose file with `pgvector/pgvector:pg16` for CI and local dev
- Integration tests against a real Postgres instance

**Estimated effort:** 3–4 weeks  
**Exit criteria:** A local Postgres/pgvector instance persists and retrieves indexed content across restarts.

---

### Phase 3 — SharePoint Connector + Document Parsing

**Goal:** A SharePoint document library can be indexed end-to-end.

Tasks:
- Wire `SharePointConnector` in `rag-connectors` to the `sharepoint_rest_api-rs` client:
  - `list_documents` → `sp_get_folder_files_recursive`
  - `load_document` → `sp_get_file_content` (base64 decode) + text extraction routing
  - `changes_since` → `sp_get_list_changes` with change token persistence
- Attach metadata from `SpFile` / `SpListItem`: title, URL, author, modified time, version
- Forward `sp_get_user_effective_permissions` results as permission hints in `DocumentMetadata`
- Text extraction routing by content type:
  - `.txt`, `.md` → UTF-8 decode directly in Rust
  - `.pdf`, `.docx`, `.xlsx`, `.pptx` → route to extension worker (Phase 4 wires this; Phase 3 can use a local Python subprocess as a stopgap)
- Add `stable_document_id` helper that produces a deterministic ID from `(site_url, server_relative_url)` so re-indexing a file updates rather than duplicates chunks
- Include file size guard (`max_file_bytes` in connector config)

**Estimated effort:** 5–7 weeks (includes document parsing work)  
**Exit criteria:** A SharePoint library can be fully indexed and incrementally synced; search returns results with correct citations pointing back to SharePoint URLs.

---

### Phase 4 — Zenoh Extension Bus

**Goal:** Out-of-process workers (Rust or Python) can serve document loading and embedding requests.

Tasks:
- Add `zenoh` dependency to `rag-zenoh`
- Implement `ZenohExtensionRegistry`:
  - Subscribe to `rag/extensions/{id}/announce` → register capability descriptor
  - Subscribe to `rag/extensions/{id}/heartbeat` → evict workers that miss N beats
  - Route `load_document` requests to the worker whose `content_types` matches
- Implement request/reply for `load_document`, `embed_texts`, `rerank`
- Implement indexing lifecycle event publishing (`rag/events/**`)
- Write a minimal Python SDK (`rag_worker_sdk`) that hides Zenoh details:
  ```python
  from rag_worker_sdk import DocumentLoaderWorker

  class PdfLoader(DocumentLoaderWorker):
      content_types = ["application/pdf"]
      def load(self, data: bytes, metadata: dict) -> LoadResult: ...
  ```
- Support mTLS for Zenoh transport via config

**Estimated effort:** 4–5 weeks  
**Exit criteria:** A standalone Python PDF loader registers itself and serves document loading requests from the Rust runtime without any Rust changes.

---

### Phase 5 — MCP Layer

**Goal:** An MCP client can search indexed content and trigger indexing.

Tasks:
- Add `rmcp` dependency to `rag-mcp`; verify license before adding
- Implement all 7 tools using `rmcp` tool macros:
  - `rag_search(query, k, source_ids?, caller_context?) → SearchResult[]`
  - `rag_get_document(document_id) → Document + chunks`
  - `rag_get_context(query, k, token_budget?) → context string + citations`
  - `rag_index_source(source_id) → IndexJob status`
  - `rag_sync_source(source_id) → SyncJob status`
  - `rag_list_sources() → Source[]`
  - `rag_explain_match(query, chunk_id) → ExplainResult`
- Return model-ready result shapes with citation blocks
- Support `RAG_READ_ONLY=true` to block all mutation tools
- Stdio transport (Claude Desktop) and HTTP/SSE transport (Cursor, remote clients)
- Load RAG components from a `rag.toml` config file

**Estimated effort:** 4–5 weeks  
**Exit criteria:** An MCP client (Claude Desktop) can search a live SharePoint corpus and get citation-ready responses.

---

### Phase 6 — Real Embedding Providers

**Goal:** Replace MockEmbedder with production-quality embedding.

Tasks:
- Implement `OpenAIEmbedder` (text-embedding-3-small / text-embedding-3-large):
  - Batch requests (up to 2048 texts per call)
  - Retry with exponential backoff on 429/5xx
  - Token counting guard (skip chunks over 8191 tokens)
  - API key from environment / config; never logged
- Implement `LocalOnnxEmbedder` using `ort` (ONNX Runtime):
  - Support `all-MiniLM-L6-v2` and `bge-m3` out of the box
  - CUDA/CoreML/DirectML acceleration when available
  - Model files downloaded from HuggingFace Hub or specified by path
- Add `embedder` field to `rag.toml` config with provider selection

**Estimated effort:** 3–4 weeks  
**Exit criteria:** A production deployment can use OpenAI embeddings with automatic fallback to a local ONNX model.

---

### Phase 7 — Production Hardening + Observability

**Goal:** The system is safe to run continuously in a production environment.

Tasks:
- OpenTelemetry tracing: instrument Indexer, Retriever, Connector, VectorStore
- Prometheus metrics: indexing throughput (docs/min), search latency (P50/P95/P99), error rate
- Graceful shutdown: drain in-flight requests before exit on SIGTERM
- Configuration validation at startup (missing required fields → clear error, no panic)
- Secrets management: support reading from environment, `rag.toml`, or external vault
- `cargo audit` clean; add to CI as a required check
- Rate limiting wrapper for embedding provider calls (token bucket)
- Circuit breaker for SharePoint connector and Zenoh workers
- Add `cargo bench` benchmarks for chunker, cosine similarity, and retriever pipeline

**Estimated effort:** 4–6 weeks  
**Exit criteria:** System handles sustained load for 24+ hours without memory growth, error rate < 0.1% under normal conditions, all telemetry flows to a collector.

---

### Phase 8 — Python Consumer API + Extension SDK

**Goal:** Python developers can index, search, and write extension workers without touching Rust.

Tasks:
- Finalize `rag_worker_sdk` Python package (pure Python, uses Zenoh Python client)
- Decide PyO3 vs Zenoh-only for the consumer API (search, index from Python scripts)
- If PyO3: expose coarse-grained `RagClient` API with `maturin`; publish to PyPI
- Write extension author guide with examples for PDF, DOCX, and XLSX loaders
- CI: run Python extension worker integration tests

**Estimated effort:** 4–5 weeks  
**Exit criteria:** A Python developer can write a custom document loader and a custom reranker, register them with the Rust runtime, and see results in search output.

---

### Phase 9 — HTTP API + Migration Path

**Goal:** Provide an HTTP interface and a migration path from any existing Python RAG service.

Tasks:
- Add `axum` + `tower-http` to `rag-server`
- REST endpoints matching the MCP tool surface (JSON in/out)
- OpenAPI spec generated via `utoipa` or `aide`
- API key auth middleware
- Document migration from the existing Python FastAPI RAG service:
  - Endpoint mapping
  - Data format differences
  - Side-by-side deployment guide

**Estimated effort:** 4–5 weeks  
**Exit criteria:** Existing HTTP clients can switch to the Rust service without changing their application code.

---

### Pre-release — Security Audit + Load Testing

**Goal:** Validate that the system is safe and performant enough for production.

Tasks:
- Run `cargo audit` and `cargo deny` as required CI checks
- Third-party security review (automated or manual)
- Load test: 100,000 document corpus, 1,000 concurrent search queries, measure P99
- Fuzz `rag-extension-protocol` deserialization using `cargo fuzz`
- Penetration test MCP input surfaces (tool parameter injection, oversized payloads)
- Review and finalize public API surface; lock semver at 1.0.0

**Estimated effort:** 3–4 weeks  
**Exit criteria:** No high/critical CVEs, P99 search < 200ms at corpus size of 50,000 chunks, public API locked.

---

## Platform Support Matrix

| Platform | CI | Binary release | Notes |
|---|---|---|---|
| Linux x86_64 (Ubuntu 22.04) | Required | Yes | Primary server target |
| Linux aarch64 (Ubuntu 22.04) | Required | Yes | ARM cloud / Graviton |
| macOS arm64 (Apple Silicon) | Required | Yes | Developer workstation |
| macOS x86_64 (Intel) | Best-effort | Yes | Legacy Mac support |
| Windows x86_64 (Server 2022 / Win 11) | Required | Yes | MCP client host |

Cross-compilation from Linux to Windows/macOS using `cross` and GitHub Actions matrix.

---

## CI / CD Requirements for v1.0

```
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --check
cargo test --workspace
cargo audit
cargo deny check licenses
# on Linux with Postgres running:
TEST_DATABASE_URL=... cargo test --workspace --features integration
```

All checks must pass on Linux, macOS, and Windows before merge to `main`.

---

## License Compliance Checklist

Before adding any new dependency, verify:

- [ ] License is MIT, Apache-2.0, Zlib, Unlicense, ISC, or BSD-2/3-Clause (pre-approved)
- [ ] If Apache-2.0: check for NOTICE file upstream; include it in our NOTICE if present
- [ ] If MPL-2.0 or LGPL: get explicit approval; may require file-level isolation
- [ ] Never add a GPL-2.0-only dependency (incompatible with Apache-2.0)
- [ ] Document the license in NOTICE before merging

Planned future dependencies and their anticipated licenses:

| Crate | License | Compatible? |
|---|---|---|
| sqlx | MIT OR Apache-2.0 | Yes |
| zenoh | EPL-2.0 OR Apache-2.0 | Yes (choose Apache-2.0) |
| axum | MIT | Yes |
| rmcp | MIT (expected) | Verify before adding |
| ort (ONNX Runtime) | MIT | Yes |
| PyO3 | MIT OR Apache-2.0 | Yes |
| opentelemetry | Apache-2.0 | Yes |
| prometheus | Apache-2.0 | Yes |

---

## Open Questions (Carry-Over from Design Document)

1. Should `rag-mcp` live as a feature flag in one crate or stay as a separate crate?
2. Which reranker strategy is the right default — cross-encoder ONNX, or LLM-based?
3. Should the extension protocol move to MessagePack for performance, or stay JSON?
4. Is HTTP compatibility with the existing Python FastAPI RAG service a hard requirement, or a nice-to-have?
5. What is the minimum viable permission model for v1.0 — SharePoint ACLs only, or generic group-based?
6. How much agent orchestration (multi-step retrieval, query rewriting) belongs here vs. in the MCP client?
