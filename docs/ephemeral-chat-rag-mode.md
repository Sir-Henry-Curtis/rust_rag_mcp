# Ephemeral Chat RAG Mode

Ephemeral chat RAG mode runs `rust-rag-mcp` as a short-lived, per-chat retrieval
server for uploaded documents. It is designed for documents that are too large
to fit directly into an LLM context window, but that should not be stored in a
long-term enterprise index.

This mode should run alongside the normal persistent SharePoint-backed RAG
server. The two modes share `rag-core`, chunking, embedding, retrieval, and
MCP tool concepts, but they have different lifecycles and storage guarantees.

---

## Relationship to Persistent SharePoint RAG

Persistent enterprise RAG and ephemeral chat RAG solve different problems.

| Mode | Purpose | Storage | Lifetime |
|---|---|---|---|
| Persistent SharePoint RAG | Search organization knowledge across many chats | Durable `VectorStore`, usually pgvector | Long-running service |
| Ephemeral chat RAG | Search files uploaded for one chat/session | Memory or temporary storage | Ends with chat/session TTL |

They can run at the same time:

```text
Claude / MCP host
    |
    |-- persistent-rag-mcp
    |     - SharePoint libraries
    |     - durable pgvector index
    |     - incremental sync
    |     - permission-aware enterprise retrieval
    |
    |-- ephemeral-chat-rag-mcp
          - uploaded files for this chat
          - in-memory or temp index
          - no long-term storage
          - deleted on chat end
```

The assistant can use the persistent instance for company knowledge and the
ephemeral instance for user-provided files in the current conversation.

---

## Goal

The goal is to let an assistant work with large uploaded documents without
placing those documents into the permanent corpus.

Example:

```text
1. User uploads a 400-page PDF during a chat.
2. Ephemeral RAG extracts text, chunks it, embeds it, and indexes it in memory.
3. The assistant asks `rag_get_context` against that temporary index.
4. Results include filename/page citations.
5. When the chat ends, the temporary index is destroyed.
```

This is useful for:

- one-off contract review
- long PDF analysis
- uploaded technical manuals
- private reports
- discovery packets
- research papers
- files the user does not want added to enterprise search

---

## Non-Goals

Ephemeral mode should not:

- sync SharePoint libraries
- persist uploaded documents after the session ends
- write chunks to the production pgvector database
- expose uploaded files to other chats
- become the authoritative enterprise knowledge base
- bypass model/provider data handling policies

It is a temporary context expansion tool, not a durable document management
system.

---

## Architecture

```text
Chat session starts
    |
    v
Ephemeral MCP server starts
    |
    v
User uploads documents
    |
    v
Upload connector / ingestion tool
    |
    v
Document loader -> chunker -> embedder -> temporary VectorStore
    |
    v
rag_search / rag_get_context / rag_get_document
    |
    v
Chat ends or TTL expires
    |
    v
Temporary index and extracted text are deleted
```

The minimal implementation can use:

| Component | Ephemeral default |
|---|---|
| `VectorStore` | `MemoryVectorStore` |
| Document source | Uploaded files |
| `SourceId` | Chat/session ID |
| `DocumentId` | Stable ID per uploaded file |
| Citations | Filename + page/section |
| Cleanup | Process exit, explicit clear, or TTL |

---

## MCP Tool Surface

The existing search/read tools still apply:

| Tool | Use |
|---|---|
| `rag_search` | Search uploaded documents |
| `rag_get_context` | Build prompt-ready context from uploaded documents |
| `rag_get_document` | Inspect an uploaded document and its chunks |
| `rag_explain_match` | Explain why an uploaded-file chunk matched |

Ephemeral mode likely needs a small upload/session tool surface:

| Tool | Purpose |
|---|---|
| `rag_add_uploaded_file` | Add one file to the temporary session index |
| `rag_add_document_text` | Add already-extracted text to the temporary index |
| `rag_list_session_documents` | List files indexed in the current chat |
| `rag_clear_session` | Delete all session chunks and extracted text |
| `rag_session_status` | Show chunk count, document count, and expiry time |

If the host already handles file upload bytes, `rag_add_uploaded_file` can
accept a file path, stream handle, or base64 payload depending on transport
capabilities.

---

## Storage and Cleanup

The strongest privacy posture is memory-only:

```text
uploaded bytes -> extracted text -> chunks -> embeddings -> MemoryVectorStore
```

