"""
PDF document loader extension worker.

Uses pdfplumber (MIT) for text and table extraction from PDF files.
pdfplumber is built on pdfminer.six (MIT), which gives it broad coverage
of PDF text encoding and a clean table-detection API.

**License note:** This file depends on pdfplumber (MIT) and pdfminer.six (MIT),
both Apache-2.0 compatible. The previously suggested pymupdf/PyMuPDF library
is AGPL-3.0 OR Artifex Commercial — incompatible with Apache-2.0 distribution.
If you need higher-fidelity extraction and hold an Artifex commercial license,
pymupdf can be substituted here, but do NOT distribute the resulting code as
Apache-2.0.

Install dependencies:
    pip install "rag-worker-sdk[pdf]" eclipse-zenoh

Run:
    python pdf_loader.py
    # or with a router:
    RAG_ZENOH_MODE=client RAG_ZENOH_ENDPOINTS=tcp/router:7447 python pdf_loader.py
"""

import base64
import io
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
            import pdfplumber  # type: ignore
        except ImportError:
            raise RuntimeError(
                "pdfplumber is required: pip install pdfplumber"
            )

        raw_bytes = base64.b64decode(req.data_base64)
        sections: list[DocumentSection] = []
        page_texts: list[str] = []

        with pdfplumber.open(io.BytesIO(raw_bytes)) as pdf:
            page_count = len(pdf.pages)

            for page_num, page in enumerate(pdf.pages, start=1):
                text = page.extract_text() or ""

                # Extract tables and append as pipe-formatted text so they
                # survive the plain-text chunking path.
                table_blocks: list[str] = []
                for table in page.extract_tables():
                    if not table:
                        continue
                    rows = [
                        " | ".join(str(cell or "").strip() for cell in row)
                        for row in table
                        if any(cell for cell in row)
                    ]
                    if rows:
                        table_blocks.append("\n".join(rows))

                if table_blocks:
                    joined = "\n\n".join(table_blocks)
                    text = f"{text}\n\n{joined}" if text.strip() else joined

                if not text.strip():
                    continue

                sections.append(DocumentSection(text=text.strip(), page=page_num))
                page_texts.append(text.strip())

        full_text = "\n\n".join(page_texts)

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
