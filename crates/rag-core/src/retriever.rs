use std::sync::Arc;

use async_trait::async_trait;
use tracing::debug;

use crate::error::RagError;
use crate::models::{CallerContext, Citation, SearchFilter, SearchResult, ScoredChunk};
use crate::traits::{Embedder, PermissionFilter, Retriever, VectorStore};

/// Standard retrieval pipeline: embed query → vector search → permission filter → citations.
pub struct StandardRetriever {
    embedder: Arc<dyn Embedder>,
    store: Arc<dyn VectorStore>,
    permission_filter: Option<Arc<dyn PermissionFilter>>,
}

impl StandardRetriever {
    pub fn new(embedder: Arc<dyn Embedder>, store: Arc<dyn VectorStore>) -> Self {
        Self { embedder, store, permission_filter: None }
    }

    pub fn with_permission_filter(mut self, filter: Arc<dyn PermissionFilter>) -> Self {
        self.permission_filter = Some(filter);
        self
    }
}

#[async_trait]
impl Retriever for StandardRetriever {
    async fn search(
        &self,
        query: &str,
        k: usize,
        filter: Option<&SearchFilter>,
        caller: Option<&CallerContext>,
    ) -> Result<Vec<SearchResult>, RagError> {
        let query_embedding = self.embedder.embed_query(query).await?;

        // Overfetch when a permission filter is present so we can still
        // return k results after filtering drops some candidates.
        let fetch_k = if self.permission_filter.is_some() { k * 3 } else { k };

        let mut scored = self.store.search(&query_embedding, fetch_k, filter).await?;

        if let (Some(pf), Some(ctx)) = (&self.permission_filter, caller) {
            scored = pf.filter(ctx, scored).await?;
        }

        let results = scored
            .into_iter()
            .take(k)
            .map(scored_chunk_to_result)
            .collect();

        debug!(query, result_count = k, "search complete");
        Ok(results)
    }
}

fn scored_chunk_to_result(sc: ScoredChunk) -> SearchResult {
    let citation = Citation::build(
        &sc.chunk.metadata.document_title,
        sc.chunk.metadata.page,
        sc.chunk.metadata.document_url.clone(),
    );

    SearchResult {
        chunk_id: sc.chunk.id,
        document_id: sc.chunk.document_id,
        source_id: sc.chunk.source_id,
        title: sc.chunk.metadata.document_title,
        source_url: sc.chunk.metadata.document_url,
        snippet: sc.chunk.text,
        score: sc.score,
        page: sc.chunk.metadata.page,
        chunk_index: sc.chunk.chunk_index,
        modified_at: sc.chunk.metadata.modified_at,
        citation,
    }
}
