use async_trait::async_trait;
use chrono::{DateTime, Utc};

use crate::error::RagError;
use crate::models::{
    CallerContext, Chunk, Document, DocumentId, DocumentRef, SearchFilter, SearchResult,
    ScoredChunk, SourceId,
};

// ── Connector ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub enum ChangeKind {
    Created,
    Modified,
    Deleted,
}

#[derive(Debug, Clone)]
pub struct ChangeEvent {
    pub document_ref: DocumentRef,
    pub kind: ChangeKind,
    pub occurred_at: DateTime<Utc>,
}

/// A source connector discovers and loads documents from one external system.
///
/// Implementors: SharePointConnector, FilesystemConnector, S3Connector, etc.
#[async_trait]
pub trait Connector: Send + Sync {
    fn source_id(&self) -> &SourceId;
    fn kind(&self) -> &str;

    /// List all documents available in the source.
    async fn list_documents(&self) -> Result<Vec<DocumentRef>, RagError>;

    /// Load the full text content of a specific document.
    async fn load_document(&self, doc_ref: &DocumentRef) -> Result<Document, RagError>;

    /// Return changes since `token`. Returns `(events, new_token)`.
    /// Pass `None` for a full initial sync.
    async fn changes_since(
        &self,
        token: Option<&str>,
    ) -> Result<(Vec<ChangeEvent>, String), RagError>;
}

// ── Chunker ───────────────────────────────────────────────────────────────────

/// Splits a loaded document into text spans ready for embedding.
///
/// Implementors: ParagraphChunker, SentenceChunker, MarkdownChunker, etc.
#[async_trait]
pub trait Chunker: Send + Sync {
    fn name(&self) -> &str;
    async fn chunk(&self, document: &Document) -> Result<Vec<Chunk>, RagError>;
}

// ── Embedder ──────────────────────────────────────────────────────────────────

/// Converts text spans into dense embedding vectors.
///
/// Implementors: MockEmbedder, OpenAIEmbedder, LocalOnnxEmbedder, etc.
#[async_trait]
pub trait Embedder: Send + Sync {
    fn name(&self) -> &str;
    fn dimension(&self) -> usize;

    async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RagError>;

    async fn embed_query(&self, query: &str) -> Result<Vec<f32>, RagError> {
        let mut results = self.embed_texts(&[query]).await?;
        results
            .pop()
            .ok_or_else(|| RagError::Embedder("embedder returned empty batch".into()))
    }
}

// ── VectorStore ───────────────────────────────────────────────────────────────

/// Durable storage for chunk embeddings and metadata.
///
/// Implementors: MemoryVectorStore, PgVectorStore, etc.
#[async_trait]
pub trait VectorStore: Send + Sync {
    async fn upsert_chunks(&self, chunks: &[Chunk]) -> Result<(), RagError>;

    async fn search(
        &self,
        embedding: &[f32],
        k: usize,
        filter: Option<&SearchFilter>,
    ) -> Result<Vec<ScoredChunk>, RagError>;

    async fn delete_by_document(&self, document_id: &DocumentId) -> Result<(), RagError>;
    async fn delete_by_source(&self, source_id: &SourceId) -> Result<(), RagError>;
    async fn count_chunks(&self) -> Result<usize, RagError>;
}

// ── Retriever ─────────────────────────────────────────────────────────────────

/// Orchestrates query embedding → vector search → permission filtering → citations.
#[async_trait]
pub trait Retriever: Send + Sync {
    async fn search(
        &self,
        query: &str,
        k: usize,
        filter: Option<&SearchFilter>,
        caller: Option<&CallerContext>,
    ) -> Result<Vec<SearchResult>, RagError>;
}

// ── PermissionFilter ──────────────────────────────────────────────────────────

/// Post-retrieval ACL enforcement. Receives overfetched candidates,
/// returns only those the caller is permitted to see.
#[async_trait]
pub trait PermissionFilter: Send + Sync {
    async fn filter(
        &self,
        caller: &CallerContext,
        chunks: Vec<ScoredChunk>,
    ) -> Result<Vec<ScoredChunk>, RagError>;
}

// ── Reranker ──────────────────────────────────────────────────────────────────

/// Optional cross-encoder or LLM-based reranking pass after vector search.
#[async_trait]
pub trait Reranker: Send + Sync {
    async fn rerank(
        &self,
        query: &str,
        candidates: Vec<ScoredChunk>,
    ) -> Result<Vec<ScoredChunk>, RagError>;
}
