-- Requires the pgvector extension: CREATE EXTENSION IF NOT EXISTS vector;
-- The extension is created by PgVectorStore::connect() before migrations run.

-- Stores arbitrary key/value config, including the embedding dimension so a
-- dimension mismatch between the configured embedder and an existing index
-- is caught at startup rather than at insert time.
CREATE TABLE IF NOT EXISTS rag_meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);

-- One row per indexed document chunk. The `embedding` column is added by
-- PgVectorStore::connect() after migrations run, because its type must encode
-- the dimension (e.g. vector(384)) which is only known at runtime.
CREATE TABLE IF NOT EXISTS rag_chunks (
    id            TEXT        PRIMARY KEY,
    document_id   TEXT        NOT NULL,
    source_id     TEXT        NOT NULL,
    chunk_index   INTEGER     NOT NULL,
    text          TEXT        NOT NULL,
    metadata      JSONB       NOT NULL DEFAULT '{}',
    created_at    TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_rag_chunks_document_id ON rag_chunks (document_id);
CREATE INDEX IF NOT EXISTS idx_rag_chunks_source_id   ON rag_chunks (source_id);
