//! Integration tests for PgVectorStore.
//!
//! These tests require a live PostgreSQL + pgvector instance. Set
//! `TEST_DATABASE_URL` to a Postgres connection string before running:
//!
//! ```sh
//! docker compose up -d
//! TEST_DATABASE_URL=postgres://rag:rag_password@localhost:5432/rag_dev \
//!     cargo test -p rag-store-pgvector
//! ```
//!
//! All tests use a unique `source_id` derived from a UUID so they can run
//! concurrently against a shared database without interfering with each other.

use rag_core::{
    models::{Chunk, ChunkId, ChunkMetadata, DocumentId, SearchFilter, SourceId},
    traits::VectorStore,
};
use rag_store_pgvector::PgVectorStore;
use uuid::Uuid;

/// Returns the test database URL, or `None` if the env var is not set.
fn db_url() -> Option<String> {
    std::env::var("TEST_DATABASE_URL").ok()
}

/// Build a normalised test embedding with a 1 at position `hot` and 0s elsewhere.
fn one_hot(dim: usize, hot: usize) -> Vec<f32> {
    let mut v = vec![0.0f32; dim];
    v[hot] = 1.0;
    v
}

fn make_chunk(
    source_id: &SourceId,
    doc_suffix: &str,
    index: u32,
    embedding: Vec<f32>,
) -> Chunk {
    Chunk {
        id: ChunkId::new(),
        document_id: DocumentId::from_str(format!("doc-{doc_suffix}")),
        source_id: source_id.clone(),
        text: format!("chunk {index} text"),
        chunk_index: index,
        embedding: Some(embedding),
        metadata: ChunkMetadata {
            document_title: format!("Doc {doc_suffix}"),
            ..Default::default()
        },
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn upsert_and_count() {
    let Some(url) = db_url() else {
        eprintln!("skipping upsert_and_count: TEST_DATABASE_URL not set");
        return;
    };

    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    // Start clean.
    store.delete_by_source(&sid).await.unwrap();

    let chunks = vec![
        make_chunk(&sid, "a", 0, one_hot(4, 0)),
        make_chunk(&sid, "a", 1, one_hot(4, 1)),
        make_chunk(&sid, "b", 0, one_hot(4, 2)),
    ];
    store.upsert_chunks(&chunks).await.unwrap();

    let total = store.count_chunks().await.unwrap();
    assert!(total >= 3, "expected at least 3 chunks, got {total}");

    store.delete_by_source(&sid).await.unwrap();
}

#[tokio::test]
async fn search_returns_closest_chunk() {
    let Some(url) = db_url() else {
        eprintln!("skipping search_returns_closest_chunk: TEST_DATABASE_URL not set");
        return;
    };

    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    store.delete_by_source(&sid).await.unwrap();

    // chunk 0: aligned with dimension 0  →  cosine similarity 1.0 with [1,0,0,0]
    // chunk 1: aligned with dimension 1  →  cosine similarity 0.0 with [1,0,0,0]
    let c0 = make_chunk(&sid, "x", 0, one_hot(4, 0));
    let c1 = make_chunk(&sid, "x", 1, one_hot(4, 1));
    store.upsert_chunks(&[c0.clone(), c1]).await.unwrap();

    let results = store.search(&one_hot(4, 0), 2, None).await.unwrap();
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].chunk.id, c0.id, "closest chunk should be first");
    assert!(
        (results[0].score - 1.0).abs() < 1e-4,
        "identical vectors → score ≈ 1.0, got {}",
        results[0].score
    );
    assert!(
        results[0].score >= results[1].score,
        "results should be sorted descending by score"
    );

    store.delete_by_source(&sid).await.unwrap();
}

