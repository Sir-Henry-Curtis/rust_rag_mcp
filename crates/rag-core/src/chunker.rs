use async_trait::async_trait;
use tracing::debug;

use crate::error::RagError;
use crate::models::{Chunk, ChunkId, ChunkMetadata, Document};
use crate::traits::Chunker;

/// Splits documents on paragraph boundaries (`\n\n`), collecting paragraphs
/// into chunks up to `max_chars`.
///
/// ## Markdown heading awareness
///
/// Lines matching ATX heading syntax (`# …` through `###### …`) are detected
/// and tracked as the current section label.  When a heading is encountered:
///
/// 1. Any buffered text from the *previous* section is flushed as its own chunk.
/// 2. `current_section` is updated to the heading text.
/// 3. The heading line itself is **not** added to the chunk buffer — a bare
///    heading without body text is not retrievable on its own.
///
/// All chunks produced after a heading carry that heading in
/// `ChunkMetadata.section`, enabling section-level filtering and
/// citation labels like "Executive Summary — p. 3".
///
/// ## Overlap
///
/// The last `overlap_chars` of each *size-boundary* flush are prepended to the
/// next chunk to preserve context.  Heading-boundary flushes do **not** carry
/// overlap because the overlap would come from the previous section.
pub struct ParagraphChunker {
    pub max_chars: usize,
    pub overlap_chars: usize,
}

impl Default for ParagraphChunker {
    fn default() -> Self {
        Self {
            max_chars: 1500,
            overlap_chars: 200,
        }
    }
}

#[async_trait]
impl Chunker for ParagraphChunker {
    fn name(&self) -> &str {
        "paragraph"
    }

