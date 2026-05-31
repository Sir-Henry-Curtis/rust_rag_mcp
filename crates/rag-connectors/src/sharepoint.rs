//! SharePoint document library connector.
//!
//! Bridges `sharepoint_mcp::SharePointClient` to the `rag_core::Connector` trait.
//!
//! ## Text extraction routing
//!
//! | Extension          | Handled by                          |
//! |--------------------|-------------------------------------|
//! | `.txt`, `.md`      | UTF-8 decode in this crate          |
//! | `.pdf`, `.docx`, `.xlsx`, `.pptx`, `.doc`, `.xls`, `.ppt` | Phase 4 extension worker via Zenoh |
//! | anything else      | UTF-8 decode attempted; error logged on invalid UTF-8 |
//!
//! ## Stable document IDs
//!
//! `DocumentId` is `sha256(site_url + "::" + server_relative_url)` encoded as
//! lowercase hex. Re-indexing the same file always produces the same ID, so
//! `upsert_chunks` in the store overwrites stale chunks rather than duplicating.

use std::sync::Arc;

use async_trait::async_trait;
use base64::{engine::general_purpose::STANDARD as B64, Engine as _};
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};

use rag_core::{
    RagError,
    models::{Document, DocumentId, DocumentMetadata, DocumentRef, SourceId},
    traits::{ChangeEvent, ChangeKind, Connector},
};
use sharepoint_mcp::{SharePointClient, client::changes::ChangeQuery};

// ── Config ────────────────────────────────────────────────────────────────────

/// Configuration for a SharePoint document library connector instance.
#[derive(Debug, Clone)]
pub struct SharePointConnectorConfig {
    /// Server-relative URL of the library or folder to index.
    /// Example: `/sites/mysite/Shared Documents`
    pub library_path: String,
    /// Display title of the SharePoint list/library, used for change tracking.
    /// Example: `Shared Documents`
    pub list_title: String,
    /// File extensions to include. Case-insensitive, without the leading dot.
    /// Empty means include all extensions.
    /// Example: `vec!["pdf", "docx", "txt", "md"]`
    pub include_extensions: Vec<String>,
    /// Skip files whose `Length` exceeds this many bytes. 0 = no limit.
    pub max_file_bytes: u64,
}

// ── Connector ─────────────────────────────────────────────────────────────────

/// Connector that indexes a SharePoint document library via the SharePoint REST API.
///
/// Accepts a pre-built [`SharePointClient`] so callers control authentication,
/// TLS settings, retry limits, and concurrency independently of the RAG layer.
///
/// ```no_run
/// use sharepoint_mcp::{SharePointClient, Config, AuthMode};
/// use rag_connectors::sharepoint::{SharePointConnector, SharePointConnectorConfig};
/// use rag_core::models::SourceId;
///
/// let sp_config = Config {
///     site_url: "https://tenant.sharepoint.com/sites/mysite".into(),
///     auth: AuthMode::AzureAd {
///         tenant_id: "…".into(), client_id: "…".into(),
///         client_secret: "…".into(), authority: None, scope: None,
///     },
///     ..Default::default()
/// };
/// let client = SharePointClient::new(sp_config).unwrap();
/// let connector = SharePointConnector::new(
///     SourceId::from_str("sp-finance"),
///     Arc::new(client),
///     SharePointConnectorConfig {
///         library_path: "/sites/mysite/Shared Documents".into(),
///         list_title: "Shared Documents".into(),
///         include_extensions: vec!["pdf".into(), "docx".into(), "txt".into()],
///         max_file_bytes: 50 * 1024 * 1024,
///     },
/// );
/// ```
pub struct SharePointConnector {
    source_id: SourceId,
    client: Arc<SharePointClient>,
    config: SharePointConnectorConfig,
}

impl SharePointConnector {
    pub fn new(
        source_id: SourceId,
        client: Arc<SharePointClient>,
        config: SharePointConnectorConfig,
    ) -> Self {
        Self { source_id, client, config }
    }
}

// ── Connector trait ───────────────────────────────────────────────────────────

#[async_trait]
impl Connector for SharePointConnector {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    fn kind(&self) -> &str {
        "sharepoint"
    }

