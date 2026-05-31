# Rust RAG/MCP Design Decisions and Preliminary Roadmap

## Purpose

This document captures the proposed direction for a new general-purpose RAG/MCP system inspired by the current Python RAG API and the Rust SharePoint REST/MCP library.

The goal is not to rewrite the current Python service file-for-file. The goal is to define a Rust library-first RAG platform that can support MCP, HTTP, CLI, Rust connectors, and standalone Python extensions through a stable extension protocol.

## Core Direction

Build the new system as a Rust library first.

MCP should be a feature or crate built on top of the library, not the core architecture. The same core APIs should be usable from:

- Rust applications
- MCP tools
- optional HTTP services
- CLI tools
- Python clients or extensions
- future connector libraries

The core value should live in indexing, document modeling, chunking, metadata, retrieval, permissions, citations, and connector orchestration.

## High-Level Architecture

```text
rag-core
  Domain models, traits, indexing and retrieval orchestration.

rag-store-pgvector
  PostgreSQL/pgvector implementation.

rag-connectors
  Rust-native connectors such as SharePoint, filesystem, S3, Git, etc.

rag-extension-protocol
  Versioned schemas for extension registration and calls.

rag-zenoh
  Zenoh transport for external extension workers.

rag-mcp
  MCP server/tools built over rag-core.

rag-server
  Optional HTTP API built over rag-core.

rag-py
  Optional Python client/bindings for Python users.
```

## Design Decisions

### 1. Rust Library First

The Rust core should own the durable interfaces:

- `DocumentSource`
- `Connector`
- `DocumentLoader`
- `Chunker`
- `Embedder`
- `VectorStore`
- `Indexer`
- `Retriever`
- `Reranker`
- `PermissionFilter`
- `CitationBuilder`

This keeps MCP, HTTP, and Python integration as presentation or extension layers rather than forcing the domain model to follow one transport.

### 2. MCP as a Layer

MCP should expose high-level operations such as:

- `rag_search`
- `rag_get_document`
- `rag_get_context`
- `rag_index_source`
- `rag_sync_source`
- `rag_list_sources`
- `rag_explain_match`

The MCP layer should call the same Rust APIs used by HTTP or CLI frontends.

### 3. SharePoint as the First Serious Connector

The existing Rust SharePoint REST/MCP library is a natural first connector because it already provides:

- document discovery
- file download
- search
- permissions
- change tracking
- metadata
- read-only safety controls

The RAG system should not be SharePoint-specific, but SharePoint is a strong proving ground for connector traits, incremental sync, citations, and permission-aware retrieval.

### 4. Zenoh for External Extensions

Standalone extensions should be able to register with the RAG runtime over Zenoh.

Zenoh is a good fit because it supports Rust and Python, and provides pub/sub plus query/queryable patterns that can act like request/reply calls.

Use Zenoh for:

- extension discovery
- capability registration
- heartbeats
- request/reply calls
- indexing progress events
- distributed extension deployments

Do not use Zenoh as the primary durable storage layer for documents or embeddings. Durable state should remain in explicit stores such as PostgreSQL/pgvector, object storage, or filesystem-backed stores.

### 5. Python Extensions as External Workers

Python extensions should usually run out-of-process.

This avoids embedding Python into the Rust service and keeps Python dependency complexity isolated. A Python loader, embedder, reranker, or metadata enricher can fail or restart without taking down the RAG/MCP server.

Preferred boundary:

```text
Rust RAG runtime <-> Zenoh <-> Python extension process
```

Calls across the boundary should be coarse-grained:

- `load_document(bytes, metadata) -> extracted text + sections + metadata`
- `embed_texts(batch[text]) -> batch[vectors]`
- `rerank(query, candidates) -> reordered candidates`
- `apply_acl(user_context, candidates) -> allowed candidates`
- `summarize_context(chunks) -> summary`

Avoid calling Python once per token, once per tiny chunk, or once per metadata field.

### 6. PyO3 as Optional Python Consumer Binding

PyO3 is still useful, but mostly for Python users who want to call the Rust RAG library from Python code.

Example:

```python
from rag_core import RagClient

rag = RagClient.open("rag.toml")
results = rag.search("thermal constraints", k=10)
```

For extension/plugin execution, prefer the Zenoh process boundary. For Python application integration, PyO3 plus `maturin` can provide a clean developer experience.

### 7. Stable Protocol Before Transport Optimization

The extension protocol should be versioned and transport-neutral. Zenoh should be one implementation of that protocol.

Initial messages can use JSON for debuggability. If payload size or performance becomes a problem, move to MessagePack, CBOR, or Protobuf while preserving the conceptual schema.

Example capability descriptor:

```json
{
  "extension_id": "python.pdf_loader",
  "protocol_version": "rag.extension.v1",
  "capabilities": ["load_document"],
  "content_types": ["application/pdf"],
  "max_payload_bytes": 104857600,
  "supports_streaming": true
}
```

Example request envelope:

```json
{
  "protocol": "rag.extension.v1",
  "request_id": "req_123",
  "operation": "load_document",
  "payload": {},
  "context": {
    "tenant_id": "default",
    "user_id": "user_123",
    "trace_id": "trace_123"
  }
}
```

### 8. Citation-Ready Results

Search results should be model-ready and source-aware, not raw vector-store tuples.

