use async_trait::async_trait;
use tracing::debug;

use crate::error::RagError;
use crate::models::{Chunk, ChunkId, ChunkMetadata, ContentSection, Document};
use crate::traits::Chunker;

/// Splits documents into chunks ready for embedding.
///
/// ## Section-aware mode (extension-worker documents)
///
/// When `document.sections` is non-empty (populated by a PDF loader, DOCX
/// loader, or other extension worker), each `ContentSection` becomes the unit
/// of chunking:
///
/// - The section `title` and `page` are written directly into
///   `ChunkMetadata.section` and `ChunkMetadata.page`.
/// - If a section's text exceeds `max_chars` it is further split on paragraph
///   (`\n\n`) boundaries, with the last `overlap_chars` carried into the next
///   sub-chunk for context continuity.
/// - **No overlap crosses section boundaries** — the section break is a hard
///   boundary, mirroring the no-overlap-across-headings rule below.
///
/// ## Heading-aware mode (plain-text / Markdown documents)
///
/// When `document.sections` is empty the chunker falls back to heading-aware
/// paragraph splitting of `document.content`:
///
/// - ATX headings (`# …` through `###### …`) flush the current buffer and
///   update the running `current_section` label.
/// - Bare heading lines are **not** added to the buffer (they carry no
///   retrievable content on their own).
/// - No overlap is carried across a heading boundary; overlap is only carried
///   at size-limit boundaries within the same section.
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
        let chunks = if document.sections.is_empty() {
            self.chunk_from_content(document)
        } else {
            self.chunk_from_sections(document)
        };

        debug!(
            document_id = %document.id,
            mode = if document.sections.is_empty() { "content" } else { "sections" },
            chunk_count = chunks.len(),
            "chunked document"
        );

        Ok(chunks)
    }
}

impl ParagraphChunker {
    // ── Section-based path ────────────────────────────────────────────────────

    fn chunk_from_sections(&self, document: &Document) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let mut chunk_index: u32 = 0;

        for section in &document.sections {
            let paragraphs: Vec<&str> = section
                .text
                .split("\n\n")
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .collect();

            let mut current = String::new();

            for para in &paragraphs {
                // Size-boundary flush within the section (overlap kept).
                if !current.is_empty() && current.len() + para.len() > self.max_chars {
                    let overlap = tail_chars(&current, self.overlap_chars);
                    chunks.push(make_chunk(
                        document,
                        &current,
                        chunk_index,
                        section.title.clone(),
                        section.page,
                    ));
                    chunk_index += 1;
                    current = overlap;
                }
                if !current.is_empty() {
                    current.push_str("\n\n");
                }
                current.push_str(para);
            }

            // Flush the last sub-chunk for this section.
            if !current.trim().is_empty() {
                chunks.push(make_chunk(
                    document,
                    &current,
                    chunk_index,
                    section.title.clone(),
                    section.page,
                ));
                chunk_index += 1;
            }
            // `current` is intentionally dropped here — no overlap across sections.
        }

        chunks
    }

    // ── Content-based (heading-aware) path ───────────────────────────────────

    fn chunk_from_content(&self, document: &Document) -> Vec<Chunk> {
        let paragraphs: Vec<&str> = document
            .content
            .split("\n\n")
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();

        let mut chunks = Vec::new();
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
                        None,
                    ));
                    chunk_index += 1;
                    current.clear();
                }
                current_section = Some(heading_text);
                continue; // bare headings are not added to the buffer
            }

            // ── Size-boundary flush ───────────────────────────────────────────
            if !current.is_empty() && current.len() + para.len() > self.max_chars {
                let overlap = tail_chars(&current, self.overlap_chars);
                chunks.push(make_chunk(
                    document,
                    &current,
                    chunk_index,
                    current_section.clone(),
                    None,
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
            chunks.push(make_chunk(document, &current, chunk_index, current_section, None));
        }

        chunks
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
        return None;
    }
    let after = &first_line[hash_count..];
    if after.starts_with(' ') {
        let text = after.trim();
        if !text.is_empty() {
            return Some(text.to_string());
        }
    }
    None
}

/// Return the last `n` characters of `s` as an owned `String`.
fn tail_chars(s: &str, n: usize) -> String {
    s.chars().rev().take(n).collect::<String>().chars().rev().collect()
}