    async fn list_documents(&self) -> Result<Vec<DocumentRef>, RagError> {
        let files = self
            .client
            .get_folder_files_recursive(&self.config.library_path)
            .await
            .map_err(|e| RagError::Connector(format!("list_documents: {e}")))?;

        let mut refs = Vec::with_capacity(files.len());

        for file in &files {
            let name = match file["Name"].as_str() {
                Some(n) => n,
                None => {
                    warn!("SharePoint file missing Name field; skipping");
                    continue;
                }
            };
            let server_relative_url = match file["ServerRelativeUrl"].as_str() {
                Some(u) => u,
                None => {
                    warn!(name, "SharePoint file missing ServerRelativeUrl; skipping");
                    continue;
                }
            };

            let ext = file_extension(server_relative_url).to_lowercase();

            // Extension filter
            if !self.config.include_extensions.is_empty()
                && !self
                    .config
                    .include_extensions
                    .iter()
                    .any(|e| e.eq_ignore_ascii_case(&ext))
            {
                debug!(name, ext, "skipping: extension not in include list");
                continue;
            }

            // Size filter
            let size_bytes: u64 = file["Length"]
                .as_str()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0);

            if self.config.max_file_bytes > 0 && size_bytes > self.config.max_file_bytes {
                warn!(
                    name,
                    size_bytes,
                    max = self.config.max_file_bytes,
                    "skipping file: exceeds max_file_bytes"
                );
                continue;
            }

            let modified_at = file["TimeLastModified"]
                .as_str()
                .and_then(parse_sp_datetime);

            let full_url = format!(
                "{}{}",
                self.client.config.site_url,
                server_relative_url
            );

            refs.push(DocumentRef {
                id: stable_id(&self.client.config.site_url, server_relative_url),
                source_id: self.source_id.clone(),
                title: name.to_string(),
                url: Some(full_url),
                modified_at,
                content_type: Some(content_type_for_ext(&ext)),
                size_bytes: Some(size_bytes),
            });
        }

