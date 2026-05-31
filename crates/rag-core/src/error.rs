use thiserror::Error;

#[derive(Error, Debug)]
pub enum RagError {
    #[error("connector error: {0}")]
    Connector(String),

    #[error("chunker error: {0}")]
    Chunker(String),

    #[error("embedder error: {0}")]
    Embedder(String),

    #[error("store error: {0}")]
    Store(String),

    #[error("retriever error: {0}")]
    Retriever(String),

    #[error("permission denied: {0}")]
    Permission(String),

    #[error("extension error: {0}")]
    Extension(String),

    #[error("serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error(transparent)]
    Other(#[from] anyhow::Error),
}
