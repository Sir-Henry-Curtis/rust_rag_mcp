//! Integration tests for the Zenoh extension bus.
//!
//! All tests open two in-process Zenoh peer sessions so no external router is
//! required. Tests skip gracefully if a zenoh session cannot be opened (e.g.,
//! when multicast is unavailable in a restricted CI environment).
//!
//! Run with:
//! ```sh
//! cargo test -p rag-zenoh
//! ```

use std::{sync::Arc, time::Duration};

use rag_extension_protocol::{
    CapabilityDescriptor, EmbedTextsResponse, ExtensionCapability, Heartbeat,
    LoadDocumentResponse, RankedChunk, RerankResponse, ResponseEnvelope, WorkerStatus,
    PROTOCOL_VERSION,
};
use rag_zenoh::{EventPublisher, ExtensionRegistry, ZenohCaller, ZenohConfig, ZenohMode};

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Unique key prefix so tests don't interfere with each other.
fn test_prefix() -> String {
    format!("test/{}", uuid::Uuid::new_v4().to_string().replace('-', ""))
}

/// Find a free TCP port by briefly binding to 127.0.0.1:0.
fn free_port() -> u16 {
    use std::net::TcpListener;
    TcpListener::bind("127.0.0.1:0")
        .expect("bind to 127.0.0.1:0")
        .local_addr()
        .expect("local_addr")
        .port()
}

/// Create two peer sessions that communicate via explicit TCP loopback (no
/// multicast required).  Session A listens; session B connects to A.
/// Returns None if the sessions can't be opened.
async fn make_peer_pair(prefix: &str) -> Option<(Arc<zenoh::Session>, Arc<zenoh::Session>)> {
    let port = free_port();
    let listen_addr = format!("tcp/127.0.0.1:{port}");

    let cfg_a = ZenohConfig {
        mode: ZenohMode::Peer,
        listen_endpoints: vec![listen_addr.clone()],
        multicast_scouting: false,
        key_prefix: prefix.to_string(),
        ..Default::default()
    };
    let cfg_b = ZenohConfig {
        mode: ZenohMode::Peer,
        connect_endpoints: vec![listen_addr],
        multicast_scouting: false,
        key_prefix: prefix.to_string(),
        ..Default::default()
    };

    let session_a = zenoh::open(cfg_a.to_zenoh_config().ok()?).await.ok()?;
    let session_b = zenoh::open(cfg_b.to_zenoh_config().ok()?).await.ok()?;
    Some((Arc::new(session_a), Arc::new(session_b)))
}

/// Open a single peer session (for tests that only need one session).
async fn single_peer_session(prefix: &str) -> Option<Arc<zenoh::Session>> {
    let port = free_port();
    let cfg = ZenohConfig {
        mode: ZenohMode::Peer,
        listen_endpoints: vec![format!("tcp/127.0.0.1:{port}")],
        multicast_scouting: false,
        key_prefix: prefix.to_string(),
        ..Default::default()
    };
    zenoh::open(cfg.to_zenoh_config().ok()?).await.ok().map(Arc::new)
}

fn pdf_descriptor(worker_id: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        extension_id: worker_id.to_string(),
        protocol_version: PROTOCOL_VERSION.to_string(),
        capabilities: vec![ExtensionCapability::LoadDocument],
        content_types: vec!["application/pdf".to_string()],
        max_payload_bytes: 50 * 1024 * 1024,
        supports_streaming: false,
    }
}

