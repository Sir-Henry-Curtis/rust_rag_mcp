# rust-rag-mcp

A library-first RAG (Retrieval-Augmented Generation) platform and MCP server for enterprise document search, written in Rust.

Designed to work with large corpora — hundreds of documents, each hundreds of pages, in formats like PDF, Word, Excel, and Markdown — surfaced through a clean MCP interface for use with Claude and other AI assistants.

## Key Features

- **Library-first architecture** — `rag-core` is a pure Rust library. MCP, HTTP, and Python bindings are layers on top, not the core.
- **Citation-ready results** — every search result includes structured citation metadata (title, page, URL, label) ready to embed directly in LLM responses.
- **Permission-aware retrieval** — permission hints are attached at index time and enforced at retrieval time. Works with connector-native ACLs.
- **Incremental sync** — connectors use change tokens to sync only what changed since the last run.
- **Pluggable extension workers** — document loaders, embedders, and rerankers can run as external processes (Rust or Python) connected via Zenoh.
- **Cross-platform** — targets Linux, macOS, and Windows. No platform-specific code in `rag-core`.

## Architecture

```
┌─────────────────────────────────────────────────────────────┐
│  Consumers                                                  │
│  rag-mcp (MCP tools)  rag-server (HTTP)  rag-py (Python)   │
└──────────────────────────────┬──────────────────────────────┘
                               │
┌──────────────────────────────▼──────────────────────────────┐
│  rag-core                                                   │
│  Domain models · Traits · Indexer · Retriever               │
│  Chunker · Embedder · VectorStore · PermissionFilter        │
└────────────┬──────────────────────────┬─────────────────────┘
             │                          │
┌────────────▼──────────┐  ┌───────────▼─────────────────────┐
│  rag-connectors       │  │  rag-zenoh  (extension bus)     │
│  SharePoint · FS · S3 │  │  PDF loader · DOCX · XLSX       │
└───────────────────────┘  │  OpenAI embedder · Rerankers    │
                           │  (out-of-process, any language) │
┌──────────────────────────▼──────────────────────────────────┐
│  rag-store-pgvector                                         │
│  PostgreSQL + pgvector durable storage                      │
└─────────────────────────────────────────────────────────────┘
```

## Crate Overview

| Crate | Status | Purpose |
|---|---|---|
| `rag-core` | Implemented (Phase 1) | Domain models, all traits, ParagraphChunker, MockEmbedder, MemoryVectorStore, Indexer, StandardRetriever |
| `rag-extension-protocol` | Implemented | Versioned `rag.extension.v1` JSON envelopes for external workers |
| `rag-store-pgvector` | Phase 2 | PostgreSQL + pgvector durable store |
| `rag-connectors` | Phase 3 | SharePoint, filesystem, S3, Git connectors |
| `rag-zenoh` | Phase 4 | Zenoh pub/sub extension bus for out-of-process workers |
| `rag-mcp` | Phase 5 | MCP server (rag_search, rag_get_document, rag_index_source, …) |
| `rag-server` | Phase 7 | Optional axum HTTP API |

## Quick Start

```toml
# Cargo.toml
[dependencies]
rag-core = { git = "https://github.com/your-org/rust-rag-mcp" }
```

```rust
use std::sync::Arc;
use rag_core::{
    chunker::ParagraphChunker,
    embedder::MockEmbedder,     // swap for OpenAIEmbedder in production
    indexer::Indexer,
    models::{Document, DocumentId, DocumentMetadata, SourceId},
    retriever::StandardRetriever,
    store::MemoryVectorStore,   // swap for PgVectorStore in production
    traits::Retriever,
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let store    = Arc::new(MemoryVectorStore::default());
    let chunker  = Arc::new(ParagraphChunker::default());
    let embedder = Arc::new(MockEmbedder::default());

    let indexer   = Indexer::new(chunker, embedder.clone(), store.clone());
    let retriever = StandardRetriever::new(embedder, store);

    // Index a document
    let doc = Document {
        id:        DocumentId::new(),
        source_id: SourceId::from_str("my-source"),
        title:     "Quarterly Report Q3".into(),
        content:   "... extracted text from PDF ...".into(),
        url:       Some("https://sharepoint.example.com/sites/finance/Q3.pdf".into()),
        metadata:  DocumentMetadata::default(),
    };

    indexer.index_document(&doc).await?;

    // Search and get citation-ready results
    let results = retriever.search("thermal constraints", 5, None, None).await?;

    for r in &results {
        println!("{:.3}  {}  {}", r.score, r.citation.label, r.snippet);
    }

    Ok(())
}
```

