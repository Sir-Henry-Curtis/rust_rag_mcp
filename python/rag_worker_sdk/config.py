"""
Worker configuration loaded from environment variables or passed directly.
"""

from __future__ import annotations

import dataclasses
import os
from typing import Optional


@dataclasses.dataclass
class WorkerConfig:
    # ── Identity ──────────────────────────────────────────────────────────────
    worker_id: str
    """Unique ID for this worker instance, e.g. "python.pdf_loader"."""

    # ── Zenoh transport ───────────────────────────────────────────────────────
    zenoh_mode: str = "peer"
    """Zenoh mode: "peer" (default, no router required) or "client"."""

    connect_endpoints: list[str] = dataclasses.field(default_factory=list)
    """Router endpoints to connect to, e.g. ["tcp/router.example.com:7447"]."""

    key_prefix: str = "rag"
    """Key prefix for all rag Zenoh topics. Override in tests."""

    # ── Timing ────────────────────────────────────────────────────────────────
    heartbeat_interval_secs: float = 10.0
    """How often to send a heartbeat, in seconds."""

    # ── TLS ───────────────────────────────────────────────────────────────────
    tls_ca_certificate: Optional[str] = None
    """Path to CA certificate PEM file (enables TLS when set)."""

    tls_client_certificate: Optional[str] = None
    """Path to client certificate PEM file (required for mTLS)."""

    tls_client_private_key: Optional[str] = None
    """Path to client private key PEM file (required for mTLS)."""

    @classmethod
    def from_env(cls, worker_id: str) -> "WorkerConfig":
        """Load configuration from environment variables.

        RAG_ZENOH_MODE            - "peer" or "client"
        RAG_ZENOH_ENDPOINTS       - comma-separated list, e.g. "tcp/host:7447"
        RAG_ZENOH_PREFIX          - key prefix (default: "rag")
        RAG_HEARTBEAT_INTERVAL    - seconds (default: 10)
        RAG_TLS_CA_CERTIFICATE    - path to CA certificate PEM
        RAG_TLS_CLIENT_CERT       - path to client certificate PEM
        RAG_TLS_CLIENT_KEY        - path to client private key PEM
        """
        endpoints_raw = os.environ.get("RAG_ZENOH_ENDPOINTS", "")
        connect_endpoints = [e.strip() for e in endpoints_raw.split(",") if e.strip()]

        return cls(
            worker_id=worker_id,
            zenoh_mode=os.environ.get("RAG_ZENOH_MODE", "peer"),
            connect_endpoints=connect_endpoints,
            key_prefix=os.environ.get("RAG_ZENOH_PREFIX", "rag"),
            heartbeat_interval_secs=float(os.environ.get("RAG_HEARTBEAT_INTERVAL", "10")),
            tls_ca_certificate=os.environ.get("RAG_TLS_CA_CERTIFICATE"),
            tls_client_certificate=os.environ.get("RAG_TLS_CLIENT_CERT"),
            tls_client_private_key=os.environ.get("RAG_TLS_CLIENT_KEY"),
        )

    def zenoh_config_dict(self) -> dict:
        """Build a dict suitable for ``zenoh.Config.from_obj``."""
        cfg: dict = {"mode": self.zenoh_mode}

        if self.connect_endpoints:
            cfg["connect"] = {"endpoints": self.connect_endpoints}

        if self.tls_ca_certificate:
            cfg["transport"] = {
                "unicast": {
                    "tls": {
                        "client_auth": bool(self.tls_client_certificate),
                        "server_name_verification": True,
                        "root_ca_certificate": self.tls_ca_certificate,
                        **(
                            {
                                "client_certificate": self.tls_client_certificate,
                                "client_private_key": self.tls_client_private_key,
                            }
                            if self.tls_client_certificate
                            else {}
                        ),
                    }
                }
            }

        return cfg
