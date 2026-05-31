//! Source connectors for rag-core.
//!
//! Each connector implements `rag_core::traits::Connector` and is gated by a
//! Cargo feature so the binary can opt in only to what it needs.

#[cfg(feature = "sharepoint")]
pub mod sharepoint;
