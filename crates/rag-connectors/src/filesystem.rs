//! Filesystem source connector.
//!
//! Walks a local directory tree and treats readable text files as indexable
//! documents. Useful for testing the full indexing pipeline without a live
//! SharePoint instance, and for self-hosted document stores on mounted NAS
//! or network drives.
//!
//! Text extraction is direct UTF-8 read — no parser workers are needed for
//! `.txt` and `.md`. Binary files (`.pdf`, `.docx`, etc.) return a
//! `RagError::Connector` pointing to Phase 4.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sha2::{Digest, Sha256};
use tracing::{debug, warn};
use walkdir::WalkDir;

use rag_core::{
    RagError,
    models::{Document, DocumentId, DocumentMetadata, DocumentRef, SourceId},
    traits::{ChangeEvent, ChangeKind, Connector},
};

// ── Config ────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct FilesystemConnectorConfig {
    /// Root directory to walk recursively.
    pub root: PathBuf,
    /// File extensions to include. Case-insensitive, without the leading dot.
    /// Empty means include all extensions.
    pub include_extensions: Vec<String>,
    /// Skip files larger than this many bytes. 0 = no limit.
    pub max_file_bytes: u64,
}

// ── Connector ─────────────────────────────────────────────────────────────────

pub struct FilesystemConnector {
    source_id: SourceId,
    config: FilesystemConnectorConfig,
}

impl FilesystemConnector {
    pub fn new(source_id: SourceId, config: FilesystemConnectorConfig) -> Self {
        Self { source_id, config }
    }
}

#[async_trait]
impl Connector for FilesystemConnector {
    fn source_id(&self) -> &SourceId {
        &self.source_id
    }

    fn kind(&self) -> &str {
        "filesystem"
    }

    async fn list_documents(&self) -> Result<Vec<DocumentRef>, RagError> {
        let root = self.config.root.clone();
        let include_extensions = self.config.include_extensions.clone();
        let max_file_bytes = self.config.max_file_bytes;
        let source_id = self.source_id.clone();

        // WalkDir is blocking; run on the threadpool.
        let refs = tokio::task::spawn_blocking(move || {
            let mut out = Vec::new();

            for entry in WalkDir::new(&root).into_iter().flatten() {
                if !entry.file_type().is_file() {
                    continue;
                }

                let path = entry.path();
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_lowercase();

                if !include_extensions.is_empty()
                    && !include_extensions
                        .iter()
                        .any(|e| e.eq_ignore_ascii_case(&ext))
                {
                    debug!(path = %path.display(), ext, "skipping: extension not in include list");
                    continue;
                }

                let meta = match entry.metadata() {
                    Ok(m) => m,
                    Err(e) => {
                        warn!(path = %path.display(), err = %e, "could not read file metadata");
                        continue;
                    }
                };

                let size_bytes = meta.len();
                if max_file_bytes > 0 && size_bytes > max_file_bytes {
                    warn!(
                        path = %path.display(),
                        size_bytes,
                        max = max_file_bytes,
                        "skipping file: exceeds max_file_bytes"
                    );
                    continue;
                }

                let title = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();

                let abs_path = path.to_string_lossy().into_owned();
                let url = format!("file://{abs_path}");

                let modified_at: Option<DateTime<Utc>> = meta
                    .modified()
                    .ok()
                    .map(|t| t.into());

                out.push(DocumentRef {
                    id: stable_id_from_path(path),
                    source_id: source_id.clone(),
                    title,
                    url: Some(url),
                    modified_at,
                    content_type: Some(format!("text/{ext}")),
                    size_bytes: Some(size_bytes),
                });
            }

            out
        })
        .await
        .map_err(|e| RagError::Connector(format!("list_documents thread: {e}")))?;

        debug!(root = %self.config.root.display(), count = refs.len(), "listed documents");
        Ok(refs)
    }

