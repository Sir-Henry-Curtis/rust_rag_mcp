//! Zenoh extension bus for rag-core.
//!
//! Provides out-of-process extension workers (document loaders, embedders,
//! rerankers) connected over Zenoh pub/sub with request/reply semantics.
//!
//! ## Quick start
//!
//! ```no_run
//! use std::sync::Arc;
//! use rag_zenoh::{ZenohConfig, open_session, ExtensionRegistry, EventPublisher};
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let config = Arc::new(ZenohConfig::default());
//!     let session = Arc::new(open_session(&config).await?);
//!
//!     let registry = ExtensionRegistry::start(session.clone(), config.clone()).await?;
//!     let events = EventPublisher::new(session.clone(), config.clone());
//!     events.index_started("sp-finance").await?;
//!     Ok(())
//! }
//! ```
//!
//! ## Keyspace
//!
//! ```text
//! {prefix}/extensions/{id}/announce   ← CapabilityDescriptor (subscriber)
//! {prefix}/extensions/{id}/heartbeat  ← Heartbeat (subscriber)
//! {prefix}/call/{id}/load             → load_document request/reply (queryable)
//! {prefix}/call/{id}/embed            → embed_texts request/reply (queryable)
//! {prefix}/call/{id}/rerank           → rerank request/reply (queryable)
//! {prefix}/events/**                  ← indexing lifecycle events (publisher)
//! ```
//! Default `prefix` = `"rag"`.

pub mod call;
pub mod config;
pub mod events;
pub mod registry;

pub use call::ZenohCaller;
pub use config::{TlsConfig, ZenohConfig, ZenohMode};
pub use events::EventPublisher;
pub use registry::ExtensionRegistry;

use std::sync::Arc;

use anyhow::anyhow;
use async_trait::async_trait;
use rag_core::{
    ContentSection, RagError,
    models::{Document, DocumentMetadata, DocumentRef, ScoredChunk},
    traits::{Embedder, Reranker},
};
use rag_extension_protocol::{
    EmbedTextsRequest, LoadDocumentRequest, LoadDocumentResponse, RerankCandidate, RerankRequest,
};

// ── Session helper ────────────────────────────────────────────────────────────

/// Open a Zenoh session with the settings from `config`.
pub async fn open_session(config: &ZenohConfig) -> Result<zenoh::Session, RagError> {
    let zconf = config.to_zenoh_config()?;
    zenoh::open(zconf)
        .await
        .map_err(|e| RagError::Other(anyhow!("zenoh open: {e}")))
}

// ── ZenohEmbedder ─────────────────────────────────────────────────────────────

/// [`Embedder`] that delegates to a remote extension worker via Zenoh.
pub struct ZenohEmbedder {
    caller: ZenohCaller,
    registry: Arc<ExtensionRegistry>,
    dimension: usize,
}

impl ZenohEmbedder {
    pub fn new(caller: ZenohCaller, registry: Arc<ExtensionRegistry>, dimension: usize) -> Self {
        Self { caller, registry, dimension }
    }
}

#[async_trait]
impl Embedder for ZenohEmbedder {
    fn name(&self) -> &str {
        "zenoh-embedder"
    }

    fn dimension(&self) -> usize {
        self.dimension
    }

    async fn embed_texts(&self, texts: &[&str]) -> Result<Vec<Vec<f32>>, RagError> {
        let worker_id = self
            .registry
            .find_embedder()
            .ok_or_else(|| RagError::Embedder("no embedder worker registered on Zenoh bus".into()))?;

        let response = self
            .caller
            .embed_texts(
                &worker_id,
                EmbedTextsRequest { texts: texts.iter().map(|s| s.to_string()).collect() },
            )
            .await?;

        Ok(response.embeddings)
    }
}

// ── ZenohReranker ─────────────────────────────────────────────────────────────

/// [`Reranker`] that delegates to a remote extension worker via Zenoh.
pub struct ZenohReranker {
    caller: ZenohCaller,
    registry: Arc<ExtensionRegistry>,
}

