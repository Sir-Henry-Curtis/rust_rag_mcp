//! Transport-neutral extension protocol for rag-core external workers.
//!
//! Version: `rag.extension.v1`
//!
//! Any transport (Zenoh, HTTP, stdio) should carry these message types verbatim.
//! Phase 4 implements the Zenoh transport in `rag-zenoh`.
//!
//! ## Zenoh keyspace convention
//! ```text
//! rag/extensions/{extension_id}/announce    — CapabilityDescriptor on startup
//! rag/extensions/{extension_id}/heartbeat   — Heartbeat every N seconds
//! rag/call/{extension_id}/load              — load_document request/reply
//! rag/call/{extension_id}/embed             — embed_texts request/reply
//! rag/call/{extension_id}/rerank            — rerank request/reply
//! rag/call/{extension_id}/transform         — transform request/reply
//! rag/call/{extension_id}/filter            — apply_acl request/reply
//! rag/events/index_started
//! rag/events/index_progress
//! rag/events/document_indexed
//! rag/events/index_failed
//! rag/events/sync_started
//! rag/events/sync_completed
//! ```

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

pub const PROTOCOL_VERSION: &str = "rag.extension.v1";

// ── Capability descriptor ─────────────────────────────────────────────────────

/// Announced by an extension worker on startup and periodically as a heartbeat.
///
/// Example (JSON):
/// ```json
/// {
///   "extension_id": "python.pdf_loader",
///   "protocol_version": "rag.extension.v1",
///   "capabilities": ["load_document"],
///   "content_types": ["application/pdf"],
///   "max_payload_bytes": 104857600,
///   "supports_streaming": true
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityDescriptor {
    pub extension_id: String,
    pub protocol_version: String,
    pub capabilities: Vec<ExtensionCapability>,
    /// MIME types this worker can handle, e.g. `["application/pdf"]`.
    pub content_types: Vec<String>,
    /// Maximum raw document payload this worker accepts in bytes.
    pub max_payload_bytes: u64,
    pub supports_streaming: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtensionCapability {
    LoadDocument,
    EmbedTexts,
    Rerank,
    ApplyAcl,
    SummarizeContext,
    Transform,
}

// ── Request / response envelopes ──────────────────────────────────────────────

/// Wraps every request from the Rust runtime to an extension worker.
///
/// Example (JSON):
/// ```json
/// {
///   "protocol": "rag.extension.v1",
///   "request_id": "req_abc123",
///   "operation": "load_document",
///   "payload": { "content_type": "application/pdf", "data_base64": "..." },
///   "context": { "tenant_id": "default", "user_id": "user_1", "trace_id": "t_1" }
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestEnvelope {
    pub protocol: String,
    pub request_id: String,
    pub operation: ExtensionCapability,
    pub payload: serde_json::Value,
    pub context: RequestContext,
}

impl RequestEnvelope {
    pub fn new(
        operation: ExtensionCapability,
        payload: serde_json::Value,
        context: RequestContext,
    ) -> Self {
        Self {
            protocol: PROTOCOL_VERSION.to_string(),
            request_id: Uuid::new_v4().to_string(),
            operation,
            payload,
            context,
        }
    }
}

/// Wraps every response from an extension worker back to the Rust runtime.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseEnvelope {
    pub request_id: String,
    pub success: bool,
    pub payload: serde_json::Value,
    pub error: Option<String>,
}

impl ResponseEnvelope {
    pub fn ok(request_id: impl Into<String>, payload: serde_json::Value) -> Self {
        Self {
            request_id: request_id.into(),
            success: true,
            payload,
            error: None,
        }
    }

    pub fn err(request_id: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            request_id: request_id.into(),
            success: false,
            payload: serde_json::Value::Null,
            error: Some(message.into()),
        }
    }
}

// ── Caller context ────────────────────────────────────────────────────────────

/// Per-request caller context forwarded to extensions for audit and ACL use.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RequestContext {
    pub tenant_id: Option<String>,
    pub user_id: Option<String>,
    pub trace_id: Option<String>,
}

// ── Heartbeat ─────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Heartbeat {
    pub extension_id: String,
    pub timestamp: DateTime<Utc>,
    pub status: WorkerStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerStatus {
    Ready,
    Busy,
    Draining,
}

// ── Typed payloads ────────────────────────────────────────────────────────────

/// Payload for `load_document` requests.
///
/// The Rust runtime sends raw file bytes (base64-encoded) to the extension,
/// which returns extracted text, sections, and any metadata it can infer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadDocumentRequest {
    pub content_type: String,
    /// Base64-encoded raw file bytes.
    pub data_base64: String,
    pub filename: Option<String>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoadDocumentResponse {
    pub text: String,
    pub sections: Vec<DocumentSection>,
    pub page_count: Option<u32>,
    pub metadata: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DocumentSection {
    pub title: Option<String>,
    pub text: String,
    pub page: Option<u32>,
    /// Optional layout metadata for layout-aware parsing (PDF tables, figures, etc.).
    /// Workers that perform visual layout analysis (RAGFlow-style) populate this.
    /// Workers that do plain text extraction leave it `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout_hints: Option<LayoutHints>,
}

/// Layout metadata from a document-parser extension worker.
///
/// Populated by workers that perform visual layout analysis (e.g. PDF bounding-box
/// extraction). Callers that only need plain text can ignore this entirely.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutHints {
    /// Visual block type inferred by the parser.
    pub block_type: Option<LayoutBlockType>,
    /// Zero-based column index on the page (0 = left column in a multi-column layout).
    pub column: Option<u32>,
    /// Bounding box as `[x0, y0, x1, y1]` in page-coordinate units (PDF points
    /// or pixel coordinates depending on the parser).
    pub bbox: Option<[f32; 4]>,
}

/// The visual type of a layout block returned by an extension worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LayoutBlockType {
    Paragraph,
    Heading,
    Table,
    Figure,
    Caption,
    Header,
    Footer,
    ListItem,
    CodeBlock,
    Other,
}

/// Payload for `embed_texts` requests (batch).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedTextsRequest {
    pub texts: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmbedTextsResponse {
    pub embeddings: Vec<Vec<f32>>,
    pub dimension: usize,
}

// ── Rerank payloads ───────────────────────────────────────────────────────────

/// Payload for `rerank` requests.
///
/// The caller sends the original query and a set of candidate chunks. The
/// worker returns those chunk IDs sorted by relevance with a new score.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankRequest {
    pub query: String,
    pub candidates: Vec<RerankCandidate>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankCandidate {
    pub chunk_id: String,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RerankResponse {
    pub ranked: Vec<RankedChunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RankedChunk {
    pub chunk_id: String,
    /// Normalised relevance score in [0, 1].
    pub score: f32,
}
