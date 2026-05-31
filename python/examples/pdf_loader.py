"""
PDF document loader extension worker.

Parses PDF files using PyMuPDF (pymupdf) and returns extracted text with
per-page sections. Register this worker with the rag-zenoh bus and the Rust
runtime will route ``application/pdf`` load_document requests to it.

Install dependencies:
    pip install "rag-worker-sdk[pdf]" eclipse-zenoh

Run:
    python pdf_loader.py
    # or with a router:
    RAG_ZENOH_MODE=client RAG_ZENOH_ENDPOINTS=tcp/router:7447 python pdf_loader.py
"""

import base64
import logging
import sys

from rag_worker_sdk import DocumentLoaderWorker, WorkerConfig
from rag_worker_sdk.protocol import DocumentSection, LoadDocumentRequest, LoadDocumentResponse

logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s %(message)s")
logger = logging.getLogger("pdf_loader")


class PdfLoaderWorker(DocumentLoaderWorker):
    CONTENT_TYPES = ["application/pdf"]

    def load(self, req: LoadDocumentRequest) -> LoadDocumentResponse:
        try:
            import pymupdf  # type: ignore
        except ImportError:
            raise RuntimeError(
                "pymupdf is required: pip install pymupdf"
            )

        raw_bytes = base64.b64decode(req.data_base64)
        doc = pymupdf.open(stream=raw_bytes, filetype="pdf")

        sections: list[DocumentSection] = []
        page_texts: list[str] = []

        for page_num, page in enumerate(doc, start=1):
            text = page.get_text().strip()
            if not text:
                continue
            sections.append(DocumentSection(text=text, page=page_num))
            page_texts.append(text)

        full_text = "\n\n".join(page_texts)
        page_count = len(doc)
        doc.close()

        logger.info(
            "parsed pdf: filename=%s pages=%d chars=%d",
            req.filename or "(unknown)",
            page_count,
            len(full_text),
        )

        return LoadDocumentResponse(
            text=full_text,
            sections=sections,
            page_count=page_count,
            metadata={"filename": req.filename, "source_content_type": req.content_type},
        )


if __name__ == "__main__":
    worker_id = sys.argv[1] if len(sys.argv) > 1 else "python.pdf_loader"
    config = WorkerConfig.from_env(worker_id)
    logger.info("starting PDF loader worker: %s", worker_id)
    PdfLoaderWorker(config).run()