impl ZenohReranker {
    pub fn new(caller: ZenohCaller, registry: Arc<ExtensionRegistry>) -> Self {
        Self { caller, registry }
    }
}

#[async_trait]
impl Reranker for ZenohReranker {
    async fn rerank(
        &self,
        query: &str,
        chunks: Vec<ScoredChunk>,
    ) -> Result<Vec<ScoredChunk>, RagError> {
        let worker_id = self
            .registry
            .find_reranker()
            .ok_or_else(|| RagError::Other(anyhow!("no reranker worker registered on Zenoh bus")))?;

        let candidates: Vec<RerankCandidate> = chunks
            .iter()
            .map(|c| RerankCandidate {
                chunk_id: c.chunk.id.0.clone(),
                text: c.chunk.text.clone(),
            })
            .collect();

        let response = self
            .caller
            .rerank(&worker_id, RerankRequest { query: query.to_string(), candidates })
            .await?;

        let mut scored: Vec<ScoredChunk> = response
            .ranked
            .iter()
            .filter_map(|ranked| {
                chunks
                    .iter()
                    .find(|c| c.chunk.id.0 == ranked.chunk_id)
                    .map(|c| ScoredChunk { chunk: c.chunk.clone(), score: ranked.score })
            })
            .collect();

        scored.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        Ok(scored)
    }
}

// ── ZenohDocumentLoader ───────────────────────────────────────────────────────

/// Routes binary file bytes to a registered document-parser extension worker.
pub struct ZenohDocumentLoader {
    caller: ZenohCaller,
    registry: Arc<ExtensionRegistry>,
}

impl ZenohDocumentLoader {
    pub fn new(caller: ZenohCaller, registry: Arc<ExtensionRegistry>) -> Self {
        Self { caller, registry }
    }

    /// Parse `data_base64` (base64-encoded raw file bytes) using a registered
    /// worker that handles `content_type`.  Returns the raw extension-protocol
    /// response.  Use [`build_document`](Self::build_document) to convert it
    /// into a `rag_core::Document` with sections populated.
    pub async fn load(
        &self,
        content_type: &str,
        data_base64: String,
        filename: Option<String>,
    ) -> Result<LoadDocumentResponse, RagError> {
        let worker_id = self
            .registry
            .find_loader_for(content_type)
            .ok_or_else(|| {
                RagError::Connector(format!(
                    "no document loader worker registered for '{content_type}'"
                ))
            })?;

        self.caller
            .load_document(
                &worker_id,
                LoadDocumentRequest {
                    content_type: content_type.to_string(),
                    data_base64,
                    filename,
                    metadata: serde_json::Value::Object(Default::default()),
                },
            )
            .await
    }

    /// Convert a [`LoadDocumentResponse`] into a [`Document`] suitable for
    /// passing to the `Indexer`.
    ///
    /// The extension-protocol `sections[]` (each carrying `title`, `text`,
    /// `page`, and optional `layout_hints`) are mapped to
    /// `rag_core::ContentSection` values stored in `Document.sections`.  The
    /// `ParagraphChunker` will use these to preserve page numbers and section
    /// titles in `ChunkMetadata` rather than re-parsing the flat text.
    ///
    /// `layout_hints` are intentionally dropped at this boundary — they are
    /// useful for workers and archival storage but rag-core does not need
    /// bounding-box information for retrieval.
    pub fn build_document(doc_ref: &DocumentRef, response: LoadDocumentResponse) -> Document {
        let sections: Vec<ContentSection> = response
            .sections
            .into_iter()
            .map(|s| ContentSection { title: s.title, text: s.text, page: s.page })
            .collect();

        Document {
            id: doc_ref.id.clone(),
            source_id: doc_ref.source_id.clone(),
            title: doc_ref.title.clone(),
            content: response.text,
            sections,
            url: doc_ref.url.clone(),
            metadata: DocumentMetadata {
                modified_at: doc_ref.modified_at,
                page_count: response.page_count,
                file_type: doc_ref
                    .content_type
                    .as_deref()
                    .and_then(|ct| ct.split('/').last())
                    .map(String::from),
                ..Default::default()
            },
        }
    }
}
