"""
Base classes for rag extension workers.

Subclass one of the base workers, implement the abstract method, then call
``worker.run()`` from your script.  The base class handles all Zenoh
connection management, announce/heartbeat lifecycle, and protocol framing.

Example (document loader)::

    from rag_worker_sdk.worker import DocumentLoaderWorker
    from rag_worker_sdk.protocol import (
        LoadDocumentRequest, LoadDocumentResponse, DocumentSection,
    )

    class PdfLoaderWorker(DocumentLoaderWorker):
        CONTENT_TYPES = ["application/pdf"]

        def load(self, req: LoadDocumentRequest) -> LoadDocumentResponse:
            import base64, pymupdf
            data = base64.b64decode(req.data_base64)
            doc = pymupdf.open(stream=data, filetype="pdf")
            sections = []
            full_text = []
            for i, page in enumerate(doc):
                text = page.get_text()
                sections.append(DocumentSection(text=text, page=i + 1))
                full_text.append(text)
            return LoadDocumentResponse(
                text="\\n\\n".join(full_text),
                sections=sections,
                page_count=len(doc),
            )

    if __name__ == "__main__":
        from rag_worker_sdk.config import WorkerConfig
        config = WorkerConfig.from_env("python.pdf_loader")
        PdfLoaderWorker(config).run()
"""

from __future__ import annotations

import abc
import json
import logging
import signal
import threading
import time
from typing import Optional

import zenoh

from .config import WorkerConfig
from .protocol import (
    CapabilityDescriptor,
    EmbedTextsRequest,
    EmbedTextsResponse,
    ExtensionCapability,
    Heartbeat,
    LoadDocumentRequest,
    LoadDocumentResponse,
    PROTOCOL_VERSION,
    RerankCandidate,
    RerankRequest,
    RerankResponse,
    RequestEnvelope,
    ResponseEnvelope,
    WorkerStatus,
)

logger = logging.getLogger(__name__)


class _BaseWorker(abc.ABC):
    """Internal base class shared by all worker types."""

    #: Capabilities this worker advertises. Subclasses set this.
    CAPABILITIES: list[ExtensionCapability] = []
    #: MIME types this worker handles. Empty = all types.
    CONTENT_TYPES: list[str] = []
    #: Maximum raw payload size in bytes.
    MAX_PAYLOAD_BYTES: int = 100 * 1024 * 1024  # 100 MiB

    def __init__(self, config: WorkerConfig) -> None:
        self.config = config
        self._session: Optional[zenoh.Session] = None
        self._stop_event = threading.Event()

    # ── Lifecycle ─────────────────────────────────────────────────────────────

    def run(self) -> None:
        """Connect to Zenoh, register, serve requests, and block until stopped."""
        # Graceful shutdown on SIGTERM / Ctrl-C
        for sig in (signal.SIGTERM, signal.SIGINT):
            signal.signal(sig, lambda *_: self.stop())

        logger.info("worker %s starting", self.config.worker_id)

        cfg_dict = self.config.zenoh_config_dict()
        z_cfg = zenoh.Config.from_obj(cfg_dict)
        self._session = zenoh.open(z_cfg)

        try:
            self._announce()
            self._start_heartbeat_thread()
            self._serve()
        finally:
            logger.info("worker %s shutting down", self.config.worker_id)
            if self._session:
                self._session.close()

    def stop(self) -> None:
        """Signal the worker to stop serving and exit ``run()``."""
        self._stop_event.set()

    # ── Announce & heartbeat ──────────────────────────────────────────────────

    def _descriptor(self) -> CapabilityDescriptor:
        return CapabilityDescriptor(
            extension_id=self.config.worker_id,
            protocol_version=PROTOCOL_VERSION,
            capabilities=self.CAPABILITIES,
            content_types=self.CONTENT_TYPES,
            max_payload_bytes=self.MAX_PAYLOAD_BYTES,
            supports_streaming=False,
        )

    def _announce(self) -> None:
        key = f"{self.config.key_prefix}/extensions/{self.config.worker_id}/announce"
        payload = self._descriptor().to_json()
        self._session.put(key, payload)
        logger.info("announced capabilities on %s", key)

    def _start_heartbeat_thread(self) -> None:
        def _beat():
            hb_key = f"{self.config.key_prefix}/extensions/{self.config.worker_id}/heartbeat"
            while not self._stop_event.is_set():
                hb = Heartbeat.now(self.config.worker_id, WorkerStatus.READY)
                self._session.put(hb_key, hb.to_json())
                self._stop_event.wait(self.config.heartbeat_interval_secs)

        t = threading.Thread(target=_beat, daemon=True, name="rag-heartbeat")
        t.start()

    # ── Request dispatch ──────────────────────────────────────────────────────

    def _serve(self) -> None:
        """Declare queryables and serve requests until stopped."""
        queryables = self._register_queryables()
        logger.info("worker %s ready", self.config.worker_id)
        # Block until stop() is called.
        self._stop_event.wait()
        for q in queryables:
            q.undeclare()

    def _register_queryables(self) -> list:
        raise NotImplementedError

    # ── Helper ────────────────────────────────────────────────────────────────

    def _dispatch(self, query) -> None:
        try:
            payload = bytes(query.payload or b"")
            envelope = RequestEnvelope.from_bytes(payload)
            response_payload = self._handle(envelope)
            env = ResponseEnvelope.ok(envelope.request_id, response_payload)
        except Exception as exc:
            logger.exception("error handling request")
            request_id = "unknown"
            try:
                request_id = RequestEnvelope.from_bytes(bytes(query.payload or b"")).request_id
            except Exception:
                pass
            env = ResponseEnvelope.err(request_id, str(exc))

        query.reply(query.key_expr, env.to_json())

    @abc.abstractmethod
    def _handle(self, envelope: RequestEnvelope) -> dict:
        ...


