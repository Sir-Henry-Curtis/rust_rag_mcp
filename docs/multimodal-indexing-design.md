# Multi-modal Indexing Design

Multi-modal indexing extends `rust-rag-mcp` beyond plain extracted text. The
goal is to make diagrams, charts, screenshots, scanned pages, and other visual
content retrievable alongside normal document text.

This should be implemented as an extension of the existing RAG pipeline, not as
a separate search product. Text remains the primary retrieval unit, but visual
evidence becomes searchable through generated descriptions, OCR text, layout
metadata, and optional image embeddings.

---

## Problem

Many enterprise PDFs and slide decks contain important information that is not
available as ordinary text:

- architecture diagrams
- flowcharts
- org charts
- screenshots
- tables rendered as images
- scanned documents
- charts with legends and axis labels
- photos embedded in reports
- annotated engineering drawings

A text-only loader may extract surrounding paragraphs while missing the actual
meaning of the visual. That can cause retrieval failures for questions like:

```text
Which system sends events into the compliance queue?
```

or:

```text
What does the Q4 revenue chart show for the enterprise segment?
```

If the answer is inside a diagram or chart image, normal text chunking will not
find it unless the document also describes the image in prose.

---

## Desired Outcome

After multi-modal indexing, visual content should become searchable through the
same MCP tools:

- `rag_search`
- `rag_get_context`
- `rag_get_document`
- `rag_explain_match`

Search results should still return citation-ready records, but those citations
may point to a page, figure, chart, or image region rather than only a paragraph.

Example result:

```text
Document: Cloud Security Architecture
Citation: Cloud Security Architecture, p. 12, fig. 3
Snippet: Figure 3 shows the event ingestion path. Application logs flow into
Kafka, then into the compliance queue, then into the audit archive.
```

---

## Architecture

Multi-modal indexing should use the existing extension-worker model.

```text
PDF / PPTX / DOCX
    |
    v
document loader worker
    |
    | extracts text, pages, image regions, charts, screenshots
    v
vision model worker
    |
    | OCR + caption + chart/table interpretation
    v
rag-core
    |
    | creates text chunks and visual-derived chunks
    v
VectorStore
    |
    v
rag_search / rag_get_context
```

The core Rust service should not directly depend on a specific vision model.
Vision work should be handled by an out-of-process worker over the extension
protocol, likely implemented first in Python.

---

## Data Model Additions

The current `Chunk` model already supports metadata, document title, URL, page,
section, permissions, and arbitrary extra document metadata. Multi-modal
indexing can start by representing visual content as normal text chunks with
richer metadata.

Suggested chunk metadata fields:

| Field | Purpose |
|---|---|
| `content_modality` | `text`, `image`, `chart`, `diagram`, `table_image`, `scanned_text` |
| `page` | Source page number |
| `figure_label` | Human label such as `Figure 3` or `Chart 2` |
| `bbox` | Bounding box on page: `x`, `y`, `width`, `height` |
| `ocr_text` | Text detected inside the image |
| `caption` | Caption generated or extracted for the visual |
| `visual_summary` | Short natural-language description |
| `visual_details` | Longer model-generated description for retrieval |
| `image_hash` | Stable hash for deduplication |
| `image_ref` | Optional pointer to stored image bytes or thumbnail |

At first, only the generated text fields need to be embedded. Later, a separate
multi-vector design could store both text embeddings and image embeddings.

---

## Extension Protocol Additions

The existing extension protocol already includes `LoadDocument`, `EmbedTexts`,
`Rerank`, `Transform`, and related capability concepts. Multi-modal indexing
can use a new or existing transform-style capability.

Suggested new capability:

```text
analyze_visuals
```

Suggested request payload:

```json
{
  "document_id": "doc_123",
  "source_id": "sharepoint-finance",
  "content_type": "application/pdf",
  "pages": [
    {
      "page": 12,
      "images": [
        {
          "image_id": "img_12_1",
          "data_base64": "...",
          "bbox": { "x": 72, "y": 140, "width": 440, "height": 280 },
          "nearby_text": "Figure 3. Event ingestion architecture"
        }
      ]
    }
  ],
  "metadata": {}
}
```

Suggested response payload:

```json
{
  "visual_chunks": [
    {
      "image_id": "img_12_1",
      "page": 12,
      "content_modality": "diagram",
      "figure_label": "Figure 3",
      "caption": "Event ingestion architecture",
      "ocr_text": "Application Logs -> Kafka -> Compliance Queue -> Audit Archive",
      "visual_summary": "The diagram shows application logs flowing into Kafka, then into the compliance queue, then into the audit archive.",
      "bbox": { "x": 72, "y": 140, "width": 440, "height": 280 },
      "confidence": 0.86
    }
  ]
}
```

The response should be model-neutral. The runtime should not care whether the
worker used a local model, cloud vision model, OCR engine, or a specialized
chart parser.

---

## Indexing Flow

### 1. Load document

The document loader extracts:

- normal text
- page boundaries
- embedded images
- rendered page images when needed
- surrounding text near each visual
- existing captions or figure labels

For PDFs, this likely belongs in a Python worker using libraries such as
PyMuPDF, pdfplumber, or similar tooling. For DOCX/PPTX/XLSX, format-specific
workers can expose embedded images and layout hints.