When the process exits, the index disappears.

For very large files, temporary disk storage may be needed. If so:

- create a unique temp directory per session
- store only necessary intermediate files
- delete temp files on `rag_clear_session`
- delete temp files on shutdown
- enforce a TTL cleanup task
- avoid writing to the persistent vector store

Recommended cleanup triggers:

- chat closes
- explicit `rag_clear_session`
- idle timeout
- maximum session age
- process shutdown
- failed ingestion cleanup

---

## Running Alongside SharePoint RAG

The expected deployment is two MCP servers registered with the same host:

```text
persistent-rag-mcp
  command: rust-rag-mcp --config rag.sharepoint.toml
  storage: pgvector
  sources: SharePoint, filesystem, S3

ephemeral-chat-rag-mcp
  command: rust-rag-mcp --mode ephemeral --session-id <chat-id>
  storage: memory or temp
  sources: uploaded files only
```

The assistant can choose based on intent:

| User intent | Preferred instance |
|---|---|
| "Search our remote work policy" | Persistent SharePoint RAG |
| "In this uploaded contract, what are the renewal terms?" | Ephemeral chat RAG |
| "Compare our policy to this uploaded vendor agreement" | Both instances |

For comparison tasks, the assistant can retrieve from both:

```text
1. Ask persistent RAG for company policy.
2. Ask ephemeral RAG for uploaded contract clauses.
3. Synthesize a cited comparison.
```

---

## Configuration Example

```toml
[mode]
kind = "ephemeral"
session_id = "chat_abc123"
ttl_minutes = 120
read_only_after_ingest = true

[store]
backend = "memory"

[uploads]
max_file_bytes = 104857600
max_total_session_bytes = 524288000
allowed_extensions = ["pdf", "docx", "xlsx", "pptx", "txt", "md"]
temp_dir = null

[embedder]
provider = "openai"
dimension = 1536
openai_model = "text-embedding-3-small"
```

For local/private deployments, `provider = "local-onnx"` may be preferable if
uploaded document text should not be sent to an external embedding API.

---

## Security and Privacy

Ephemeral mode should make its storage behavior obvious.

Recommended guarantees:

- uploaded documents are scoped to one session
- chunks are not written to the persistent store
- temporary files are deleted on cleanup
- session IDs are unguessable
- uploads have size and type limits
- no mutation tools for persistent sources are available
- logs do not include uploaded document text
- errors do not leak document contents

If external embedding or vision providers are used, the deployment docs should
make that clear. "Ephemeral" means no long-term storage by this server; it does
not automatically mean no external provider receives text.

---

## Implementation Plan

### Milestone A: Ephemeral config mode

- Add a config flag for `mode = "ephemeral"`.
- Force `store.backend = "memory"` unless explicitly configured for temp
  storage.
- Disable source sync tools.
- Add TTL and session ID settings.

### Milestone B: Upload ingestion tools

- Add `rag_add_document_text`.
- Add `rag_add_uploaded_file` for supported transports.
- Use existing loaders, chunker, embedder, and `Indexer`.
- Generate citations from filename, page, and section metadata.

### Milestone C: Session tools

- Add `rag_list_session_documents`.
- Add `rag_session_status`.
- Add `rag_clear_session`.
- Ensure cleanup deletes chunks and temporary files.

### Milestone D: Host integration

- Document how to run one ephemeral server per chat.
- Document how to register persistent and ephemeral MCP servers side by side.
- Add examples for comparing persistent policy results with uploaded-file
  results.

### Milestone E: Large-file hardening

- Stream extraction and embedding in batches.
- Add session byte limits.
- Add ingestion progress status.
- Add cancellation support.
- Add cleanup on partial ingestion failure.

---

## Open Questions

- Should one process serve many ephemeral sessions, or should each chat get its
  own process?
- How should MCP hosts pass uploaded file bytes or file handles?
- Should temp storage be allowed, or should the first implementation be
  memory-only?
- Should ephemeral mode support multimodal visual extraction?
- How should session cleanup be verified in tests?

---

## Summary

Ephemeral chat RAG mode is a companion deployment mode for uploaded documents.
It should run alongside the persistent SharePoint RAG instance:

- persistent RAG handles durable enterprise knowledge
- ephemeral RAG handles temporary chat-local files

Both use the same core retrieval engine, but they have different storage and
lifecycle rules. This keeps uploaded documents useful during a chat without
committing them to the long-term organizational index.