Preferred result shape:

```json
{
  "chunk_id": "chunk_123",
  "document_id": "doc_123",
  "title": "Quarterly Report",
  "source_url": "https://example/doc",
  "snippet": "Relevant text...",
  "score": 0.82,
  "page": 4,
  "chunk_index": 12,
  "modified_at": "2026-05-31T10:00:00Z",
  "citation": {
    "label": "Quarterly Report, p. 4",
    "url": "https://example/doc"
  }
}
```

### 9. Permission-Aware Retrieval

The core should support permission filtering without assuming one permission system.

Connectors should be able to attach permission metadata. Retrieval should accept a user or caller context and filter results before returning them to MCP, HTTP, or Python callers.

Permission enforcement should be pluggable:

- connector-native ACLs
- group-based filters
- tenant filters
- custom extension filters
- external authorization service checks

## Proposed Zenoh Keyspace

```text
rag/extensions/{extension_id}/announce
rag/extensions/{extension_id}/heartbeat
rag/extensions/{extension_id}/capabilities

rag/call/{extension_id}/load
rag/call/{extension_id}/embed
rag/call/{extension_id}/transform
rag/call/{extension_id}/rerank
rag/call/{extension_id}/filter

rag/events/index_started
rag/events/index_progress
rag/events/document_indexed
rag/events/index_failed
rag/events/sync_started
rag/events/sync_completed
```

## Preliminary Roadmap

### Phase 0: Design Spike

- Define core domain models: documents, chunks, sources, citations, search results.
- Define connector, embedder, vector store, chunker, and retriever traits.
- Define extension protocol envelope and capability descriptor.
- Decide initial serialization format.
- Create minimal crate layout.

Exit criteria:

- A small Rust prototype can index in-memory text and return citation-ready search results.

### Phase 1: Rust Core MVP

- Implement `rag-core`.
- Implement a simple chunker.
- Implement an in-memory vector store for tests.
- Implement one embedding provider abstraction with a mock embedder.
- Implement basic indexing and search orchestration.
- Add unit tests around chunking, metadata propagation, and result shaping.

Exit criteria:

- The core library can index documents and search them without MCP, HTTP, Python, or external storage.

### Phase 2: pgvector Store

- Implement PostgreSQL/pgvector storage.
- Store document metadata, chunk metadata, embeddings, source IDs, and digests.
- Add migration strategy.
- Add delete/reindex by source or document ID.
- Add index inspection APIs.

Exit criteria:

- A local Postgres/pgvector setup can persist and retrieve indexed content.

### Phase 3: SharePoint Connector

- Implement a connector adapter over the Rust SharePoint library.
- Support file discovery, file download, metadata extraction, and stable document IDs.
- Add incremental sync using SharePoint change tracking.
- Attach source URL, title, path, author, modified time, version, and permission hints.

Exit criteria:

- A SharePoint library or folder can be indexed and incrementally synced.

### Phase 4: Zenoh Extension Bus

- Implement `rag-extension-protocol`.
- Implement `rag-zenoh` extension registry.
- Support extension announcement, heartbeat, and capability lookup.
- Support request/reply for at least one operation, such as `load_document`.
- Build a small Python SDK that hides Zenoh details from extension authors.

Exit criteria:

- A standalone Python extension can register itself and serve document loading requests from the Rust runtime.

### Phase 5: MCP Layer

- Implement `rag-mcp`.
- Expose high-level tools for search, indexing, source inspection, and sync.
- Return model-ready document cards and citation-ready search results.
- Add read-only/safety mode for indexing and mutation operations.

Exit criteria:

- An MCP client can search indexed content and inspect cited documents.

### Phase 6: Python Consumer API

- Decide whether Python users need PyO3 bindings, a Zenoh client SDK, or both.
- If PyO3 is needed, expose coarse-grained APIs with `maturin`.
- If Zenoh client SDK is enough, provide a pure Python package for search and extension authoring.

Exit criteria:

- Python users can search, index, or author extensions without depending on the old FastAPI codebase.

### Phase 7: HTTP and Migration Path

- Add optional HTTP API if needed.
- Map current useful Python RAG API concepts into the new system.
- Port selected tests from the current repository as behavioral checks.
- Document migration from the current RAG API to the Rust RAG/MCP platform.

Exit criteria:

- Existing users have a clear path to adopt the new system without a hard cutover.

## Open Questions

- Should `rag-mcp` live as a feature in one crate or as a separate crate depending on `rag-core`?
- Which embedding providers should be native Rust first?
- Should document parsing be mostly Rust-native, mostly Python-extension-driven, or hybrid?
- Should the extension protocol use JSON initially, or start with MessagePack/Protobuf?
- What permission model is the minimum viable abstraction?
- Should HTTP compatibility with the current FastAPI service be a goal, or should migration prefer new APIs?
- How much agent orchestration belongs in this project versus external agent frameworks?

## Recommended First Implementation Slice

Start with a narrow vertical slice:

1. Rust `rag-core` with in-memory indexing and search.
2. pgvector persistence.
3. SharePoint connector indexing one library.
4. Citation-ready search results.
5. MCP `rag_search` and `rag_get_document`.
6. One Python extension over Zenoh for custom document loading.

This proves the main architecture without prematurely committing to every connector, every embedding provider, or full backwards compatibility.
