//! Extension worker registry: discover workers via announce/heartbeat, evict stale ones.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use tracing::{debug, info, warn};

use anyhow::anyhow;
use rag_core::RagError;
use rag_extension_protocol::{CapabilityDescriptor, ExtensionCapability, Heartbeat};

use crate::config::ZenohConfig;

// ── Internal worker state ─────────────────────────────────────────────────────

#[derive(Debug)]
struct WorkerEntry {
    descriptor: CapabilityDescriptor,
    last_heartbeat: Instant,
}

// ── Registry ──────────────────────────────────────────────────────────────────

/// Tracks registered extension workers discovered via Zenoh announce/heartbeat.
///
/// # Lifecycle
///
/// 1. Call [`ExtensionRegistry::start`] once to create a registry and launch
///    background subscriber tasks.
/// 2. Use [`find_loader_for`](Self::find_loader_for),
///    [`find_embedder`](Self::find_embedder), and
///    [`find_reranker`](Self::find_reranker) to route requests to workers.
/// 3. Drop the registry or call [`shutdown`](Self::shutdown) to stop background
///    tasks and unsubscribe from Zenoh.
#[derive(Clone)]
pub struct ExtensionRegistry {
    workers: Arc<Mutex<HashMap<String, WorkerEntry>>>,
    shutdown_tx: Arc<tokio::sync::watch::Sender<bool>>,
}

impl ExtensionRegistry {
    /// Start the registry against an open Zenoh session.
    ///
    /// Spawns background tasks to:
    /// - subscribe to announce messages and register new workers
    /// - subscribe to heartbeat messages and update last-seen timestamps
    /// - run a watchdog that evicts workers that miss `max_missed_heartbeats` consecutive beats
    pub async fn start(
        session: Arc<zenoh::Session>,
        config: Arc<ZenohConfig>,
    ) -> Result<Self, RagError> {
        let workers: Arc<Mutex<HashMap<String, WorkerEntry>>> = Default::default();
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        // ── Announce subscriber ───────────────────────────────────────────────
        let announce_sub = session
            .declare_subscriber(config.announce_wildcard())
            .await
            .map_err(|e| RagError::Other(anyhow!("subscribe announce: {e}")))?;

        let workers_ann = workers.clone();
        let mut rx_ann = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = rx_ann.changed() => break,
                    Ok(sample) = announce_sub.recv_async() => {
                        let bytes: Vec<u8> = sample.payload().to_bytes().into_owned();
                        match serde_json::from_slice::<CapabilityDescriptor>(&bytes) {
                            Ok(desc) => {
                                info!(
                                    worker_id = %desc.extension_id,
                                    capabilities = ?desc.capabilities,
                                    "worker announced"
                                );
                                workers_ann.lock().unwrap().insert(
                                    desc.extension_id.clone(),
                                    WorkerEntry {
                                        descriptor: desc,
                                        last_heartbeat: Instant::now(),
                                    },
                                );
                            }
                            Err(e) => warn!("invalid announce payload: {e}"),
                        }
                    }
                }
            }
        });

        // ── Heartbeat subscriber ──────────────────────────────────────────────
        let heartbeat_sub = session
            .declare_subscriber(config.heartbeat_wildcard())
            .await
            .map_err(|e| RagError::Other(anyhow!("subscribe heartbeat: {e}")))?;

        let workers_hb = workers.clone();
        let mut rx_hb = shutdown_rx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    biased;
                    _ = rx_hb.changed() => break,
                    Ok(sample) = heartbeat_sub.recv_async() => {
                        let bytes: Vec<u8> = sample.payload().to_bytes().into_owned();
                        match serde_json::from_slice::<Heartbeat>(&bytes) {
                            Ok(hb) => {
                                debug!(worker_id = %hb.extension_id, "heartbeat received");
                                let mut map = workers_hb.lock().unwrap();
                                if let Some(entry) = map.get_mut(&hb.extension_id) {
                                    entry.last_heartbeat = Instant::now();
                                }
                                // Heartbeat from an unknown worker is ignored; it
                                // must send a proper announce first.
                            }
                            Err(e) => warn!("invalid heartbeat payload: {e}"),
                        }
                    }
                }
            }
        });

        // ── Watchdog task ─────────────────────────────────────────────────────
        let workers_wd = workers.clone();
        let cfg_wd = config.clone();
        let mut rx_wd = shutdown_rx.clone();
        tokio::spawn(async move {
            let tick = Duration::from_secs(cfg_wd.heartbeat_interval_secs);
            let max_age = tick * cfg_wd.max_missed_heartbeats as u32;
            let mut interval = tokio::time::interval(tick);
            loop {
                tokio::select! {
                    biased;
                    _ = rx_wd.changed() => break,
                    _ = interval.tick() => {
                        workers_wd.lock().unwrap().retain(|id, entry| {
                            let age = entry.last_heartbeat.elapsed();
                            if age > max_age {
                                warn!(worker_id = %id, age_secs = age.as_secs(), "evicting stale worker");
                                false
                            } else {
                                true
                            }
                        });
                    }
                }
            }
        });

        Ok(Self {
            workers,
            shutdown_tx: Arc::new(shutdown_tx),
        })
    }

    /// Stop all background tasks and unsubscribe from Zenoh.
    pub fn shutdown(&self) {
        let _ = self.shutdown_tx.send(true);
    }

    // ── Worker queries ────────────────────────────────────────────────────────

    /// Return the ID of the first registered worker that can load documents of
    /// the given MIME content type (e.g. `"application/pdf"`).
    pub fn find_loader_for(&self, content_type: &str) -> Option<String> {
        let map = self.workers.lock().unwrap();
        map.values()
            .find(|e| {
                e.descriptor.capabilities.contains(&ExtensionCapability::LoadDocument)
                    && (e.descriptor.content_types.is_empty()
                        || e.descriptor
                            .content_types
                            .iter()
                            .any(|ct| ct.eq_ignore_ascii_case(content_type)))
            })
            .map(|e| e.descriptor.extension_id.clone())
    }

    /// Return the ID of the first registered worker that can embed texts.
    pub fn find_embedder(&self) -> Option<String> {
        let map = self.workers.lock().unwrap();
        map.values()
            .find(|e| {
                e.descriptor
                    .capabilities
                    .contains(&ExtensionCapability::EmbedTexts)
            })
            .map(|e| e.descriptor.extension_id.clone())
    }

    /// Return the ID of the first registered reranker worker.
    pub fn find_reranker(&self) -> Option<String> {
        let map = self.workers.lock().unwrap();
        map.values()
            .find(|e| {
                e.descriptor
                    .capabilities
                    .contains(&ExtensionCapability::Rerank)
            })
            .map(|e| e.descriptor.extension_id.clone())
    }

    /// Return capability descriptors for all currently registered workers.
    pub fn list_workers(&self) -> Vec<CapabilityDescriptor> {
        self.workers
            .lock()
            .unwrap()
            .values()
            .map(|e| e.descriptor.clone())
            .collect()
    }

    /// Number of registered workers.
    pub fn worker_count(&self) -> usize {
        self.workers.lock().unwrap().len()
    }

    /// Directly register a worker (used in tests and the Python SDK announce handler).
    #[doc(hidden)]
    pub fn register(&self, descriptor: CapabilityDescriptor) {
        self.workers.lock().unwrap().insert(
            descriptor.extension_id.clone(),
            WorkerEntry { descriptor, last_heartbeat: Instant::now() },
        );
    }
}

impl Drop for ExtensionRegistry {
    fn drop(&mut self) {
        self.shutdown();
    }
}
