use async_trait::async_trait;
use sqlx::{PgPool, Row};
use tracing::{debug, info};

use rag_core::{
    RagError,
    models::{Chunk, ChunkId, ChunkMetadata, DocumentId, SearchFilter, ScoredChunk, SourceId},
    traits::VectorStore,
};

/// Encode a `Vec<f32>` as a pgvector text literal: `[1.0,0.0,…]`.
///
/// PostgreSQL casts this to the `vector` type via `$1::vector`. This approach
/// avoids depending on the pgvector crate's sqlx feature, which has
/// version-coupling issues across sqlx minor releases.
fn encode_vec(v: &[f32]) -> String {
    let body = v
        .iter()
        .map(|x| format!("{x:.8}"))
        .collect::<Vec<_>>()
        .join(",");
    format!("[{body}]")
}

/// PostgreSQL + pgvector implementation of [`VectorStore`].
///
/// The `embedding` column and HNSW index are created dynamically in
/// [`PgVectorStore::connect`] because the column type encodes the dimension
/// (e.g. `vector(384)`) which is only known at runtime.
#[derive(Debug)]
pub struct PgVectorStore {
    pool: PgPool,
    dimension: usize,
}

impl PgVectorStore {
    /// Connect to Postgres, run migrations, and prepare the vector column and
    /// HNSW index for the given embedding `dimension`.
    ///
    /// Returns an error if an existing index was built for a different
    /// dimension, preventing silent type mismatches at insert time.
    pub async fn connect(url: &str, dimension: usize) -> Result<Self, RagError> {
        let pool = PgPool::connect(url)
            .await
            .map_err(|e| RagError::Store(format!("connect: {e}")))?;

        // pgvector extension must exist before migrations or ALTER TABLE.
        sqlx::query("CREATE EXTENSION IF NOT EXISTS vector")
            .execute(&pool)
            .await
            .map_err(|e| RagError::Store(format!("create extension: {e}")))?;

        // Run embedded migrations — uses CARGO_MANIFEST_DIR/migrations.
        sqlx::migrate!()
            .run(&pool)
            .await
            .map_err(|e| RagError::Store(format!("migrate: {e}")))?;

        // Guard against dimension mismatch with an existing index.
        let dim_str = dimension.to_string();
        let existing: Option<String> = sqlx::query_scalar(
            "SELECT value FROM rag_meta WHERE key = 'embedding_dimension'",
        )
        .fetch_optional(&pool)
        .await
        .map_err(|e| RagError::Store(format!("read meta: {e}")))?;

        match existing.as_deref() {
            None => {
                sqlx::query(
                    "INSERT INTO rag_meta (key, value) VALUES ('embedding_dimension', $1)",
                )
                .bind(&dim_str)
                .execute(&pool)
                .await
                .map_err(|e| RagError::Store(format!("write meta: {e}")))?;
            }
            Some(stored) if stored != dim_str => {
                return Err(RagError::Store(format!(
                    "embedding dimension mismatch: index was built with dimension {stored}, \
                     configured embedder has dimension {dimension}. \
                     Re-index the corpus or connect with dimension={stored}."
                )));
            }
            _ => {}
        }

        // Add the typed vector column if this is the first connect.
        sqlx::query(&format!(
            "ALTER TABLE rag_chunks \
             ADD COLUMN IF NOT EXISTS embedding vector({dimension})"
        ))
        .execute(&pool)
        .await
        .map_err(|e| RagError::Store(format!("add embedding column: {e}")))?;

        // HNSW index — no-op if it already exists.
        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_rag_chunks_embedding \
             ON rag_chunks USING hnsw (embedding vector_cosine_ops)",
        )
        .execute(&pool)
        .await
        .map_err(|e| RagError::Store(format!("create hnsw index: {e}")))?;

        info!(dimension, "PgVectorStore ready");
        Ok(Self { pool, dimension })
    }

    // ── Inspection helpers (not on the trait) ─────────────────────────────

    /// Returns `(source_id, chunk_count)` pairs for every source in the store.
    pub async fn chunk_counts_by_source(&self) -> Result<Vec<(String, i64)>, RagError> {
        let rows = sqlx::query(
            "SELECT source_id, COUNT(*)::bigint AS n \
             FROM rag_chunks \
             GROUP BY source_id \
             ORDER BY source_id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RagError::Store(format!("chunk_counts_by_source: {e}")))?;

        rows.iter()
            .map(|r| {
                let sid: String = r.try_get("source_id")
                    .map_err(|e| RagError::Store(e.to_string()))?;
                let n: i64 = r.try_get("n")
                    .map_err(|e| RagError::Store(e.to_string()))?;
                Ok((sid, n))
            })
            .collect()
    }

    /// Timestamp of the most recently indexed chunk for `source_id`.
    pub async fn last_indexed_at(
        &self,
        source_id: &SourceId,
    ) -> Result<Option<chrono::DateTime<chrono::Utc>>, RagError> {
        let ts: Option<chrono::DateTime<chrono::Utc>> = sqlx::query_scalar(
            "SELECT MAX(created_at) FROM rag_chunks WHERE source_id = $1",
        )
        .bind(&source_id.0)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| RagError::Store(format!("last_indexed_at: {e}")))?;

        Ok(ts)
    }
}

// ── VectorStore trait ─────────────────────────────────────────────────────────

#[async_trait]
impl VectorStore for PgVectorStore {
    async fn upsert_chunks(&self, chunks: &[Chunk]) -> Result<(), RagError> {
        if chunks.is_empty() {
            return Ok(());
        }

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| RagError::Store(format!("begin tx: {e}")))?;

