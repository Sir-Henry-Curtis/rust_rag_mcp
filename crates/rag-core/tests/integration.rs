use std::sync::Arc;

use rag_core::{
    chunker::ParagraphChunker,
    embedder::MockEmbedder,
    indexer::Indexer,
    models::{Document, DocumentId, DocumentMetadata, SourceId},
    retriever::StandardRetriever,
    store::MemoryVectorStore,
    traits::{Retriever, VectorStore},
};

fn sample_document() -> Document {
    Document {
        id: DocumentId::new(),
        source_id: SourceId::from_str("test-source"),
        title: "Thermal Constraints Report".to_string(),
        content: [
            "This report summarizes thermal constraints for the Q3 deployment.",
            "The primary concern is heat dissipation in high-density server racks.",
            "Ambient temperature must not exceed 35°C at the intake.",
            "Cooling capacity was evaluated across three scenarios.",
            "Scenario A assumes 80% utilization with standard airflow.",
            "Scenario B introduces rear-door heat exchangers for dense rows.",
            "Scenario C models a hybrid liquid-cooling approach for GPU nodes.",
        ]
        .join("\n\n"),
        url: Some("https://example.com/reports/thermal-q3".to_string()),
        metadata: DocumentMetadata::default(),
    }
}

#[tokio::test]
async fn index_and_search_returns_results() {
    let store = Arc::new(MemoryVectorStore::default());
    let chunker = Arc::new(ParagraphChunker::default());
    let embedder = Arc::new(MockEmbedder::default());

    let indexer = Indexer::new(chunker, embedder.clone(), store.clone());
    let doc = sample_document();

    let chunk_count = indexer.index_document(&doc).await.unwrap();
    assert!(chunk_count > 0, "at least one chunk must be indexed");

    let total = store.count_chunks().await.unwrap();
    assert_eq!(total, chunk_count);

    let retriever = StandardRetriever::new(embedder, store);
    let results = retriever.search("thermal constraints", 3, None, None).await.unwrap();

    assert!(!results.is_empty(), "search must return at least one result");

    let first = &results[0];
    assert_eq!(first.title, "Thermal Constraints Report");
    assert!(!first.citation.label.is_empty());
    assert!(first.citation.url.is_some());
    assert!(first.score >= -1.0 && first.score <= 1.0);
}

#[tokio::test]
async fn delete_document_removes_chunks() {
    let store = Arc::new(MemoryVectorStore::default());
    let chunker = Arc::new(ParagraphChunker::default());
    let embedder = Arc::new(MockEmbedder::default());

    let indexer = Indexer::new(chunker, embedder, store.clone());
    let doc = sample_document();
    let doc_id = doc.id.clone();

    indexer.index_document(&doc).await.unwrap();
    assert!(store.count_chunks().await.unwrap() > 0);

    indexer.delete_document(&doc_id).await.unwrap();
    assert_eq!(store.count_chunks().await.unwrap(), 0);
}

#[tokio::test]
async fn citation_includes_page_when_present() {
    use rag_core::models::Citation;

    let with_page = Citation::build("Annual Report", Some(42), Some("https://example.com".into()));
    assert_eq!(with_page.label, "Annual Report, p. 42");

    let without_page = Citation::build("Annual Report", None, None);
    assert_eq!(without_page.label, "Annual Report");
}
