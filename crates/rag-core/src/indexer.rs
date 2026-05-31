use std::sync::Arc;

use tracing::{debug, info};

use crate::error::RagError;
use crate::models::{Document, DocumentId, SourceId};
use crate::traits::{Chunker, Embedder, VectorStore};

/// Orchestrates the ingest pipeline: chunk → embed → store.
pub struct Indexer {
    chunker: Arc<dyn Chunker>,
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn VectorStore>,
}

impl Indexer {
    pub fn new(
        chunker: Arc<dyn Chunker>,
        embedder: Arc<dyn Embedder>,
        store: Arc<dyn VectorStore>,
    ) -> Self {
        Self { chunker, embedder, store }
    }

    /// Index a single document. Returns the number of chunks stored.
    pub async fn index_document(&self, document: &Document) -> Result<usize, RagError> {
        info!(
            document_id = %document.id,
            title = %document.title,
            "indexing document"
        );

        let mut chunks = self.chunker.chunk(document).await?;

        let texts: Vec<&str> = chunks.iter().map(|c| c.text.as_str()).collect();
        let embeddings = self.embedder.embed_texts(&texts).await?;

        for (chunk, embedding) in chunks.iter_mut().zip(embeddings) {
            chunk.embedding = Some(embedding);
        }

        let count = chunks.len();
        self.store.upsert_chunks(&chunks).await?;

        debug!(
            document_id = %document.id,
            chunk_count = count,
            "document indexed"
        );
        Ok(count)
    }

    /// Remove all chunks for a document (e.g. on delete or full reindex).
    pub async fn delete_document(&self, document_id: &DocumentId) -> Result<(), RagError> {
        self.store.delete_by_document(document_id).await
    }

    /// Remove all chunks for a source (e.g. when a source is deregistered).
    pub async fn delete_source(&self, source_id: &SourceId) -> Result<(), RagError> {
        self.store.delete_by_source(source_id).await
    }
}
