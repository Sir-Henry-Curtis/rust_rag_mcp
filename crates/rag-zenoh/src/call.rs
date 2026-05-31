//! Request/reply calls from the Rust runtime to extension workers via Zenoh queryables.

use std::{sync::Arc, time::Duration};

use tracing::debug;

use anyhow::anyhow;
use rag_core::RagError;
use rag_extension_protocol::{
    EmbedTextsRequest, EmbedTextsResponse, ExtensionCapability, LoadDocumentRequest,
    LoadDocumentResponse, RerankRequest, RerankResponse, RequestContext, RequestEnvelope,
    ResponseEnvelope,
};

use crate::config::ZenohConfig;

// ── Caller ────────────────────────────────────────────────────────────────────

/// Makes typed request/reply calls to extension workers via Zenoh queryables.
///
/// Each call:
/// 1. Wraps the typed payload in a [`RequestEnvelope`].
/// 2. Serialises it and sends it as a Zenoh `get` query to
///    `{prefix}/call/{worker_id}/{op}`.
/// 3. Waits for the first reply within the configured timeout.
/// 4. Deserialises the [`ResponseEnvelope`] and unwraps the typed payload.
#[derive(Clone)]
pub struct ZenohCaller {
    session: Arc<zenoh::Session>,
    config: Arc<ZenohConfig>,
    timeout: Duration,
}

impl ZenohCaller {
    pub fn new(session: Arc<zenoh::Session>, config: Arc<ZenohConfig>) -> Self {
        let timeout = Duration::from_secs(config.call_timeout_secs);
        Self { session, config, timeout }
    }

    // ── Public typed API ──────────────────────────────────────────────────────

    /// Ask a `load_document` worker to parse raw file bytes and return text.
    pub async fn load_document(
        &self,
        worker_id: &str,
        request: LoadDocumentRequest,
    ) -> Result<LoadDocumentResponse, RagError> {
        let payload = serde_json::to_value(&request).map_err(RagError::Serialization)?;
        let envelope = RequestEnvelope::new(
            ExtensionCapability::LoadDocument,
            payload,
            RequestContext::default(),
        );
        let resp = self.call(worker_id, "load", envelope).await?;
        serde_json::from_value(resp.payload).map_err(RagError::Serialization)
    }

    /// Ask an `embed_texts` worker to produce dense embeddings for a batch of texts.
    pub async fn embed_texts(
        &self,
        worker_id: &str,
        request: EmbedTextsRequest,
    ) -> Result<EmbedTextsResponse, RagError> {
        let payload = serde_json::to_value(&request).map_err(RagError::Serialization)?;
        let envelope = RequestEnvelope::new(
            ExtensionCapability::EmbedTexts,
            payload,
            RequestContext::default(),
        );
        let resp = self.call(worker_id, "embed", envelope).await?;
        serde_json::from_value(resp.payload).map_err(RagError::Serialization)
    }

    /// Ask a `rerank` worker to reorder candidates by relevance to the query.
    pub async fn rerank(
        &self,
        worker_id: &str,
        request: RerankRequest,
    ) -> Result<RerankResponse, RagError> {
        let payload = serde_json::to_value(&request).map_err(RagError::Serialization)?;
        let envelope = RequestEnvelope::new(
            ExtensionCapability::Rerank,
            payload,
            RequestContext::default(),
        );
        let resp = self.call(worker_id, "rerank", envelope).await?;
        serde_json::from_value(resp.payload).map_err(RagError::Serialization)
    }

    // ── Generic inner call ────────────────────────────────────────────────────

    async fn call(
        &self,
        worker_id: &str,
        op: &str,
        envelope: RequestEnvelope,
    ) -> Result<ResponseEnvelope, RagError> {
        let key = self.config.call_key(worker_id, op);
        let request_id = envelope.request_id.clone();

        let body = serde_json::to_vec(&envelope).map_err(RagError::Serialization)?;
        debug!(worker_id, op, request_id = %request_id, "sending zenoh call");

        let replies = self
            .session
            .get(&key)
            .payload(body)
            .timeout(self.timeout)
            .await
            .map_err(|e| RagError::Other(anyhow!("zenoh get {key}: {e}")))?;

        // Take the first reply; further replies (if any) are ignored.
        let reply = replies
            .recv_async()
            .await
            .map_err(|_| RagError::Other(anyhow!("no reply from worker {worker_id} for {op}")))?;

        let sample = reply.result().map_err(|e| {
            RagError::Other(anyhow!("worker {worker_id}/{op} returned error: {e}"))
        })?;

        let bytes: Vec<u8> = sample.payload().to_bytes().into_owned();
        let resp: ResponseEnvelope =
            serde_json::from_slice(&bytes).map_err(RagError::Serialization)?;

        if !resp.success {
            let msg = resp.error.unwrap_or_else(|| "unknown error".into());
            return Err(RagError::Other(anyhow!("worker {worker_id}/{op} failed: {msg}")));
        }

        debug!(worker_id, op, "call succeeded");
        Ok(resp)
    }
}
