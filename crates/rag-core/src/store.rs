use async_trait::async_trait;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::RagError;
use crate::models::{Chunk, DocumentId, SearchFilter, ScoredChunk, SourceId};
use crate::traits::VectorStore;

struct Entry {
    chunk: Chunk,
    embedding: Vec<f32>,
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> f32 {
    let dot: f32 = a.iter().zip(b).map(|(x, y)| x * y).sum();
    let na: f32 = a.iter().map(|x| x * x).sum::<f32>().sqrt();
    let nb: f32 = b.iter().map(|x| x * x).sum::<f32>().sqrt();
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na * nb) }
}

/// Thread-safe in-memory vector store backed by a `Vec<Entry>`.
///
/// Suitable for unit tests and the Phase 0 design spike.
/// Phase 2 replaces this with `PgVectorStore` for durable persistence.
#[derive(Default)]
pub struct MemoryVectorStore {
    entries: Arc<RwLock<Vec<Entry>>>,
}

#[async_trait]
impl VectorStore for MemoryVectorStore {
    async fn upsert_chunks(&self, chunks: &[Chunk]) -> Result<(), RagError> {
        let mut store = self.entries.write().await;
        for chunk in chunks {
            let embedding = chunk.embedding.clone().ok_or_else(|| {
                RagError::Store(format!("chunk {} has no embedding", chunk.id))
            })?;
            store.retain(|e| e.chunk.id != chunk.id);
            store.push(Entry { chunk: chunk.clone(), embedding });
        }
        Ok(())
    }

    async fn search(
        &self,
        embedding: &[f32],
        k: usize,
        filter: Option<&SearchFilter>,
    ) -> Result<Vec<ScoredChunk>, RagError> {
        let store = self.entries.read().await;

        let mut scored: Vec<ScoredChunk> = store
            .iter()
            .filter(|e| {
                let Some(f) = filter else { return true };
                if !f.source_ids.is_empty() && !f.source_ids.contains(&e.chunk.source_id) {
                    return false;
                }
                if !f.document_ids.is_empty() && !f.document_ids.contains(&e.chunk.document_id) {
                    return false;
                }
                true
            })
            .map(|e| ScoredChunk {
                chunk: e.chunk.clone(),
                score: cosine_similarity(embedding, &e.embedding),
            })
            .collect();

        scored.sort_by(|a, b| {
            b.score
                .partial_cmp(&a.score)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        scored.truncate(k);
        Ok(scored)
    }

    async fn delete_by_document(&self, document_id: &DocumentId) -> Result<(), RagError> {
        self.entries
            .write()
            .await
            .retain(|e| &e.chunk.document_id != document_id);
        Ok(())
    }

    async fn delete_by_source(&self, source_id: &SourceId) -> Result<(), RagError> {
        self.entries
            .write()
            .await
            .retain(|e| &e.chunk.source_id != source_id);
        Ok(())
    }

    async fn count_chunks(&self) -> Result<usize, RagError> {
        Ok(self.entries.read().await.len())
    }
}
