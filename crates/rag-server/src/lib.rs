//! Optional HTTP API server built on top of rag-core (Phase 7 implementation).
//!
//! Phase 7 work:
//!   - Add axum + tower-http.
//!   - Expose REST endpoints mirroring the MCP tool surface.
//!   - Provide a migration path from the existing Python FastAPI RAG service.

pub struct RagHttpServer;