# ── Document loader worker ────────────────────────────────────────────────────


class DocumentLoaderWorker(_BaseWorker):
    """Base class for workers that parse binary documents (PDF, DOCX, etc.)."""

    CAPABILITIES = [ExtensionCapability.LOAD_DOCUMENT]

    @abc.abstractmethod
    def load(self, request: LoadDocumentRequest) -> LoadDocumentResponse:
        """Parse the document and return extracted text + sections."""
        ...

    def _register_queryables(self) -> list:
        key = f"{self.config.key_prefix}/call/{self.config.worker_id}/load"
        q = self._session.declare_queryable(key)
        logger.info("serving load_document on %s", key)

        def _serve_loop():
            for query in q:
                if self._stop_event.is_set():
                    break
                self._dispatch(query)

        t = threading.Thread(target=_serve_loop, daemon=True, name="rag-load")
        t.start()
        return [q]

    def _handle(self, envelope: RequestEnvelope) -> dict:
        req_data = envelope.payload
        req = LoadDocumentRequest(
            content_type=req_data["content_type"],
            data_base64=req_data["data_base64"],
            filename=req_data.get("filename"),
            metadata=req_data.get("metadata"),
        )
        resp = self.load(req)
        return resp.to_dict()


# ── Embedder worker ───────────────────────────────────────────────────────────


class EmbedderWorker(_BaseWorker):
    """Base class for workers that embed texts into dense vectors."""

    CAPABILITIES = [ExtensionCapability.EMBED_TEXTS]

    @abc.abstractmethod
    def embed(self, request: EmbedTextsRequest) -> EmbedTextsResponse:
        """Embed a batch of texts and return the dense vectors."""
        ...

    def _register_queryables(self) -> list:
        key = f"{self.config.key_prefix}/call/{self.config.worker_id}/embed"
        q = self._session.declare_queryable(key)
        logger.info("serving embed_texts on %s", key)

        def _serve_loop():
            for query in q:
                if self._stop_event.is_set():
                    break
                self._dispatch(query)

        t = threading.Thread(target=_serve_loop, daemon=True, name="rag-embed")
        t.start()
        return [q]

    def _handle(self, envelope: RequestEnvelope) -> dict:
        req = EmbedTextsRequest(texts=envelope.payload["texts"])
        resp = self.embed(req)
        return resp.to_dict()


# ── Reranker worker ───────────────────────────────────────────────────────────


class RerankerWorker(_BaseWorker):
    """Base class for workers that rerank candidate chunks."""

    CAPABILITIES = [ExtensionCapability.RERANK]

    @abc.abstractmethod
    def rerank(self, request: RerankRequest) -> RerankResponse:
        """Reorder candidates by relevance to the query."""
        ...

    def _register_queryables(self) -> list:
        key = f"{self.config.key_prefix}/call/{self.config.worker_id}/rerank"
        q = self._session.declare_queryable(key)
        logger.info("serving rerank on %s", key)

        def _serve_loop():
            for query in q:
                if self._stop_event.is_set():
                    break
                self._dispatch(query)

        t = threading.Thread(target=_serve_loop, daemon=True, name="rag-rerank")
        t.start()
        return [q]

    def _handle(self, envelope: RequestEnvelope) -> dict:
        candidates = [
            RerankCandidate(chunk_id=c["chunk_id"], text=c["text"])
            for c in envelope.payload["candidates"]
        ]
        req = RerankRequest(query=envelope.payload["query"], candidates=candidates)
        resp = self.rerank(req)
        return resp.to_dict()