        debug!(
            library = %self.config.library_path,
            count = refs.len(),
            "listed documents"
        );
        Ok(refs)
    }

    async fn load_document(&self, doc_ref: &DocumentRef) -> Result<Document, RagError> {
        let full_url = doc_ref
            .url
            .as_deref()
            .ok_or_else(|| RagError::Connector("document has no url".into()))?;

        // Strip site URL prefix to get the server-relative URL the client expects.
        let server_relative_url = full_url
            .strip_prefix(&self.client.config.site_url)
            .unwrap_or(full_url);

        let ext = file_extension(server_relative_url).to_lowercase();

        // Binary formats require a Phase 4 extension worker.
        match ext.as_str() {
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => {
                return Err(RagError::Connector(format!(
                    ".{ext} files require a document parser extension worker (Phase 4 — Zenoh). \
                     Register a Python or Rust loader via rag-zenoh."
                )));
            }
            _ => {}
        }

        let b64 = self
            .client
            .get_file_content(server_relative_url)
            .await
            .map_err(|e| RagError::Connector(format!("get_file_content: {e}")))?;

        let bytes = B64
            .decode(b64.trim())
            .map_err(|e| RagError::Connector(format!("base64 decode: {e}")))?;

        let content = String::from_utf8_lossy(&bytes).into_owned();

        Ok(Document {
            id: doc_ref.id.clone(),
            source_id: doc_ref.source_id.clone(),
            title: doc_ref.title.clone(),
            content,
            url: doc_ref.url.clone(),
            metadata: DocumentMetadata {
                modified_at: doc_ref.modified_at,
                file_type: Some(ext),
                ..Default::default()
            },
        })
    }

    async fn changes_since(
        &self,
        token: Option<&str>,
    ) -> Result<(Vec<ChangeEvent>, String), RagError> {
        let query = ChangeQuery {
            change_token_start: token,
            file: true,
            add: true,
            update: true,
            delete_object: true,
            rename: true,
            recursive_all: true,
            ..Default::default()
        };

        let changes = self
            .client
            .get_list_changes(&self.config.list_title, &query)
            .await
            .map_err(|e| RagError::Connector(format!("get_list_changes: {e}")))?;

        // The new sync token is the ChangeToken from the last change in the batch.
        let new_token = changes
            .last()
            .and_then(|c| c["ChangeToken"]["StringValue"].as_str())
            .map(String::from)
            .unwrap_or_default();

        let mut events = Vec::new();

        for change in &changes {
            let change_type = match change["ChangeType"].as_u64() {
                Some(t) => t,
                None => continue,
            };

            // SharePoint change types: 1=Add 2=Update 3=DeleteObject 4=Rename 5=Move
            let kind = match change_type {
                1 => ChangeKind::Created,
                2 | 4 | 5 => ChangeKind::Modified,
                3 => ChangeKind::Deleted,
                _ => continue,
            };

            // File changes carry expanded file metadata; deletions may not.
            let server_relative_url = change["File"]["ServerRelativeUrl"].as_str();
            let name = change["File"]["Name"].as_str();
            let modified = change["File"]["TimeLastModified"].as_str();

            // Apply extension filter for non-delete changes.
            if !matches!(kind, ChangeKind::Deleted) {
                if let Some(url) = server_relative_url {
                    let ext = file_extension(url).to_lowercase();
                    if !self.config.include_extensions.is_empty()
                        && !self
                            .config
                            .include_extensions
                            .iter()
                            .any(|e| e.eq_ignore_ascii_case(&ext))
                    {
                        continue;
                    }
                }
            }

            let (doc_id, url_opt) = match server_relative_url {
                Some(rel) => (
                    stable_id(&self.client.config.site_url, rel),
                    Some(format!("{}{}", self.client.config.site_url, rel)),
                ),
                None => {
                    // Deletion without a file URL — fall back to item ID.
                    let item_id = change["ItemId"].as_u64().unwrap_or(0);
                    (
                        DocumentId::from_str(format!(
                            "sp-item-{}-{}",
                            self.source_id, item_id
                        )),
                        None,
                    )
                }
            };

            events.push(ChangeEvent {
                document_ref: rag_core::models::DocumentRef {
                    id: doc_id,
                    source_id: self.source_id.clone(),
                    title: name.unwrap_or("").to_string(),
                    url: url_opt,
                    modified_at: modified.and_then(parse_sp_datetime),
                    content_type: server_relative_url
                        .map(|u| content_type_for_ext(&file_extension(u).to_lowercase())),
                    size_bytes: None,
                },
                kind,
                occurred_at: Utc::now(),
            });
        }

        debug!(
            list = %self.config.list_title,
            event_count = events.len(),
            "changes_since complete"
        );
        Ok((events, new_token))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Stable, deterministic `DocumentId` from site URL + server-relative file URL.
pub fn stable_id(site_url: &str, server_relative_url: &str) -> DocumentId {
    let mut hasher = Sha256::new();
    hasher.update(site_url.as_bytes());
    hasher.update(b"::");
    hasher.update(server_relative_url.as_bytes());
    let hash = hasher.finalize();
    DocumentId::from_str(
        hash.iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    )
}

fn file_extension(path: &str) -> &str {
    path.rsplit('.').next().unwrap_or("")
}

fn content_type_for_ext(ext: &str) -> String {
    match ext {
        "pdf"  => "application/pdf".into(),
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document".into(),
        "xlsx" => "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet".into(),
        "pptx" => "application/vnd.openxmlformats-officedocument.presentationml.presentation".into(),
        "doc"  => "application/msword".into(),
        "xls"  => "application/vnd.ms-excel".into(),
        "ppt"  => "application/vnd.ms-powerpoint".into(),
        "txt"  => "text/plain".into(),
        "md"   => "text/markdown".into(),
        "csv"  => "text/csv".into(),
        "json" => "application/json".into(),
        _      => format!("application/{ext}"),
    }
}

fn parse_sp_datetime(s: &str) -> Option<DateTime<Utc>> {
    // SharePoint returns ISO 8601 strings like "2024-03-15T10:30:00Z"
    DateTime::parse_from_rfc3339(s)
        .ok()
        .map(|dt| dt.with_timezone(&Utc))
}

// ── Unit tests (no SharePoint server required) ────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_id_is_deterministic() {
        let a = stable_id(
            "https://tenant.sharepoint.com/sites/mysite",
            "/sites/mysite/Shared Documents/report.pdf",
        );
        let b = stable_id(
            "https://tenant.sharepoint.com/sites/mysite",
            "/sites/mysite/Shared Documents/report.pdf",
        );
        assert_eq!(a, b);
    }

    #[test]
    fn stable_id_differs_by_path() {
        let a = stable_id("https://sp.example.com/sites/a", "/sites/a/Docs/file.txt");
        let b = stable_id("https://sp.example.com/sites/a", "/sites/a/Docs/other.txt");
        assert_ne!(a, b);
    }

    #[test]
    fn stable_id_differs_by_site() {
        let a = stable_id("https://sp.example.com/sites/a", "/path/file.txt");
        let b = stable_id("https://sp.example.com/sites/b", "/path/file.txt");
        assert_ne!(a, b);
    }

    #[test]
    fn stable_id_is_64_hex_chars() {
        let id = stable_id("https://sp.example.com", "/docs/file.txt");
        assert_eq!(id.0.len(), 64);
        assert!(id.0.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn file_extension_extracts_last_segment() {
        assert_eq!(file_extension("report.v2.pdf"), "pdf");
        assert_eq!(file_extension("README.md"), "md");
        assert_eq!(file_extension("no_extension"), "no_extension");
    }

    #[test]
    fn content_type_for_known_extensions() {
        assert_eq!(content_type_for_ext("pdf"), "application/pdf");
        assert_eq!(content_type_for_ext("txt"), "text/plain");
        assert_eq!(content_type_for_ext("md"), "text/markdown");
    }

    #[test]
    fn parse_sp_datetime_valid() {
        let dt = parse_sp_datetime("2024-03-15T10:30:00Z");
        assert!(dt.is_some());
    }

    #[test]
    fn parse_sp_datetime_invalid() {
        assert!(parse_sp_datetime("not-a-date").is_none());
    }
}
