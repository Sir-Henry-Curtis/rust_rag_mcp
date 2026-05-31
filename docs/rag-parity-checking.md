# RAG Parity Checking

Parity checking is how this project compares its behavior against established
open-source RAG systems without trying to clone any one of them.

The goal is not feature parity for its own sake. The goal is to make sure
`rust-rag-mcp` covers the important capabilities users expect from a serious
RAG library and MCP server:

- reliable ingestion
- clean retrieval abstractions
- useful document parsing
- metadata and citation quality
- permission-aware search
- vector store flexibility
- local/private and enterprise deployment modes
- evaluation and debugging hooks
- agent/tool integration

When another project has a useful feature, this project should decide whether
to adopt it, adapt it to the Rust/MCP architecture, defer it, or explicitly
leave it out of scope.

---

## Reference Projects

These projects form the parity reference set.

| Project | Category | What to Learn From |
|---|---|---|
| Haystack | RAG framework | Pipeline composition, document stores, retrievers, generators, rerankers, evaluation |
| LlamaIndex | RAG/data framework | Connectors, indexes, query engines, retrievers, agents, metadata filtering |
| LangChain | Agent/tool framework | Retriever interfaces, vector store integrations, document loaders, tool/agent integration |
| RAGFlow | Document RAG engine | PDF/document understanding, layout-aware parsing, chunk quality, production document workflows |
| Dify | LLM app platform | Knowledge bases, workflows, external knowledge APIs, app-facing RAG UX |
| AnythingLLM | Self-hosted document chat | Workspace-based document upload, local/private RAG, user-facing document QA |
| PrivateGPT | Local/private RAG | Local document ingestion, privacy-first deployment, uploaded-file workflows |
| Khoj | Personal knowledge RAG | Personal knowledge search, local notes/docs, conversational retrieval |
| Flowise | Visual LLM/RAG builder | No-code RAG pipelines, retriever composition, operational UX expectations |
| txtai | Embeddings/RAG library | Embedding indexes, semantic search, lightweight local workflows |
| Verba | RAG application | Search UX, document exploration, Weaviate-backed RAG behavior |

Primary parity targets:

1. Haystack and LlamaIndex for library/API shape.
2. RAGFlow for document ingestion quality.
3. Dify and AnythingLLM for product/workflow expectations.
4. PrivateGPT and Khoj for local/private and ephemeral-document behavior.

---

## Parity Dimensions

Parity checks should be organized by capability, not by project.

| Dimension | Questions to Ask |
|---|---|
| Ingestion | Can documents be loaded, parsed, chunked, embedded, and reindexed safely? |
| Connectors | Are source integrations practical for real systems, not just demos? |
| Chunking | Can chunk size, overlap, page/section metadata, and layout hints be controlled? |
| Embeddings | Are local and hosted embedding providers supported with batching and retries? |
| Vector stores | Can the backend be swapped without changing retrieval callers? |
| Retrieval | Are metadata filters, top-k search, reranking, hybrid search, and citations possible? |
| Citations | Can users verify answers through document title, URL, page, section, and chunk metadata? |
| Permissions | Can results be filtered by caller identity and source-native ACL hints? |
| Evaluation | Can retrieval quality, citation support, and latency be measured? |
| Tool/API surface | Can agents and apps call search, context, document, index, sync, and explain tools? |
| Operations | Are config validation, observability, graceful shutdown, and failure handling present? |
| Privacy modes | Can the system support persistent enterprise RAG and ephemeral uploaded-document RAG? |

---

## How to Run a Parity Check

For each milestone, perform a small review before marking it complete.

1. Select the relevant reference projects.
2. Compare the milestone's feature surface against their equivalent concepts.
3. Record any meaningful gap as one of:
   - `adopt now`
   - `adapt differently`
   - `defer`
   - `out of scope`
4. Add follow-up roadmap or parking-lot items where needed.
5. Confirm the project still fits its own architecture: Rust core, MCP/HTTP
   surfaces, source-specific connectors, extension workers, and strong
   citations.

Parity findings should be written down in the PR, release notes, milestone
notes, or a short markdown appendix.

---

## Milestone Mapping