### 2. Analyze visuals

The vision worker receives extracted images or rendered page regions and
returns:

- OCR text
- image type classification
- caption or summary
- detailed description
- chart interpretation when possible
- bounding box and confidence

The worker should prefer factual descriptions over creative interpretation.
For charts, it should capture axis labels, series names, units, trend
direction, and notable values when readable.

### 3. Create visual-derived chunks

`rag-core` or the connector layer converts each visual analysis into a normal
chunk text body:

```text
[Diagram on page 12, Figure 3: Event ingestion architecture]
OCR: Application Logs -> Kafka -> Compliance Queue -> Audit Archive
Summary: The diagram shows application logs flowing into Kafka, then into the
compliance queue, then into the audit archive.
Nearby text: ...
```

This text is embedded by the normal embedder and stored through the normal
`VectorStore` trait.

### 4. Retrieve normally

At query time, no special multimodal retrieval is required for the first
version. Queries embed as text and can match visual-derived chunks because the
visuals were converted into descriptive text at index time.

---

## Citation Behavior

Visual citations need to be precise enough for a user to verify the result.

Recommended citation label formats:

```text
Document Title, p. 12, fig. 3
Document Title, p. 8, chart 2
Document Title, p. 4, scanned text region
```

If the source URL can deep-link to a page or anchor, include that URL. If not,
return the document URL plus page and figure metadata.

`rag_get_document` should eventually expose visual chunks alongside text chunks
so clients can show a page thumbnail or image crop.

---

## Storage Options

The first version should store visual-derived text and metadata in the same
`VectorStore` as normal chunks.

Image bytes should not be stored directly inside every vector backend unless
there is a clear reason. Better options:

- object storage for extracted image crops
- filesystem cache for local deployments
- database blob table for small deployments
- no image storage at first, only page and bounding box metadata

The minimal implementation only needs:

- `content_modality`
- page number
- bounding box
- caption/summary/OCR text
- citation metadata

---

## Retrieval Modes

### Phase 1: Text-only retrieval over visual descriptions

Convert visual content to text at index time and embed it with the normal
embedder. This is the simplest and most useful first step.

### Phase 2: Hybrid text + image embedding

Store both:

- text embedding for visual description
- image embedding for the original crop

This requires a richer `VectorStore` model, possibly named vectors or a
secondary image-vector collection.

### Phase 3: Visual reranking

For visual-heavy queries, retrieve candidate visual chunks, then ask a vision
model to inspect the original image crop and rerank or verify the answer.

This is more expensive but useful for high-stakes chart and diagram questions.

---

## Quality and Safety

Vision-generated descriptions can be wrong. The implementation should treat
them as retrieval aids, not ground truth.

Recommended safeguards:

- store confidence scores from OCR and vision workers
- keep OCR text separate from model-generated descriptions
- preserve bounding boxes and page numbers for human verification
- allow low-confidence visual chunks to be filtered or downranked
- include nearby source text to reduce hallucinated visual descriptions
- log model name and worker version used for analysis
- support reindexing visuals when the worker improves

For high-stakes use cases, final answers should cite the page/figure and avoid
claiming unreadable values from charts unless the extraction confidence is high.

---

## Implementation Plan

### Milestone A: Metadata-only support

- Add conventions for `content_modality`, `figure_label`, `bbox`, `ocr_text`,
  `caption`, and `visual_summary` in chunk metadata.
- Update docs and tests to confirm visual chunks can be stored and retrieved.
- No new vision worker yet.

### Milestone B: PDF visual extraction worker

- Add a Python PDF loader that extracts page images and embedded visual regions.
- Return visual regions through the extension protocol.
- Create visual-derived chunks from OCR and captions.

### Milestone C: Vision analysis worker

- Add an out-of-process worker that performs OCR and visual captioning.
- Route visual regions from the PDF loader to the vision worker.
- Store visual summaries as searchable chunks.

### Milestone D: MCP visibility

- Update `rag_search` and `rag_get_context` responses to expose modality,
  page, figure label, and confidence.
- Update `rag_get_document` to list text chunks and visual chunks.
- Update `rag_explain_match` to explain whether the match came from OCR,
  caption, visual summary, or nearby text.

### Milestone E: Advanced retrieval

- Evaluate image embeddings and named vectors.
- Add optional visual reranking for visual-heavy queries.
- Add thumbnails or image crops for clients that can display them.

---

## Open Questions

- Should visual chunks share the same `Chunk` type or get a dedicated
  `VisualChunk` model?
- Should image crops be stored by this project or only referenced externally?
- Which visual worker should be the first supported reference implementation?
- How should chart values be represented when extraction confidence is partial?
- Should `VectorStore` grow named-vector support, or should visual embeddings
  live in a separate store?

---

## Summary

The first useful version of multi-modal indexing should be text-first:
extract visuals, generate OCR/captions/descriptions, store those as normal
chunks with rich metadata, and retrieve them through the existing RAG tools.

That design gives users access to diagrams and charts without disrupting the
current architecture. More advanced image embeddings and visual reranking can
come later, once the simpler visual-derived chunk pipeline proves useful.
