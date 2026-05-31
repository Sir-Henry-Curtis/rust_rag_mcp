//! SharePoint document library connector (Phase 3 implementation).
//!
//! Bridges `sharepoint_rest_api` (the Rust SharePoint REST/MCP library at
//! `C:\Users\jlbri\Desktop\sharepoint_rest_api-rs`) to the `rag_core::Connector`
//! trait.
//!
//! Phase 3 work:
//!   - Add sharepoint_rest_api as a path dependency in Cargo.toml.
//!   - Implement `list_documents` using `sp_get_folder_files_recursive`.
//!   - Implement `load_document` using `sp_get_file_content` (base64) + text extraction.
//!   - Implement `changes_since` using `sp_get_list_changes` + change tokens.
//!   - Attach title, URL, author, modified time, version, and permission hints.
//!   - Forward `sp_get_user_effective_permissions` results as `permissions` hints.
//!   - Supported formats: .txt, .md routed directly; .pdf/.docx/.xlsx routed to
//!     an extension worker (rag-zenoh) for text extraction.

use async_trait::async_trait;
use chrono::Utc;

use rag_core::{
    ChangeEvent, Connector, RagError,
    models::{Document, DocumentId, DocumentMetadata, DocumentRef, SourceId},
};

/// Configuration for a SharePoint library connector instance.
#[derive(Debug, Clone)]
pub struct SharePointConnectorConfig {
    /// Full SharePoint site URL, e.g. `https://tenant.sharepoint.com/sites/mysite`.
    pub site_url: String,
    /// Server-relative path to the library or folder to index,
    /// e.g. `/sites/mysite/Shared Documents`.
    pub library_path: String,
    /// File extensions to include (empty = all). Case-insensitive, no leading dot.
    /// Example: `["pdf", "docx", "xlsx", "txt", "md"]`
    pub include_extensions: Vec<String>,
    /// Skip files larger than this many bytes (0 = no limit).
    pub max_file_bytes: u64,
}

/// Connector that indexes a SharePoint document library via the SharePoint REST API.
pub struct SharePointConnector {
    source_id: SourceId,
    config: SharePointConnectorConfig,
}

impl SharePointConnector {
    pub fn new(source_id: SourceId, config: SharePointConnectorConfig) -> Self {
        Self { source_id, config }
    }
}

#[async_trait]
impl Connector for SharePointConnector {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    fn kind(&self) -> &str {
        "sharepoint"
    }

    async fn list_documents(&self) -> Result<Vec<DocumentRef>, RagError> {
        // Phase 3: call SharePointClient::get_folder_files_recursive(library_path)
        // and map each SpFile into a DocumentRef.
        Err(RagError::Connector(
            "SharePoint connector not yet implemented (Phase 3)".into(),
        ))
    }

    async fn load_document(&self, _doc_ref: &DocumentRef) -> Result<Document, RagError> {
        // Phase 3: call SharePointClient::get_file_content(url) to get base64 bytes,
        // then route to the appropriate loader:
        //   - .txt / .md  → UTF-8 decode directly
        //   - .pdf        → rag-zenoh extension worker (python.pdf_loader)
        //   - .docx       → rag-zenoh extension worker (python.docx_loader)
        //   - .xlsx       → rag-zenoh extension worker (python.xlsx_loader)
        Err(RagError::Connector(
            "SharePoint connector not yet implemented (Phase 3)".into(),
        ))
    }

    async fn changes_since(
        &self,
        _token: Option<&str>,
    ) -> Result<(Vec<ChangeEvent>, String), RagError> {
        // Phase 3: call SharePointClient::get_list_changes(token) and map
        // each change record into a ChangeEvent with Created/Modified/Deleted kind.
        Err(RagError::Connector(
            "SharePoint connector not yet implemented (Phase 3)".into(),
        ))
    }
}
