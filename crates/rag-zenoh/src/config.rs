//! Zenoh connection configuration and TLS/mTLS support.

use anyhow::anyhow;
use rag_core::RagError;

// ── Public config types ───────────────────────────────────────────────────────

/// Top-level configuration for the Zenoh extension bus.
#[derive(Debug, Clone)]
pub struct ZenohConfig {
    /// Zenoh session mode.
    pub mode: ZenohMode,
    /// Endpoints to connect to (e.g. `"tcp/router.internal:7447"`).
    /// Empty means peer auto-discovery via multicast scouting.
    pub connect_endpoints: Vec<String>,
    /// Endpoints for this session to listen on (peer mode only).
    /// Useful for tests and for running without a dedicated router when
    /// other peers connect explicitly rather than via multicast scouting.
    /// Example: `vec!["tcp/127.0.0.1:17001".to_string()]`.
    pub listen_endpoints: Vec<String>,
    /// Enable UDP multicast scouting for peer discovery.
    /// Set to `false` when using explicit `connect_endpoints` / `listen_endpoints`
    /// to avoid network dependency (e.g. in CI environments without multicast).
    pub multicast_scouting: bool,
    /// mTLS configuration. `None` means plaintext TCP.
    pub tls: Option<TlsConfig>,
    /// How often workers are expected to send heartbeats, in seconds.
    pub heartbeat_interval_secs: u64,
    /// Number of consecutive missed heartbeats before a worker is evicted.
    pub max_missed_heartbeats: u32,
    /// Timeout for a single request/reply call, in seconds.
    pub call_timeout_secs: u64,
    /// Key prefix for all rag-zenoh topics. Default: `"rag"`.
    /// Override in tests to avoid cross-test interference.
    pub key_prefix: String,
}

impl Default for ZenohConfig {
    fn default() -> Self {
        Self {
            mode: ZenohMode::Peer,
            connect_endpoints: vec![],
            listen_endpoints: vec![],
            multicast_scouting: true,
            tls: None,
            heartbeat_interval_secs: 15,
            max_missed_heartbeats: 3,
            call_timeout_secs: 30,
            key_prefix: "rag".to_string(),
        }
    }
}

/// Zenoh session mode.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum ZenohMode {
    /// Decentralised peer-to-peer. Workers discover each other via multicast
    /// scouting or explicit connect endpoints. No external router required.
    #[default]
    Peer,
    /// Client that connects to a Zenoh router. Required in restricted network
    /// environments where multicast is unavailable.
    Client,
}

/// mTLS credentials for Zenoh transport security.
///
/// All paths are to PEM-format files. The same CA certificate is used both for
/// verifying the remote peer and for issuing the local client certificate.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    /// Path to the CA certificate PEM file (used to verify the server).
    pub ca_certificate_path: String,
    /// Path to the client certificate PEM file.
    pub client_certificate_path: String,
    /// Path to the client private key PEM file.
    pub client_private_key_path: String,
    /// Whether to verify the server hostname against the certificate CN/SAN.
    /// Always `true` in production; may be `false` on internal networks with
    /// wildcard or IP-SAN certificates.
    pub server_name_verification: bool,
}

// ── Config builder ────────────────────────────────────────────────────────────

impl ZenohConfig {
    /// Build a [`zenoh::Config`] from this struct.
    pub fn to_zenoh_config(&self) -> Result<zenoh::Config, RagError> {
        let mode_str = match self.mode {
            ZenohMode::Peer => "peer",
            ZenohMode::Client => "client",
        };

        // Build a JSON object that zenoh will deserialise into its Config type.
        let mut obj = serde_json::json!({ "mode": mode_str });

        if !self.connect_endpoints.is_empty() {
            obj["connect"] = serde_json::json!({ "endpoints": self.connect_endpoints });
        }

        if !self.listen_endpoints.is_empty() {
            obj["listen"] = serde_json::json!({ "endpoints": self.listen_endpoints });
        }

        if !self.multicast_scouting {
            obj["scouting"] = serde_json::json!({ "multicast": { "enabled": false } });
        }

        if let Some(tls) = &self.tls {
            obj["transport"] = serde_json::json!({
                "unicast": {
                    "tls": {
                        "client_auth": true,
                        "server_name_verification": tls.server_name_verification,
                        "root_ca_certificate": tls.ca_certificate_path,
                        "client_certificate": tls.client_certificate_path,
                        "client_private_key": tls.client_private_key_path
                    }
                }
            });
        }

        let json_str = serde_json::to_string(&obj)
            .map_err(|e| RagError::Other(anyhow!("serialize zenoh config: {e}")))?;

        zenoh::Config::from_json5(&json_str)
            .map_err(|e| RagError::Other(anyhow!("invalid zenoh config: {e}")))
    }

    // ── Convenience key-expression builders ──────────────────────────────────

    pub fn announce_key(&self, worker_id: &str) -> String {
        format!("{}/extensions/{}/announce", self.key_prefix, worker_id)
    }

    pub fn announce_wildcard(&self) -> String {
        format!("{}/extensions/*/announce", self.key_prefix)
    }

    pub fn heartbeat_key(&self, worker_id: &str) -> String {
        format!("{}/extensions/{}/heartbeat", self.key_prefix, worker_id)
    }

    pub fn heartbeat_wildcard(&self) -> String {
        format!("{}/extensions/*/heartbeat", self.key_prefix)
    }

    pub fn call_key(&self, worker_id: &str, op: &str) -> String {
        format!("{}/call/{}/{}", self.key_prefix, worker_id, op)
    }

    pub fn event_key(&self, event_name: &str) -> String {
        format!("{}/events/{}", self.key_prefix, event_name)
    }
}

// ── Unit tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_config_has_expected_values() {
        let c = ZenohConfig::default();
        assert_eq!(c.key_prefix, "rag");
        assert_eq!(c.heartbeat_interval_secs, 15);
        assert_eq!(c.max_missed_heartbeats, 3);
        assert_eq!(c.call_timeout_secs, 30);
        assert!(c.connect_endpoints.is_empty());
        assert!(c.tls.is_none());
    }

    #[test]
    fn key_builders_use_prefix() {
        let mut c = ZenohConfig::default();
        c.key_prefix = "test/abc".to_string();
        assert_eq!(c.announce_key("w1"), "test/abc/extensions/w1/announce");
        assert_eq!(c.announce_wildcard(), "test/abc/extensions/*/announce");
        assert_eq!(c.heartbeat_key("w1"), "test/abc/extensions/w1/heartbeat");
        assert_eq!(c.call_key("w1", "load"), "test/abc/call/w1/load");
        assert_eq!(c.event_key("index_started"), "test/abc/events/index_started");
    }

    #[test]
    fn to_zenoh_config_peer_mode() {
        let config = ZenohConfig::default();
        // Should not error — the JSON mode value is valid.
        let result = config.to_zenoh_config();
        assert!(result.is_ok(), "expected Ok, got: {:?}", result.err());
    }

    #[test]
    fn to_zenoh_config_with_endpoints() {
        let config = ZenohConfig {
            mode: ZenohMode::Client,
            connect_endpoints: vec!["tcp/router.example.com:7447".to_string()],
            ..Default::default()
        };
        let result = config.to_zenoh_config();
        assert!(result.is_ok(), "{:?}", result.err());
    }
}
