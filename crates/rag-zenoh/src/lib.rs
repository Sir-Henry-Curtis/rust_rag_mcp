//! Zenoh transport for rag-core external extension workers.
//!
//! Phase 4 work:
//!   - Add zenoh dependency.
//!   - Implement `ExtensionRegistry`: discover workers via announce/heartbeat keys.
//!   - Implement request/reply for `load_document`, `embed_texts`, `rerank`.
//!   - Implement capability lookup: given a content-type, find the right worker.
//!   - Implement heartbeat watchdog: evict workers that miss N consecutive beats.
//!
//! ## Keyspace
//! ```text
//! rag/extensions/{id}/announce   → CapabilityDescriptor (subscriber)
//! rag/extensions/{id}/heartbeat  → Heartbeat (subscriber)
//! rag/call/{id}/load             → RequestEnvelope / ResponseEnvelope (queryable)
//! rag/call/{id}/embed            → RequestEnvelope / ResponseEnvelope (queryable)
//! rag/call/{id}/rerank           → RequestEnvelope / ResponseEnvelope (queryable)
//! rag/events/**                  → indexing lifecycle events (publisher)
//! ```

pub struct ZenohExtensionRegistry;
