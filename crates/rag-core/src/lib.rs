pub mod chunker;
pub mod embedder;
pub mod error;
pub mod indexer;
pub mod models;
pub mod retriever;
pub mod store;
pub mod traits;

pub use error::RagError;
pub use models::{
    CallerContext, ChunkId, ChunkMetadata, Citation, DocumentId, DocumentMetadata, DocumentRef,
    Document, SearchFilter, SearchResult, ScoredChunk, Source, SourceId,
};
pub use traits::{
    ChangeEvent, ChangeKind, Chunker, Connector, Embedder, PermissionFilter, Reranker, Retriever,
    VectorStore,
};
