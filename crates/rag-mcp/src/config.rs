use serde::{Deserialize, Serialize};

/// Top-level `rag.toml` configuration loaded at server startup.
#[derive(Debug, Clone, Deserialize, Serialize, Default)]
pub struct RagConfig {
    #[serde(default)]
    pub store: StoreConfig,

    #[serde(default)]
    pub embedder: EmbedderConfig,
}

// ── Store ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct StoreConfig {
    /// Which vector store backend to use.
    /// Accepted values: `"memory"`, `"pgvector"`.
    pub backend: StoreBackend,

    /// PostgreSQL connection URL.
    /// Required when `backend = "pgvector"`.
    /// Example: `postgres://rag:password@localhost:5432/rag_dev`
    pub database_url: Option<String>,
}

impl Default for StoreConfig {
    fn default() -> Self {
        Self {
            backend: StoreBackend::Memory,
            database_url: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum StoreBackend {
    #[default]
    Memory,
    Pgvector,
}

// ── Embedder ──────────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct EmbedderConfig {
    /// Which embedding provider to use.
    /// Accepted values: `"mock"`, `"openai"`, `"local-onnx"`.
    pub provider: EmbedderProvider,

    /// Output dimension for the selected model.
    /// Must match any existing pgvector index dimension.
    /// Defaults: mock → 384, openai text-embedding-3-small → 1536,
    ///           bge-m3 → 1024, all-MiniLM-L6-v2 → 384.
    pub dimension: usize,

    // ── OpenAI ───────────────────────────────────────────────────────────
    /// OpenAI model name. Defaults to `text-embedding-3-small`.
    pub openai_model: Option<String>,

    // ── Local ONNX ───────────────────────────────────────────────────────
    /// HuggingFace model ID or local filesystem path to the ONNX model.
    /// Example: `"sentence-transformers/all-MiniLM-L6-v2"`
    pub model_path: Option<String>,
}

impl Default for EmbedderConfig {
    fn default() -> Self {
        Self {
            provider: EmbedderProvider::Mock,
            dimension: 384,
            openai_model: None,
            model_path: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default, PartialEq, Eq)]
#[serde(rename_all = "kebab-case")]
pub enum EmbedderProvider {
    #[default]
    Mock,
    Openai,
    LocalOnnx,
}

// ── Validation ────────────────────────────────────────────────────────────────

impl RagConfig {
    /// Validate the configuration at startup. Returns a human-readable error
    /// message for any missing required field so the operator knows exactly
    /// what to fix before the server will start.
    pub fn validate(&self) -> Result<(), String> {
        if self.store.backend == StoreBackend::Pgvector
            && self.store.database_url.is_none()
        {
            return Err(
                "store.database_url is required when store.backend = \"pgvector\"".into(),
            );
        }
        if self.embedder.provider == EmbedderProvider::Openai
            && std::env::var("OPENAI_API_KEY").is_err()
        {
            return Err(
                "OPENAI_API_KEY environment variable is required when \
                 embedder.provider = \"openai\""
                    .into(),
            );
        }
        if self.embedder.provider == EmbedderProvider::LocalOnnx
            && self.embedder.model_path.is_none()
        {
            return Err(
                "embedder.model_path is required when \
                 embedder.provider = \"local-onnx\""
                    .into(),
            );
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_is_valid() {
        RagConfig::default().validate().unwrap();
    }

    #[test]
    fn pgvector_without_url_is_invalid() {
        let cfg = RagConfig {
            store: StoreConfig {
                backend: StoreBackend::Pgvector,
                database_url: None,
            },
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }

    #[test]
    fn pgvector_with_url_is_valid() {
        let cfg = RagConfig {
            store: StoreConfig {
                backend: StoreBackend::Pgvector,
                database_url: Some("postgres://rag:pw@localhost/rag".into()),
            },
            ..Default::default()
        };
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn local_onnx_without_path_is_invalid() {
        let cfg = RagConfig {
            embedder: EmbedderConfig {
                provider: EmbedderProvider::LocalOnnx,
                model_path: None,
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(cfg.validate().is_err());
    }
}