## MCP Tools (Phase 5)

Once `rag-mcp` is implemented, the following tools will be available in any MCP-compatible host (Claude Desktop, Cursor, etc.):

| Tool | Description |
|---|---|
| `rag_search` | Semantic search over all indexed content; returns citation-ready results |
| `rag_get_document` | Retrieve a full document and its chunks by document ID |
| `rag_get_context` | Fetch ranked context passages for an LLM prompt |
| `rag_index_source` | Trigger a full index of a registered source |
| `rag_sync_source` | Incremental sync from a source's change feed |
| `rag_list_sources` | List all registered sources and their index status |
| `rag_explain_match` | Explain why a chunk matched a query (audit / debug) |

Set `RAG_READ_ONLY=true` to block all mutation tools.

## Platform Support

| Platform | Tier | Notes |
|---|---|---|
| Linux (Ubuntu 22.04+, RHEL 8+) | Tier 1 | Primary deployment target |
| macOS (Apple Silicon + Intel) | Tier 1 | Primary developer target |
| Windows 11 / Server 2022+ | Tier 2 | MCP client host; CI tested |

### Windows Build Note

When running `cargo build` or `cargo check` outside a Visual Studio Developer Command Prompt, the MSVC linker needs to find Windows SDK libraries. The `.cargo/config.toml` in this repo sets `LIB` and `INCLUDE` for MSVC 14.51 and Windows SDK 10.0.26100.0. Update those paths if you upgrade Visual Studio Build Tools or the Windows SDK.

On Linux and macOS no special configuration is needed.

## Running Tests

```sh
cargo test -p rag-core   # unit + integration tests for the core library
cargo test --workspace   # all crates
```

## License

Apache License, Version 2.0. See [LICENSE](LICENSE) for the full text.

This project uses third-party open-source libraries. All dependencies are permissively licensed (MIT, Apache-2.0, Unlicense, Zlib, Unicode-3.0). See [NOTICE](NOTICE) for the full attribution list.

To generate a complete SPDX license report for a distribution:

```sh
cargo install cargo-about
cargo about generate about.hbs > third-party-licenses.html
```

## Roadmap

See [ROADMAP.md](ROADMAP.md) for the full plan toward v1.0.

## Additional Docs

- [Understanding RAG](docs/understanding-rag.md) explains why this project exists and how it complements direct SharePoint MCP access.
- [Using rust-rag-mcp in Larger Systems](docs/using-rag-in-larger-systems.md) describes evaluation workflows, human approval flows, and agentic research patterns.
- [VectorStore Backend Comparison](docs/vectorstore-backend-comparison.md) compares pgvector, OpenSearch, Elasticsearch, Qdrant, Weaviate, Pinecone, and other adapter candidates.
- [Multi-modal Indexing Design](docs/multimodal-indexing-design.md) outlines how diagrams, charts, screenshots, and scanned pages can be indexed through vision extension workers.
- [Query Rewriting and Conversation-Aware Retrieval](docs/query-rewriting-and-conversation-retrieval.md) details query expansion, follow-up resolution, and multi-query retrieval planning.
- [Federated Search Design](docs/federated-search-design.md) describes fan-out search across multiple independent RAG instances and merged citation-ready results.

## Contributing

Issues and pull requests are welcome. Please read the ROADMAP before opening a large feature PR to confirm it aligns with the planned direction.