| Milestone | Primary Parity References | Focus |
|---|---|---|
| M1 Core Scaffold | Haystack, LlamaIndex, LangChain | Core abstractions and retrieval pipeline shape |
| M2 pgvector Store | Haystack, LlamaIndex, LangChain, txtai | Vector store adapter behavior and metadata filters |
| M3 SharePoint + Parsing | RAGFlow, LlamaIndex, Dify | Connector realism, document metadata, parsing quality |
| M4 Extension Bus | Haystack, LangChain, Dify, Flowise | Plugin/worker model and external component ergonomics |
| M5 MCP Layer | Dify, AnythingLLM, LangChain, LlamaIndex | Tool surface, context responses, app/agent integration |
| M6 Embedding Providers | Haystack, LlamaIndex, LangChain, txtai | Provider configuration, batching, retries, local embeddings |
| M7 Hardening | Dify, RAGFlow, Haystack | Observability, retries, rate limits, production behavior |
| M8 Python SDK | Haystack, LangChain, LlamaIndex | Extension author ergonomics and Python developer experience |
| M9 v1.0 Release | Dify, AnythingLLM, RAGFlow | Deployment docs, release packaging, stable public API |
| M10 HTTP API | Dify, AnythingLLM, LlamaIndex | REST shape, external knowledge API, migration friendliness |

---

## Feature Development Rules

Use parity checks to sharpen decisions, not to inflate scope.

Adopt a feature when:

- it solves a real use case in this project's roadmap
- it fits the trait-based Rust architecture
- it improves retrieval quality, safety, citations, or operations
- it can be tested without a large product surface

Adapt a feature when:

- another project's idea is good but its implementation assumes Python,
  notebooks, web UI, or a hosted control plane
- MCP/HTTP callers need a more structured version
- enterprise permissions or citations require a stricter model

Defer a feature when:

- it is valuable but not needed for the next release
- it belongs in the parking lot
- it depends on another milestone

Leave a feature out of scope when:

- it turns this project into a SaaS platform
- it requires a UI/control plane the roadmap explicitly excludes
- it weakens permission boundaries
- it makes `rag-core` depend on a specific agent framework

---

## Examples

### Document Parsing

RAGFlow should be used as a reference for document parsing expectations,
especially PDFs with tables, figures, scanned pages, and layout structure.

This project should not copy RAGFlow's whole product shape. Instead, it should
use the comparison to improve:

- loader worker payloads
- page/section metadata
- table and visual extraction
- chunk quality checks
- citation precision

### Query Engines

LlamaIndex has many query/retrieval patterns. This project should use that as
inspiration for:

- query rewriting
- multi-query retrieval
- reranking
- metadata-aware retrieval
- conversation-aware retrieval

But the implementation should still return simple `SearchResult` and
context-passage structures through MCP/HTTP.

### App Workflows

Dify and AnythingLLM are useful references for what users expect from knowledge
bases and uploaded-document chat.

This project should use them to check:

- file upload flow expectations
- session/local document behavior
- knowledge source status
- clear error messages
- external API usability

It should not expand into a full workflow builder or web UI unless the roadmap
changes.

---

## Deliverables

Each milestone parity check should produce at least one of:

- a short milestone note
- a PR checklist item
- a roadmap update
- a test case inspired by a parity gap
- a documented decision to defer or reject a feature

The most important output is not a giant comparison table. It is better product
judgment: knowing which features matter, which ones do not, and which ones need
to be shaped differently for a Rust-first MCP RAG engine.

---

## Reference Links

- [Haystack](https://haystack.deepset.ai/)
- [LlamaIndex](https://www.llamaindex.ai/)
- [LangChain](https://docs.langchain.com/oss/python/langchain/overview)
- [RAGFlow](https://github.com/infiniflow/ragflow)
- [Dify Knowledge Base](https://docs.dify.ai/en/guides/knowledge-base)
- [Dify External Knowledge Base](https://docs.dify.ai/en/use-dify/knowledge/connect-external-knowledge-base)
- [AnythingLLM](https://anythingllm.com/)
- [PrivateGPT](https://privategpt.dev/)
- [Khoj](https://docs.khoj.dev/features/all-features/)
- [Flowise](https://flowiseai.com/)
- [txtai](https://github.com/neuml/txtai)
- [Verba](https://github.com/weaviate/Verba)