fn make_chunk(
    document: &Document,
    text: &str,
    chunk_index: u32,
    section: Option<String>,
    page: Option<u32>,
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
            page,
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
            sections: vec![],
            url: None,
            metadata: DocumentMetadata::default(),
        }
    }

    fn doc_with_sections(sections: Vec<ContentSection>) -> Document {
        let content = sections.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join("\n\n");
        Document {
            id: DocumentId::new(),
            source_id: SourceId::from_str("test"),
            title: "PDF Doc".into(),
            content,
            sections,
            url: None,
            metadata: DocumentMetadata::default(),
        }
    }

    // ── Content-path (heading-aware) tests ─────────────────────────────────

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
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].metadata.section.as_deref(), Some("Introduction"));
        assert!(!chunks[0].text.contains("## Introduction"));
    }

    #[tokio::test]
    async fn heading_flushes_previous_section_without_overlap() {
        let content = "## Section A\n\nContent for A.\n\n## Section B\n\nContent for B.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].metadata.section.as_deref(), Some("Section A"));
        assert!(chunks[0].text.contains("Content for A"));
        assert_eq!(chunks[1].metadata.section.as_deref(), Some("Section B"));
        assert!(chunks[1].text.contains("Content for B"));
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
        let content = "####### Not a heading\n\nBody text.";
        let c = ParagraphChunker::default();
        let chunks = c.chunk(&doc(content)).await.unwrap();
        assert!(chunks[0].metadata.section.is_none());
    }

    #[tokio::test]
    async fn extract_heading_helper() {
        assert_eq!(extract_heading("# Title"), Some("Title".into()));
        assert_eq!(extract_heading("## Sub"), Some("Sub".into()));
        assert_eq!(extract_heading("#NoSpace"), None);
        assert_eq!(extract_heading("####### Too many"), None);
        assert_eq!(extract_heading("Normal paragraph"), None);
        assert_eq!(extract_heading("# "), None);
    }

    // ── Section-path tests (extension-worker documents) ────────────────────

    #[tokio::test]
    async fn sections_carry_page_and_title_into_chunk_metadata() {
        let c = ParagraphChunker::default();
        let d = doc_with_sections(vec![
            ContentSection {
                title: Some("Introduction".into()),
                text: "This is the introduction.".into(),
                page: Some(1),
            },
            ContentSection {
                title: Some("Results".into()),
                text: "These are the results.".into(),
                page: Some(5),
            },
        ]);
        let chunks = c.chunk(&d).await.unwrap();
        assert_eq!(chunks.len(), 2);
        assert_eq!(chunks[0].metadata.section.as_deref(), Some("Introduction"));
        assert_eq!(chunks[0].metadata.page, Some(1));
        assert_eq!(chunks[1].metadata.section.as_deref(), Some("Results"));
        assert_eq!(chunks[1].metadata.page, Some(5));
    }

    #[tokio::test]
    async fn section_without_title_produces_chunk_with_no_section() {
        let c = ParagraphChunker::default();
        let d = doc_with_sections(vec![ContentSection {
            title: None,
            text: "Untitled content.".into(),
            page: Some(2),
        }]);
        let chunks = c.chunk(&d).await.unwrap();
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].metadata.section.is_none());
        assert_eq!(chunks[0].metadata.page, Some(2));
    }

    #[tokio::test]
    async fn large_section_splits_into_multiple_chunks_same_page() {
        let c = ParagraphChunker { max_chars: 50, overlap_chars: 10 };
        let long_text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph here.";
        let d = doc_with_sections(vec![ContentSection {
            title: Some("Long Section".into()),
            text: long_text.into(),
            page: Some(3),
        }]);
        let chunks = c.chunk(&d).await.unwrap();
        // Should produce multiple chunks, each on the same page.
        assert!(chunks.len() > 1, "expected split, got {} chunks", chunks.len());
        for chunk in &chunks {
            assert_eq!(chunk.metadata.section.as_deref(), Some("Long Section"));
            assert_eq!(chunk.metadata.page, Some(3));
        }
    }

    #[tokio::test]
    async fn no_cross_section_overlap_in_section_mode() {
        let c = ParagraphChunker { max_chars: 20, overlap_chars: 10 };
        let d = doc_with_sections(vec![
            ContentSection {
                title: Some("A".into()),
                text: "Section A content.".into(),
                page: Some(1),
            },
            ContentSection {
                title: Some("B".into()),
                text: "Section B content.".into(),
                page: Some(2),
            },
        ]);
        let chunks = c.chunk(&d).await.unwrap();
        // Section B chunk must not contain text from Section A.
        let b_chunks: Vec<_> = chunks.iter().filter(|c| c.metadata.section.as_deref() == Some("B")).collect();
        assert!(!b_chunks.is_empty());
        for chunk in &b_chunks {
            assert!(
                !chunk.text.contains("Section A"),
                "Section B chunk contains Section A text: {}",
                chunk.text
            );
        }
    }

    #[tokio::test]
    async fn section_mode_chunk_indices_are_sequential() {
        let c = ParagraphChunker::default();
        let d = doc_with_sections(vec![
            ContentSection { title: Some("S1".into()), text: "One.".into(), page: Some(1) },
            ContentSection { title: Some("S2".into()), text: "Two.".into(), page: Some(2) },
            ContentSection { title: Some("S3".into()), text: "Three.".into(), page: Some(3) },
        ]);
        let chunks = c.chunk(&d).await.unwrap();
        assert_eq!(chunks.len(), 3);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.chunk_index, i as u32);
        }
    }
}
