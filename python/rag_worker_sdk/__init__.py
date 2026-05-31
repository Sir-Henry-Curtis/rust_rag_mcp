"""
rag_worker_sdk — Python SDK for rag-zenoh extension workers.

Subclass one of the base workers and implement the required method, then call
``worker.run()`` from your ``__main__`` block.

Quick start::

    from rag_worker_sdk import DocumentLoaderWorker, WorkerConfig
    from rag_worker_sdk.protocol import LoadDocumentRequest, LoadDocumentResponse

    class MyLoader(DocumentLoaderWorker):
        CONTENT_TYPES = ["application/pdf"]
        def load(self, req: LoadDocumentRequest) -> LoadDocumentResponse: ...

    MyLoader(WorkerConfig.from_env("my.loader")).run()
"""

from .config import WorkerConfig
from .protocol import (
    CapabilityDescriptor,
    DocumentSection,
    EmbedTextsRequest,
    EmbedTextsResponse,
    ExtensionCapability,
    Heartbeat,
    LoadDocumentRequest,
    LoadDocumentResponse,
    PROTOCOL_VERSION,
    RankedChunk,
    RerankCandidate,
    RerankRequest,
    RerankResponse,
    WorkerStatus,
)
from .worker import DocumentLoaderWorker, EmbedderWorker, RerankerWorker

__all__ = [
    "WorkerConfig",
    "DocumentLoaderWorker",
    "EmbedderWorker",
    "RerankerWorker",
    "CapabilityDescriptor",
    "DocumentSection",
    "EmbedTextsRequest",
    "EmbedTextsResponse",
    "ExtensionCapability",
    "Heartbeat",
    "LoadDocumentRequest",
    "LoadDocumentResponse",
    "PROTOCOL_VERSION",
    "RankedChunk",
    "RerankCandidate",
    "RerankRequest",
    "RerankResponse",
    "WorkerStatus",
]