#[tokio::test]
async fn search_with_source_filter() {
    let Some(url) = db_url() else {
        eprintln!("skipping search_with_source_filter: TEST_DATABASE_URL not set");
        return;
    };

    let sid_a = SourceId::from_str(format!("test-a-{}", Uuid::new_v4()));
    let sid_b = SourceId::from_str(format!("test-b-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    store.delete_by_source(&sid_a).await.unwrap();
    store.delete_by_source(&sid_b).await.unwrap();

    store
        .upsert_chunks(&[make_chunk(&sid_a, "p", 0, one_hot(4, 0))])
        .await
        .unwrap();
    store
        .upsert_chunks(&[make_chunk(&sid_b, "q", 0, one_hot(4, 0))])
        .await
        .unwrap();

    let filter = SearchFilter::by_source(sid_a.clone());
    let results = store.search(&one_hot(4, 0), 10, Some(&filter)).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk.source_id, sid_a);

    store.delete_by_source(&sid_a).await.unwrap();
    store.delete_by_source(&sid_b).await.unwrap();
}

#[tokio::test]
async fn delete_by_document_removes_only_that_document() {
    let Some(url) = db_url() else {
        eprintln!("skipping delete_by_document: TEST_DATABASE_URL not set");
        return;
    };

    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    store.delete_by_source(&sid).await.unwrap();

    let c0 = make_chunk(&sid, "keep", 0, one_hot(4, 0));
    let c1 = make_chunk(&sid, "delete", 0, one_hot(4, 1));
    let doc_to_delete = c1.document_id.clone();

    store.upsert_chunks(&[c0.clone(), c1]).await.unwrap();
    store.delete_by_document(&doc_to_delete).await.unwrap();

    let filter = SearchFilter::by_source(sid.clone());
    let results = store.search(&one_hot(4, 0), 10, Some(&filter)).await.unwrap();

    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk.id, c0.id, "only 'keep' doc chunk should remain");

    store.delete_by_source(&sid).await.unwrap();
}

#[tokio::test]
async fn upsert_updates_existing_chunk() {
    let Some(url) = db_url() else {
        eprintln!("skipping upsert_updates_existing_chunk: TEST_DATABASE_URL not set");
        return;
    };

    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    store.delete_by_source(&sid).await.unwrap();

    let id = ChunkId::new();
    let original = Chunk {
        id: id.clone(),
        document_id: DocumentId::from_str("doc-u"),
        source_id: sid.clone(),
        text: "original".into(),
        chunk_index: 0,
        embedding: Some(one_hot(4, 0)),
        metadata: ChunkMetadata::default(),
    };
    store.upsert_chunks(&[original]).await.unwrap();

    // Re-upsert with updated text and embedding.
    let updated = Chunk {
        id: id.clone(),
        document_id: DocumentId::from_str("doc-u"),
        source_id: sid.clone(),
        text: "updated".into(),
        chunk_index: 0,
        embedding: Some(one_hot(4, 3)),
        metadata: ChunkMetadata::default(),
    };
    store.upsert_chunks(&[updated]).await.unwrap();

    // The updated chunk should score highest against dimension 3.
    let filter = SearchFilter::by_source(sid.clone());
    let results = store.search(&one_hot(4, 3), 1, Some(&filter)).await.unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].chunk.text, "updated");

    store.delete_by_source(&sid).await.unwrap();
}

#[tokio::test]
async fn dimension_mismatch_is_rejected() {
    let Some(url) = db_url() else {
        eprintln!("skipping dimension_mismatch: TEST_DATABASE_URL not set");
        return;
    };

    // Connect with dimension 4 to initialise the meta record.
    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();
    store.delete_by_source(&sid).await.unwrap();

    // A second connect with a different dimension should fail.
    // (Use a fresh DB URL with a different schema if needed; here we rely on
    // the rag_meta dimension guard.)
    let result = PgVectorStore::connect(&url, 768).await;
    assert!(
        result.is_err(),
        "connecting with mismatched dimension should return Err"
    );
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("mismatch"),
        "error should mention mismatch, got: {msg}"
    );
}

#[tokio::test]
async fn chunk_counts_by_source_and_last_indexed() {
    let Some(url) = db_url() else {
        eprintln!("skipping inspection: TEST_DATABASE_URL not set");
        return;
    };

    let sid = SourceId::from_str(format!("test-{}", Uuid::new_v4()));
    let store = PgVectorStore::connect(&url, 4).await.unwrap();

    store.delete_by_source(&sid).await.unwrap();

    store
        .upsert_chunks(&[
            make_chunk(&sid, "d1", 0, one_hot(4, 0)),
            make_chunk(&sid, "d1", 1, one_hot(4, 1)),
        ])
        .await
        .unwrap();

    let counts = store.chunk_counts_by_source().await.unwrap();
    let my_count = counts
        .iter()
        .find(|(s, _)| s == &sid.0)
        .map(|(_, n)| *n)
        .unwrap_or(0);
    assert_eq!(my_count, 2);

    let ts = store.last_indexed_at(&sid).await.unwrap();
    assert!(ts.is_some(), "last_indexed_at should return Some after indexing");

    store.delete_by_source(&sid).await.unwrap();
}