        for chunk in chunks {
            let embedding = chunk.embedding.as_ref().ok_or_else(|| {
                RagError::Store(format!("chunk {} has no embedding", chunk.id))
            })?;

            if embedding.len() != self.dimension {
                return Err(RagError::Store(format!(
                    "chunk {} embedding has {} dimensions but store expects {}",
                    chunk.id,
                    embedding.len(),
                    self.dimension
                )));
            }

            let vec_literal = encode_vec(embedding);
            let metadata = serde_json::to_value(&chunk.metadata)
                .map_err(RagError::Serialization)?;

            sqlx::query(&format!(
                "INSERT INTO rag_chunks \
                     (id, document_id, source_id, chunk_index, text, embedding, metadata) \
                 VALUES ($1, $2, $3, $4, $5, $6::vector({dim}), $7) \
                 ON CONFLICT (id) DO UPDATE SET \
                     document_id = EXCLUDED.document_id, \
                     source_id   = EXCLUDED.source_id, \
                     chunk_index = EXCLUDED.chunk_index, \
                     text        = EXCLUDED.text, \
                     embedding   = EXCLUDED.embedding, \
                     metadata    = EXCLUDED.metadata",
                dim = self.dimension
            ))
            .bind(&chunk.id.0)
            .bind(&chunk.document_id.0)
            .bind(&chunk.source_id.0)
            .bind(chunk.chunk_index as i32)
            .bind(&chunk.text)
            .bind(&vec_literal)
            .bind(metadata)
            .execute(&mut *tx)
            .await
            .map_err(|e| RagError::Store(format!("upsert chunk {}: {e}", chunk.id)))?;
        }

        tx.commit()
            .await
            .map_err(|e| RagError::Store(format!("commit: {e}")))?;

        debug!(count = chunks.len(), "upserted chunks");
        Ok(())
    }

    async fn search(
        &self,
        embedding: &[f32],
        k: usize,
        filter: Option<&SearchFilter>,
    ) -> Result<Vec<ScoredChunk>, RagError> {
        if embedding.len() != self.dimension {
            return Err(RagError::Store(format!(
                "query embedding has {} dimensions but store expects {}",
                embedding.len(),
                self.dimension
            )));
        }

        let vec_literal = encode_vec(embedding);

        // Empty vecs mean "no filter on this field".
        let source_ids: Vec<&str> = filter
            .map(|f| f.source_ids.iter().map(|s| s.0.as_str()).collect())
            .unwrap_or_default();
        let document_ids: Vec<&str> = filter
            .map(|f| f.document_ids.iter().map(|d| d.0.as_str()).collect())
            .unwrap_or_default();

        // array_length returns NULL for an empty array → the IS NULL branch
        // means "skip this filter" when no IDs are provided.
        let rows = sqlx::query(&format!(
            "SELECT id, document_id, source_id, chunk_index, text, metadata, \
                    (1.0 - (embedding <=> $1::vector({dim})))::float4 AS score \
             FROM rag_chunks \
             WHERE (array_length($2::text[], 1) IS NULL OR source_id   = ANY($2::text[])) \
               AND (array_length($3::text[], 1) IS NULL OR document_id = ANY($3::text[])) \
             ORDER BY embedding <=> $1::vector({dim}) \
             LIMIT $4",
            dim = self.dimension
        ))
        .bind(&vec_literal)
        .bind(&source_ids[..])
        .bind(&document_ids[..])
        .bind(k as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| RagError::Store(format!("search: {e}")))?;

        rows.iter()
            .map(|row| {
                let metadata_val: serde_json::Value = row
                    .try_get("metadata")
                    .map_err(|e| RagError::Store(e.to_string()))?;
                let metadata: ChunkMetadata =
                    serde_json::from_value(metadata_val).unwrap_or_default();

                Ok(ScoredChunk {
                    chunk: Chunk {
                        id: ChunkId::from_str(
                            row.try_get::<String, _>("id")
                                .map_err(|e| RagError::Store(e.to_string()))?,
                        ),
                        document_id: DocumentId::from_str(
                            row.try_get::<String, _>("document_id")
                                .map_err(|e| RagError::Store(e.to_string()))?,
                        ),
                        source_id: SourceId::from_str(
                            row.try_get::<String, _>("source_id")
                                .map_err(|e| RagError::Store(e.to_string()))?,
                        ),
                        chunk_index: row
                            .try_get::<i32, _>("chunk_index")
                            .map_err(|e| RagError::Store(e.to_string()))?
                            as u32,
                        text: row
                            .try_get("text")
                            .map_err(|e| RagError::Store(e.to_string()))?,
                        embedding: None,
                        metadata,
                    },
                    score: row
                        .try_get::<f32, _>("score")
                        .map_err(|e| RagError::Store(e.to_string()))?,
                })
            })
            .collect()
    }

    async fn delete_by_document(&self, document_id: &DocumentId) -> Result<(), RagError> {
        sqlx::query("DELETE FROM rag_chunks WHERE document_id = $1")
            .bind(&document_id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| RagError::Store(format!("delete_by_document: {e}")))?;
        Ok(())
    }

    async fn delete_by_source(&self, source_id: &SourceId) -> Result<(), RagError> {
        sqlx::query("DELETE FROM rag_chunks WHERE source_id = $1")
            .bind(&source_id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| RagError::Store(format!("delete_by_source: {e}")))?;
        Ok(())
    }

    async fn count_chunks(&self) -> Result<usize, RagError> {
        let n: i64 = sqlx::query_scalar("SELECT COUNT(*)::bigint FROM rag_chunks")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| RagError::Store(format!("count_chunks: {e}")))?;
        Ok(n as usize)
    }
}
