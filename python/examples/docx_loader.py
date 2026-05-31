"""
DOCX document loader extension worker.

Parses Microsoft Word (.docx) files using python-docx and returns extracted
text organised by paragraph and heading sections.

Install dependencies:
    pip install "rag-worker-sdk[docx]" eclipse-zenoh

Run:
    python docx_loader.py
"""

import base64
import io
import logging
import sys

from rag_worker_sdk import DocumentLoaderWorker, WorkerConfig
from rag_worker_sdk.protocol import DocumentSection, LoadDocumentRequest, LoadDocumentResponse

logging.basicConfig(level=logging.INFO, format="%(levelname)s %(name)s %(message)s")
logger = logging.getLogger("docx_loader")


class DocxLoaderWorker(DocumentLoaderWorker):
    CONTENT_TYPES = [
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "application/msword",
    ]

    def load(self, req: LoadDocumentRequest) -> LoadDocumentResponse:
        try:
            from docx import Document  # type: ignore
        except ImportError:
            raise RuntimeError("python-docx is required: pip install python-docx")

        raw_bytes = base64.b64decode(req.data_base64)
        doc = Document(io.BytesIO(raw_bytes))

        sections: list[DocumentSection] = []
        current_heading: str | None = None
        current_paragraphs: list[str] = []
        full_text_parts: list[str] = []

        def flush_section() -> None:
            if current_paragraphs:
                section_text = "\n".join(current_paragraphs)
                sections.append(DocumentSection(text=section_text, title=current_heading))

        for para in doc.paragraphs:
            text = para.text.strip()
            if not text:
                continue

            full_text_parts.append(text)

            # python-docx style names: Heading 1, Heading 2, etc.
            if para.style.name.startswith("Heading"):
                flush_section()
                current_heading = text
                current_paragraphs = []
            else:
                current_paragraphs.append(text)

        flush_section()

        full_text = "\n\n".join(full_text_parts)

        logger.info(
            "parsed docx: filename=%s sections=%d chars=%d",
            req.filename or "(unknown)",
            len(sections),
            len(full_text),
        )

        return LoadDocumentResponse(
            text=full_text,
            sections=sections,
            page_count=None,
            metadata={"filename": req.filename, "source_content_type": req.content_type},
        )


if __name__ == "__main__":
    worker_id = sys.argv[1] if len(sys.argv) > 1 else "python.docx_loader"
    config = WorkerConfig.from_env(worker_id)
    logger.info("starting DOCX loader worker: %s", worker_id)
    DocxLoaderWorker(config).run()
