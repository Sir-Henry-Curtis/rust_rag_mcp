//! Indexing lifecycle event publisher (`rag/events/**`).

use std::sync::Arc;

use chrono::Utc;
use serde::Serialize;
use tracing::debug;

use anyhow::anyhow;
use rag_core::RagError;

use crate::config::ZenohConfig;

// ── Event payloads ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct IndexStartedEvent<'a> {
    source_id: &'a str,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct IndexProgressEvent<'a> {
    source_id: &'a str,
    indexed: usize,
    total: usize,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct DocumentIndexedEvent<'a> {
    source_id: &'a str,
    document_id: &'a str,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct IndexFailedEvent<'a> {
    source_id: &'a str,
    error: &'a str,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SyncStartedEvent<'a> {
    source_id: &'a str,
    timestamp: chrono::DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct SyncCompletedEvent<'a> {
    source_id: &'a str,
    changed: usize,
    timestamp: chrono::DateTime<Utc>,
}

// ── Publisher ─────────────────────────────────────────────────────────────────

/// Publishes indexing lifecycle events to the Zenoh key space `{prefix}/events/**`.
///
/// Subscribers (monitoring dashboards, audit services, etc.) can subscribe to
/// individual event types or the entire `rag/events/**` wildcard.
#[derive(Clone)]
pub struct EventPublisher {
    session: Arc<zenoh::Session>,
    config: Arc<ZenohConfig>,
}

impl EventPublisher {
    pub fn new(session: Arc<zenoh::Session>, config: Arc<ZenohConfig>) -> Self {
        Self { session, config }
    }

    /// A full index run has started for `source_id`.
    pub async fn index_started(&self, source_id: &str) -> Result<(), RagError> {
        self.publish(
            "index_started",
            &IndexStartedEvent { source_id, timestamp: Utc::now() },
        )
        .await
    }

    /// `indexed` out of `total` documents have been processed so far.
    pub async fn index_progress(
        &self,
        source_id: &str,
        indexed: usize,
        total: usize,
    ) -> Result<(), RagError> {
        self.publish(
            "index_progress",
            &IndexProgressEvent { source_id, indexed, total, timestamp: Utc::now() },
        )
        .await
    }

    /// A single document has been indexed successfully.
    pub async fn document_indexed(
        &self,
        source_id: &str,
        document_id: &str,
    ) -> Result<(), RagError> {
        self.publish(
            "document_indexed",
            &DocumentIndexedEvent { source_id, document_id, timestamp: Utc::now() },
        )
        .await
    }

    /// The full index run failed with `error`.
    pub async fn index_failed(&self, source_id: &str, error: &str) -> Result<(), RagError> {
        self.publish(
            "index_failed",
            &IndexFailedEvent { source_id, error, timestamp: Utc::now() },
        )
        .await
    }

    /// An incremental sync has started for `source_id`.
    pub async fn sync_started(&self, source_id: &str) -> Result<(), RagError> {
        self.publish(
            "sync_started",
            &SyncStartedEvent { source_id, timestamp: Utc::now() },
        )
        .await
    }

    /// An incremental sync completed; `changed` documents were updated.
    pub async fn sync_completed(&self, source_id: &str, changed: usize) -> Result<(), RagError> {
        self.publish(
            "sync_completed",
            &SyncCompletedEvent { source_id, changed, timestamp: Utc::now() },
        )
        .await
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    async fn publish<T: Serialize>(&self, event: &str, payload: &T) -> Result<(), RagError> {
        let key = self.config.event_key(event);
        let bytes = serde_json::to_vec(payload).map_err(RagError::Serialization)?;
        debug!(event, "publishing zenoh event");
        self.session
            .put(&key, bytes)
            .await
            .map_err(|e| RagError::Other(anyhow!("zenoh put {key}: {e}")))
    }
}