    async fn load_document(&self, doc_ref: &DocumentRef) -> Result<Document, RagError> {
        let url = doc_ref
            .url
            .as_deref()
            .ok_or_else(|| RagError::Connector("document has no url".into()))?;

        let path = url.strip_prefix("file://").unwrap_or(url);

        let ext = Path::new(path)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_lowercase();

        match ext.as_str() {
            "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" => {
                return Err(RagError::Connector(format!(
                    ".{ext} files require a document parser extension worker (Phase 4 — Zenoh)"
                )));
            }
            _ => {}
        }

        let content = tokio::fs::read_to_string(path)
            .await
            .map_err(|e| RagError::Connector(format!("read file: {e}")))?;

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
        _token: Option<&str>,
    ) -> Result<(Vec<ChangeEvent>, String), RagError> {
        // The filesystem has no change-feed API; re-scan the whole tree and
        // emit every file as Modified. The Indexer's upsert_chunks will
        // overwrite unchanged chunks without duplicating them.
        let docs = self.list_documents().await?;
        let events = docs
            .into_iter()
            .map(|d| ChangeEvent {
                document_ref: d,
                kind: ChangeKind::Modified,
                occurred_at: Utc::now(),
            })
            .collect();
        Ok((events, "full-scan".to_string()))
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

fn stable_id_from_path(path: &Path) -> DocumentId {
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let hash = hasher.finalize();
    DocumentId::from_str(
        hash.iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
    )
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_connector(root: &Path, extensions: Vec<&str>) -> FilesystemConnector {
        FilesystemConnector::new(
            SourceId::from_str("fs-test"),
            FilesystemConnectorConfig {
                root: root.to_path_buf(),
                include_extensions: extensions.iter().map(|e| e.to_string()).collect(),
                max_file_bytes: 0,
            },
        )
    }

    #[tokio::test]
    async fn lists_txt_files() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("b.md"), b"world").unwrap();
        std::fs::write(dir.path().join("skip.pdf"), b"%PDF").unwrap();

        let c = make_connector(dir.path(), vec!["txt", "md"]);
        let docs = c.list_documents().await.unwrap();
        assert_eq!(docs.len(), 2);
        assert!(docs.iter().any(|d| d.title == "a.txt"));
        assert!(docs.iter().any(|d| d.title == "b.md"));
    }

    #[tokio::test]
    async fn all_extensions_when_filter_empty() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"hello").unwrap();
        std::fs::write(dir.path().join("b.pdf"), b"%PDF").unwrap();

        let c = make_connector(dir.path(), vec![]);
        let docs = c.list_documents().await.unwrap();
        assert_eq!(docs.len(), 2);
    }

    #[tokio::test]
    async fn respects_max_file_bytes() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("small.txt"), b"hi").unwrap();
        std::fs::write(dir.path().join("large.txt"), b"x".repeat(1000).as_slice()).unwrap();

        let c = FilesystemConnector::new(
            SourceId::from_str("fs-test"),
            FilesystemConnectorConfig {
                root: dir.path().to_path_buf(),
                include_extensions: vec![],
                max_file_bytes: 10,
            },
        );
        let docs = c.list_documents().await.unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].title, "small.txt");
    }

    #[tokio::test]
    async fn load_document_reads_content() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        std::fs::write(&file_path, b"hello world").unwrap();

        let c = make_connector(dir.path(), vec!["txt"]);
        let docs = c.list_documents().await.unwrap();
        assert_eq!(docs.len(), 1);

        let doc = c.load_document(&docs[0]).await.unwrap();
        assert_eq!(doc.content.trim(), "hello world");
        assert_eq!(doc.metadata.file_type.as_deref(), Some("txt"));
    }

    #[tokio::test]
    async fn load_document_rejects_binary_formats() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("report.pdf"), b"%PDF-1.4").unwrap();

        let c = make_connector(dir.path(), vec![]);
        let docs = c.list_documents().await.unwrap();
        assert_eq!(docs.len(), 1);

        let err = c.load_document(&docs[0]).await.unwrap_err();
        assert!(err.to_string().contains("extension worker"));
    }

    #[tokio::test]
    async fn stable_ids_are_consistent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();

        let c = make_connector(dir.path(), vec!["txt"]);
        let docs1 = c.list_documents().await.unwrap();
        let docs2 = c.list_documents().await.unwrap();
        assert_eq!(docs1[0].id, docs2[0].id);
    }

    #[tokio::test]
    async fn changes_since_returns_full_scan() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), b"x").unwrap();

        let c = make_connector(dir.path(), vec!["txt"]);
        let (events, token) = c.changes_since(None).await.unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(token, "full-scan");
        assert!(matches!(events[0].kind, ChangeKind::Modified));
    }
}
