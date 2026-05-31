//! Source connectors for rag-core.
//!
//! Each connector implements `rag_core::traits::Connector` and is gated by a
//! Cargo feature so binaries only compile what they need.

#[cfg(feature = "sharepoint")]
pub mod sharepoint;

#[cfg(feature = "filesystem")]
pub mod filesystem;