    async fn chunk(&self, document: &Document) -> Result<Vec<Chunk>, RagError> {
        let paragraphs: Vec<&str> = document
            .content
            .split("\n\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut current = String::new();
        let mut chunk_index: u32 = 0;
        let mut current_section: Option<String> = None;

        for para in &paragraphs {
            // ── Heading detection ─────────────────────────────────────────────
            if let Some(heading_text) = extract_heading(para) {
                // Flush buffered content from the previous section without
                // carrying overlap — overlap across section boundaries would
                // mix section context and confuse retrieval.
                if !current.trim().is_empty() {
                    chunks.push(make_chunk(
                        document,
                        &current,
                        chunk_index,
                        current_section.clone(),
                    ));
                    chunk_index += 1;
                    current.clear();
                }
                current_section = Some(heading_text);
                continue; // bare headings are not added to the buffer
            }

            // ── Size-boundary flush ───────────────────────────────────────────
            if !current.is_empty() && current.len() + para.len() > self.max_chars {
                let overlap: String = current
                    .chars()
                    .rev()
                    .take(self.overlap_chars)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();

                chunks.push(make_chunk(
                    document,
                    &current,
                    chunk_index,
                    current_section.clone(),
                ));
                chunk_index += 1;
                current = overlap;
            }

            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
        }

        // ── Final flush ───────────────────────────────────────────────────────
        if !current.trim().is_empty() {
            chunks.push(make_chunk(document, &current, chunk_index, current_section));
        }

        debug!(
            document_id = %document.id,
            chunk_count = chunks.len(),
            "chunked document"
        );

        Ok(chunks)
    }
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Detect a Markdown ATX heading (`# text` … `###### text`).
/// Returns the heading text without the `#` prefix, or `None`.
fn extract_heading(para: &str) -> Option<String> {
    let first_line = para.lines().next()?.trim();
    if !first_line.starts_with('#') {
        return None;
    }
    let hash_count = first_line.chars().take_while(|&c| c == '#').count();
    if hash_count > 6 {
        return None; // More than 6 '#' is not a valid ATX heading.
    }
    let after = &first_line[hash_count..];
    // ATX heading syntax requires at least one space after the hashes.
    if after.starts_with(' ') {
        let text = after.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

fn make_chunk(
    document: &Document,
    text: &str,
    chunk_index: u32,
    section: Option<String>,
) -> Chunk {
    Chunk {
        id: ChunkId::new(),
        document_id: document.id.clone(),
        source_id: document.source_id.clone(),
        text: text.trim().to_string(),
        chunk_index,
        embedding: None,
        metadata: ChunkMetadata {
            document_title: document.title.clone(),
            document_url: document.url.clone(),
            modified_at: document.metadata.modified_at,
            permissions: document.metadata.permissions.clone(),
            section,
            ..Default::default()
        },
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{DocumentId, DocumentMetadata, SourceId};

    fn doc(content: &str) -> Document {
        Document {
            id: DocumentId::new(),
            source_id: SourceId::from_str("test"),
            title: "Test Doc".into(),
            content: content.into(),
            url: None,
            metadata: DocumentMetadata::default(),
        }
    }

    #[tokio::test]
    async fn plain_paragraphs_have_no_section() {
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc("Hello world.\n\nSecond paragraph.")).await.unwrap();
        assert!(!chunks.is_empty());
        assert!(chunks[0].metadata.section.is_none());
    }

    #[tokio::test]
    async fn heading_sets_section_on_following_chunks() {
        let content = "## Executive Summary\n\nThis quarter was strong.\n\nRevenue grew 15%.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        // Both body paragraphs should carry the section label.
        assert!(!chunks.is_empty());
        for chunk in &chunks {
            assert_eq!(
                chunk.metadata.section.as_deref(),
                Some("Executive Summary"),
                "chunk text: {}",
                chunk.text
            );
        }
    }

    #[tokio::test]
    async fn bare_heading_does_not_become_a_chunk() {
        let content = "## Introduction\n\nSome body text here.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        // Only one chunk (the body text); the heading itself is not chunked.
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata.section.as_deref(), Some("Introduction"));
        assert!(!chunks[0].text.contains("## Introduction"));
    }

    #[tokio::test]
    async fn heading_flushes_previous_section_without_overlap() {
        let content = "## Section A\n\nContent for A.\n\n## Section B\n\nContent for B.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        // Each section becomes its own chunk with the correct label.
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].metadata.section.as_deref(), Some("Section A"));
        assert!(chunks[0].text.contains("Content for A"));
        assert_eq!(chunks[1].metadata.section.as_deref(), Some("Section B"));
        assert!(chunks[1].text.contains("Content for B"));
        // Section B chunk must not contain any text from Section A.
        assert!(!chunks[1].text.contains("Content for A"));
    }

    #[tokio::test]
    async fn multiple_heading_levels_all_detected() {
        for prefix in ["# H1", "## H2", "### H3", "###### H6"] {
            let heading_text = prefix.trim_start_matches('#').trim();
            let content = format!("{prefix}\n\nBody text.");
            let c = ParagraphChunker::default();
            let chunks = c.chunk(&doc(&content)).await.unwrap();
            assert_eq!(
                chunks[0].metadata.section.as_deref(),
                Some(heading_text),
                "failed for prefix: {prefix}"
            );
        }
    }

    #[tokio::test]
    async fn seven_hashes_not_a_heading() {
        // ####### is not a valid Markdown ATX heading (max 6).
        let content = "####### Not a heading\n\nBody text.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        assert!(chunks[0].metadata.section.is_none());
    }

    #[tokio::test]
    async fn extract_heading_helper() {
        assert_eq!(extract_heading("# Title"), Some("Title".into()));
        assert_eq!(extract_heading("## Sub"), Some("Sub".into()));
        assert_eq!(extract_heading("#NoSpace"), None); // no space after #
        assert_eq!(extract_heading("####### Too many"), None);
        assert_eq!(extract_heading("Normal paragraph"), None);
        assert_eq!(extract_heading("# "), None); // empty heading text
    }
}
