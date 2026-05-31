//! MCP server built on top of rag-core (Phase 5 implementation).
//!
//! ## Tools
//! | Tool                | Description                                              |
//! |---------------------|----------------------------------------------------------|
//! | `rag_search`        | Semantic search over all indexed content                 |
//! | `rag_get_document`  | Retrieve a full document and its chunks by document ID   |
//! | `rag_get_context`   | Fetch citation-ready context passages for an LLM prompt  |
//! | `rag_index_source`  | Trigger a full index of a registered source              |
//! | `rag_sync_source`   | Incremental sync from a source's change feed             |
//! | `rag_list_sources`  | List all registered sources and their index status       |
//! | `rag_explain_match` | Explain why a chunk matched a query (debug / audit)      |
//!
//! ## Safety
//! Mutation tools (`rag_index_source`, `rag_sync_source`) can be blocked
//! by setting `RAG_READ_ONLY=true` in the environment.

pub struct RagMcpServer;
