//! PostgreSQL + pgvector `VectorStore` implementation.
//!
//! Phase 2 work:
//!   - Add sqlx (postgres + runtime-tokio-rustls) and the pgvector crate.
//!   - Create a migration for `rag_chunks` table with a `vector(N)` column.
//!   - Implement `VectorStore` using `<->` cosine-distance operator.
//!   - Add delete/reindex by source or document ID.
//!   - Add index inspection APIs (count, list sources, etc.).

pub struct PgVectorStore;
