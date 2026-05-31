"""
Protocol types matching rag-extension-protocol (rag.extension.v1).

These dataclasses mirror the Rust structs in crates/rag-extension-protocol/src/lib.rs.
Any change to that file should be reflected here.
"""

from __future__ import annotations

import dataclasses
import json
import uuid
from datetime import datetime, timezone
from enum import Enum
from typing import Any, Optional


PROTOCOL_VERSION = "rag.extension.v1"


# ── Capability ────────────────────────────────────────────────────────────────


class ExtensionCapability(str, Enum):
    LOAD_DOCUMENT = "load_document"
    EMBED_TEXTS = "embed_texts"
    RERANK = "rerank"
    APPLY_ACL = "apply_acl"
    SUMMARIZE_CONTEXT = "summarize_context"
    TRANSFORM = "transform"


class WorkerStatus(str, Enum):
    READY = "ready"
    BUSY = "busy"
    DRAINING = "draining"


@dataclasses.dataclass
class CapabilityDescriptor:
    extension_id: str
    protocol_version: str
    capabilities: list[ExtensionCapability]
    content_types: list[str]
    max_payload_bytes: int
    supports_streaming: bool

    def to_json(self) -> bytes:
        return json.dumps(dataclasses.asdict(self)).encode()


@dataclasses.dataclass
class Heartbeat:
    extension_id: str
    timestamp: str  # ISO 8601
    status: WorkerStatus

    def to_json(self) -> bytes:
        return json.dumps(dataclasses.asdict(self)).encode()

    @classmethod
    def now(cls, extension_id: str, status: WorkerStatus = WorkerStatus.READY) -> "Heartbeat":
        return cls(
            extension_id=extension_id,
            timestamp=datetime.now(timezone.utc).isoformat(),
            status=status,
        )


# ── Envelopes ─────────────────────────────────────────────────────────────────


@dataclasses.dataclass
class RequestContext:
    tenant_id: Optional[str] = None
    user_id: Optional[str] = None
    trace_id: Optional[str] = None


@dataclasses.dataclass
class RequestEnvelope:
    protocol: str
    request_id: str
    operation: ExtensionCapability
    payload: Any
    context: RequestContext

    @classmethod
    def from_bytes(cls, data: bytes) -> "RequestEnvelope":
        d = json.loads(data)
        return cls(
            protocol=d["protocol"],
            request_id=d["request_id"],
            operation=ExtensionCapability(d["operation"]),
            payload=d["payload"],
            context=RequestContext(**d.get("context", {})),
        )


@dataclasses.dataclass
class ResponseEnvelope:
    request_id: str
    success: bool
    payload: Any
    error: Optional[str] = None

    def to_json(self) -> bytes:
        return json.dumps(dataclasses.asdict(self)).encode()

    @classmethod
    def ok(cls, request_id: str, payload: Any) -> "ResponseEnvelope":
        return cls(request_id=request_id, success=True, payload=payload)

    @classmethod
    def err(cls, request_id: str, message: str) -> "ResponseEnvelope":
        return cls(request_id=request_id, success=False, payload=None, error=message)


# ── Typed payloads ────────────────────────────────────────────────────────────


@dataclasses.dataclass
class LoadDocumentRequest:
    content_type: str
    data_base64: str
    filename: Optional[str]
    metadata: Any


@dataclasses.dataclass
class DocumentSection:
    text: str
    title: Optional[str] = None
    page: Optional[int] = None


@dataclasses.dataclass
class LoadDocumentResponse:
    text: str
    sections: list[DocumentSection]
    page_count: Optional[int] = None
    metadata: Any = None

    def to_dict(self) -> dict:
        return {
            "text": self.text,
            "sections": [dataclasses.asdict(s) for s in self.sections],
            "page_count": self.page_count,
            "metadata": self.metadata,
        }


@dataclasses.dataclass
class EmbedTextsRequest:
    texts: list[str]


@dataclasses.dataclass
class EmbedTextsResponse:
    embeddings: list[list[float]]
    dimension: int

    def to_dict(self) -> dict:
        return dataclasses.asdict(self)


@dataclasses.dataclass
class RerankCandidate:
    chunk_id: str
    text: str


@dataclasses.dataclass
class RerankRequest:
    query: str
    candidates: list[RerankCandidate]


@dataclasses.dataclass
class RankedChunk:
    chunk_id: str
    score: float


@dataclasses.dataclass
class RerankResponse:
    ranked: list[RankedChunk]

    def to_dict(self) -> dict:
        return {"ranked": [dataclasses.asdict(c) for c in self.ranked]}