fn embedder_descriptor(worker_id: &str) -> CapabilityDescriptor {
    CapabilityDescriptor {
        extension_id: worker_id.to_string(),
        protocol_version: PROTOCOL_VERSION.to_string(),
        capabilities: vec![ExtensionCapability::EmbedTexts],
        content_types: vec![],
        max_payload_bytes: 1 * 1024 * 1024,
        supports_streaming: false,
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn announce_registers_worker_in_registry() {
    let prefix = test_prefix();

    let Some((runtime_session, worker_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping announce_registers_worker_in_registry: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        ..Default::default()
    });

    let registry = ExtensionRegistry::start(runtime_session.clone(), config.clone())
        .await
        .expect("registry start");

    // Give subscriber time to establish before publishing.
    tokio::time::sleep(Duration::from_millis(200)).await;

    // Worker publishes its announce.
    let desc = pdf_descriptor("python.pdf_loader");
    let payload = serde_json::to_vec(&desc).unwrap();
    worker_session
        .put(&config.announce_key("python.pdf_loader"), payload)
        .await
        .expect("put announce");

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(registry.worker_count(), 1);
    let workers = registry.list_workers();
    assert_eq!(workers[0].extension_id, "python.pdf_loader");
    assert!(registry.find_loader_for("application/pdf").is_some());
    assert!(registry.find_loader_for("application/msword").is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn watchdog_evicts_worker_after_missed_heartbeats() {
    let prefix = test_prefix();

    let Some(session) = single_peer_session(&prefix).await else {
        eprintln!("skipping watchdog_evicts: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        heartbeat_interval_secs: 1,
        max_missed_heartbeats: 2,
        ..Default::default()
    });

    let registry = ExtensionRegistry::start(session.clone(), config.clone())
        .await
        .expect("registry start");

    // Register directly (simulates a recent announce + heartbeat).
    registry.register(pdf_descriptor("test.worker"));
    assert_eq!(registry.worker_count(), 1);

    // Wait for 3× the heartbeat interval (watchdog ticks every 1 s, evicts after 2 s).
    tokio::time::sleep(Duration::from_millis(3500)).await;

    assert_eq!(
        registry.worker_count(),
        0,
        "worker should have been evicted after missing heartbeats"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn heartbeat_keeps_worker_alive() {
    let prefix = test_prefix();

    let Some((runtime_session, worker_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping heartbeat_keeps_alive: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        heartbeat_interval_secs: 1,
        max_missed_heartbeats: 2,
        ..Default::default()
    });

    let registry = ExtensionRegistry::start(runtime_session.clone(), config.clone())
        .await
        .expect("registry start");

    // Register, then subscribe to heartbeat channel.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let desc = embedder_descriptor("rust.embedder");
    let announce_payload = serde_json::to_vec(&desc).unwrap();
    worker_session
        .put(&config.announce_key("rust.embedder"), announce_payload)
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(registry.worker_count(), 1);

    // Send two heartbeats 1 s apart; worker should survive past the 2 s eviction window.
    for _ in 0..2 {
        let hb = Heartbeat {
            extension_id: "rust.embedder".to_string(),
            timestamp: chrono::Utc::now(),
            status: WorkerStatus::Ready,
        };
        let payload = serde_json::to_vec(&hb).unwrap();
        worker_session
            .put(&config.heartbeat_key("rust.embedder"), payload)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(900)).await;
    }

    // After 2 heartbeats at ~0.9 s intervals (1.8 s total), worker still alive.
    assert_eq!(
        registry.worker_count(),
        1,
        "worker should still be alive after regular heartbeats"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn call_load_document_round_trip() {
    let prefix = test_prefix();

    let Some((runtime_session, worker_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping call_load_document: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        call_timeout_secs: 5,
        ..Default::default()
    });

    // Simulate a worker: declare a queryable on the load key.
    let load_key = config.call_key("test.pdf_loader", "load");
    let queryable = worker_session
        .declare_queryable(&load_key)
        .await
        .expect("declare queryable");

    // Serve one request in the background.
    let qbl_key = load_key.clone();
    tokio::spawn(async move {
        if let Ok(query) = queryable.recv_async().await {
            let response = LoadDocumentResponse {
                text: "Hello from PDF".to_string(),
                sections: vec![],
                page_count: Some(1),
                metadata: serde_json::Value::Null,
            };
            let env = ResponseEnvelope::ok("req-1", serde_json::to_value(&response).unwrap());
            let bytes = serde_json::to_vec(&env).unwrap();
            let _ = query.reply(&qbl_key, bytes).await;
        }
    });

    // Give the queryable time to register.
    tokio::time::sleep(Duration::from_millis(200)).await;

    let caller = ZenohCaller::new(runtime_session, config);
    let result = caller
        .load_document(
            "test.pdf_loader",
            rag_extension_protocol::LoadDocumentRequest {
                content_type: "application/pdf".to_string(),
                data_base64: "JVBER".to_string(),
                filename: Some("test.pdf".to_string()),
                metadata: serde_json::Value::Null,
            },
        )
        .await
        .expect("load_document call");

    assert_eq!(result.text, "Hello from PDF");
    assert_eq!(result.page_count, Some(1));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn call_embed_texts_round_trip() {
    let prefix = test_prefix();

    let Some((runtime_session, worker_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping call_embed_texts: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        call_timeout_secs: 5,
        ..Default::default()
    });

    let embed_key = config.call_key("test.embedder", "embed");
    let queryable = worker_session
        .declare_queryable(&embed_key)
        .await
        .expect("declare queryable");

    let qbl_key = embed_key.clone();
    tokio::spawn(async move {
        if let Ok(query) = queryable.recv_async().await {
            let response = EmbedTextsResponse {
                embeddings: vec![vec![0.1, 0.2, 0.3], vec![0.4, 0.5, 0.6]],
                dimension: 3,
            };
            let env = ResponseEnvelope::ok("req-2", serde_json::to_value(&response).unwrap());
            let bytes = serde_json::to_vec(&env).unwrap();
            let _ = query.reply(&qbl_key, bytes).await;
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let caller = ZenohCaller::new(runtime_session, config);
    let result = caller
        .embed_texts(
            "test.embedder",
            rag_extension_protocol::EmbedTextsRequest {
                texts: vec!["hello".to_string(), "world".to_string()],
            },
        )
        .await
        .expect("embed_texts call");

    assert_eq!(result.dimension, 3);
    assert_eq!(result.embeddings.len(), 2);
    assert!((result.embeddings[0][0] - 0.1).abs() < 1e-5);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn events_are_published_and_received() {
    let prefix = test_prefix();

    let Some((publisher_session, subscriber_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping events_published: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        ..Default::default()
    });

    // Subscribe to all events under our test prefix.
    let wildcard = format!("{}/events/**", prefix);
    let sub = subscriber_session
        .declare_subscriber(&wildcard)
        .await
        .expect("declare subscriber");

    tokio::time::sleep(Duration::from_millis(200)).await;

    let publisher = EventPublisher::new(publisher_session, config);
    publisher.index_started("sp-finance").await.expect("publish");
    publisher.document_indexed("sp-finance", "doc-1").await.expect("publish");
    publisher.index_failed("sp-finance", "timeout").await.expect("publish");

    // Collect 3 events with a generous timeout.
    let mut received = vec![];
    for _ in 0..3 {
        let timeout = tokio::time::timeout(Duration::from_secs(2), sub.recv_async()).await;
        match timeout {
            Ok(Ok(sample)) => {
                let key = sample.key_expr().to_string();
                received.push(key);
            }
            _ => break,
        }
    }

    assert_eq!(received.len(), 3, "expected 3 events, got {}: {:?}", received.len(), received);
    assert!(received.iter().any(|k| k.contains("index_started")));
    assert!(received.iter().any(|k| k.contains("document_indexed")));
    assert!(received.iter().any(|k| k.contains("index_failed")));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn rerank_call_round_trip() {
    let prefix = test_prefix();

    let Some((runtime_session, worker_session)) = make_peer_pair(&prefix).await else {
        eprintln!("skipping call_rerank: zenoh unavailable");
        return;
    };

    let config = Arc::new(ZenohConfig {
        mode: ZenohMode::Peer,
        multicast_scouting: false,
        key_prefix: prefix.clone(),
        call_timeout_secs: 5,
        ..Default::default()
    });

    let rerank_key = config.call_key("test.reranker", "rerank");
    let queryable = worker_session
        .declare_queryable(&rerank_key)
        .await
        .expect("declare queryable");

    let qbl_key = rerank_key.clone();
    tokio::spawn(async move {
        if let Ok(query) = queryable.recv_async().await {
            let response = RerankResponse {
                ranked: vec![
                    RankedChunk { chunk_id: "c2".to_string(), score: 0.95 },
                    RankedChunk { chunk_id: "c1".to_string(), score: 0.42 },
                ],
            };
            let env = ResponseEnvelope::ok("req-3", serde_json::to_value(&response).unwrap());
            let bytes = serde_json::to_vec(&env).unwrap();
            let _ = query.reply(&qbl_key, bytes).await;
        }
    });

    tokio::time::sleep(Duration::from_millis(200)).await;

    let caller = ZenohCaller::new(runtime_session, config);
    let result = caller
        .rerank(
            "test.reranker",
            rag_extension_protocol::RerankRequest {
                query: "rust async runtime".to_string(),
                candidates: vec![
                    rag_extension_protocol::RerankCandidate {
                        chunk_id: "c1".to_string(),
                        text: "tokio is an async runtime".to_string(),
                    },
                    rag_extension_protocol::RerankCandidate {
                        chunk_id: "c2".to_string(),
                        text: "async rust with zenoh".to_string(),
                    },
                ],
            },
        )
        .await
        .expect("rerank call");

    assert_eq!(result.ranked.len(), 2);
    assert_eq!(result.ranked[0].chunk_id, "c2");
    assert!((result.ranked[0].score - 0.95).abs() < 1e-5);
}
