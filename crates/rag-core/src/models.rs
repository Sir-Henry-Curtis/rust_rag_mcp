use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

// ── IDs ──────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SourceId(pub String);

impl SourceId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for SourceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct DocumentId(pub String);

impl DocumentId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for DocumentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChunkId(pub String);

impl ChunkId {
    pub fn new() -> Self {
        Self(Uuid::new_v4().to_string())
    }

    pub fn from_str(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

impl std::fmt::Display for ChunkId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

// ── Source ────────────────────────────────────────────────────────────────────

/// A registered document source (SharePoint library, S3 bucket, filesystem, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Source {
    pub id: SourceId,
    pub name: String,
    /// Connector kind, e.g. "sharepoint", "filesystem", "s3".
    pub kind: String,
    pub base_url: Option<String>,
    pub config: serde_json::Value,
}

// ── Document ──────────────────────────────────────────────────────────────────

/// A lightweight reference returned by a connector's discovery pass,
/// before the full text content is loaded.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentRef {
    pub id: DocumentId,
    pub source_id: SourceId,
    pub title: String,
    pub url: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub content_type: Option<String>,
    pub size_bytes: Option<u64>,
}

/// A fully loaded document with extracted text ready for chunking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    pub id: DocumentId,
    pub source_id: SourceId,
    pub title: String,
    /// Plain-text content extracted from the original file.
    pub content: String,
    pub url: Option<String>,
    pub metadata: DocumentMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DocumentMetadata {
    pub author: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub created_at: Option<DateTime<Utc>>,
    pub version: Option<String>,
    /// MIME type or file extension, e.g. "application/pdf", "docx".
    pub file_type: Option<String>,
    pub page_count: Option<u32>,
    /// ACL hints forwarded from the connector for permission filtering.
    pub permissions: Option<Vec<String>>,
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

// ── Chunk ─────────────────────────────────────────────────────────────────────

/// A sub-document text span produced by a chunker, ready for embedding.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Chunk {
    pub id: ChunkId,
    pub document_id: DocumentId,
    pub source_id: SourceId,
    pub text: String,
    /// Zero-based position within the document's chunk sequence.
    pub chunk_index: u32,
    /// Populated by the embedder before the chunk is written to a store.
    pub embedding: Option<Vec<f32>>,
    pub metadata: ChunkMetadata,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChunkMetadata {
    pub document_title: String,
    pub document_url: Option<String>,
    /// Best-effort page number within a multi-page document.
    pub page: Option<u32>,
    pub section: Option<String>,
    pub modified_at: Option<DateTime<Utc>>,
    pub permissions: Option<Vec<String>>,
}

// ── Search ────────────────────────────────────────────────────────────────────

/// A chunk paired with its cosine similarity score from a vector search.
#[derive(Debug, Clone)]
pub struct ScoredChunk {
    pub chunk: Chunk,
    pub score: f32,
}

/// A fully decorated search result ready to hand to an LLM, including citation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchResult {
    pub chunk_id: ChunkId,
    pub document_id: DocumentId,
    pub source_id: SourceId,
    pub title: String,
    pub source_url: Option<String>,
    pub snippet: String,
    pub score: f32,
    pub page: Option<u32>,
    pub chunk_index: u32,
    pub modified_at: Option<DateTime<Utc>>,
    pub citation: Citation,
}

/// A human-readable source attribution ready to embed in an LLM response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Citation {
    pub label: String,
    pub url: Option<String>,
}

impl Citation {
    pub fn build(title: &str, page: Option<u32>, url: Option<String>) -> Self {
        let label = match page {
            Some(p) => format!("{title}, p. {p}"),
            None => title.to_string(),
        };
        Self { label, url }
    }
}

// ── Context passed to permission filters and retrievers ───────────────────────

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CallerContext {
    pub user_id: Option<String>,
    pub groups: Vec<String>,
    pub tenant_id: Option<String>,
    #[serde(default)]
    pub extra: serde_json::Value,
}

/// Optional predicate applied before vector search to restrict results
/// to specific sources or document IDs.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SearchFilter {
    pub source_ids: Vec<SourceId>,
    pub document_ids: Vec<DocumentId>,
    pub content_types: Vec<String>,
}

impl SearchFilter {
    pub fn by_source(source_id: SourceId) -> Self {
        Self {
            source_ids: vec![source_id],
            ..Default::default()
        }
    }
}
