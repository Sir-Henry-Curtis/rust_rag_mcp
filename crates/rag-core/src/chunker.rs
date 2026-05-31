use async_trait::async_trait;
use tracing::debug;

use crate::error::RagError;
use crate::models::{Chunk, ChunkId, ChunkMetadata, Document};
use crate::traits::Chunker;

/// Splits documents on paragraph boundaries (`\n\n`), collecting paragraphs
/// into chunks up to `max_chars`. The last `overlap_chars` of each finished
/// chunk are prepended to the next one to preserve context across boundaries.
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

        let base_meta = ChunkMetadata {
            document_title: document.title.clone(),
            document_url: document.url.clone(),
            modified_at: document.metadata.modified_at,
            permissions: document.metadata.permissions.clone(),
            ..Default::default()
        };

        let mut chunks: Vec<Chunk> = Vec::new();
        let mut current = String::new();
        let mut chunk_index: u32 = 0;

        for para in paragraphs {
            // If adding this paragraph would exceed the limit, flush first.
            if !current.is_empty() && current.len() + para.len() > self.max_chars {
                let overlap: String = current
                    .chars()
                    .rev()
                    .take(self.overlap_chars)
                    .collect::<String>()
                    .chars()
                    .rev()
                    .collect();

                chunks.push(Chunk {
                    id: ChunkId::new(),
                    document_id: document.id.clone(),
                    source_id: document.source_id.clone(),
                    text: current.trim().to_string(),
                    chunk_index,
                    embedding: None,
                    metadata: base_meta.clone(),
                });
                chunk_index += 1;
                current = overlap;
            }

            if !current.is_empty() {
                current.push_str("\n\n");
            }
            current.push_str(para);
        }

        // Flush the final chunk.
        if !current.trim().is_empty() {
            chunks.push(Chunk {
                id: ChunkId::new(),
                document_id: document.id.clone(),
                source_id: document.source_id.clone(),
                text: current.trim().to_string(),
                chunk_index,
                embedding: None,
                metadata: base_meta,
            });
        }

        debug!(
            document_id = %document.id,
            chunk_count = chunks.len(),
            "chunked document"
        );

        Ok(chunks)
    }
}
