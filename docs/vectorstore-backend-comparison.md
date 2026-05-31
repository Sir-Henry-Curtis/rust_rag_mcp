# VectorStore Backend Comparison

`rag-core` defines `VectorStore` as the storage and search boundary for chunk
embeddings. That trait lets this project keep the RAG pipeline stable while
different backends handle vector persistence, indexing, metadata filtering, and
nearest-neighbor search.

This document compares practical backend candidates for future
`VectorStore` adapter crates.

---

## What VectorStore Needs

For this project, a good backend needs more than raw vector search. It should
support the RAG shape of the data:

- upsert document chunks with embeddings
- search by query embedding
- filter by source ID, document ID, content type, and permissions
- delete all chunks for a document or source
- return stable chunk IDs, text, metadata, score, and citation fields
- survive process restarts
- scale with corpus size without making retrieval slow or fragile

The current production-oriented backend is `rag-store-pgvector`, using
PostgreSQL plus pgvector. Future adapters can implement the same trait without
changing `Indexer`, `Retriever`, MCP tools, or callers.

---

## Summary Recommendation

| Backend | Best fit |
|---|---|
| pgvector | Default self-hosted choice; simplest if the project already uses PostgreSQL |
| OpenSearch | Best open-source search platform when hybrid keyword + vector search matters |
| Elasticsearch | Strong managed or enterprise search choice with mature text search and vector search |
| Qdrant | Strong dedicated vector database with good filtering and simple operations |
| Weaviate | Feature-rich vector database with hybrid search, modules, multi-tenancy, and RAG conveniences |
| Pinecone | Managed vector database when teams want low operations burden |
| Milvus | Large-scale open-source vector database for high-volume, specialized vector workloads |
| LanceDB | Embedded or lakehouse-style vector search, especially for multimodal/local data workflows |
| Chroma | Developer-friendly local/prototype vector store |
| SQLite vector extensions | Small local or edge deployments, not primary enterprise corpus search |
| FAISS | High-performance vector search library, but not a full durable metadata database by itself |

For this project's main enterprise SharePoint RAG target, the strongest
near-term candidates are:

1. `pgvector` for the default durable store.
2. OpenSearch or Elasticsearch for hybrid search-heavy deployments.
3. Qdrant for a dedicated open-source vector database.
4. Pinecone for managed cloud vector search.

---

## Comparison Table

| Backend | Type | Strengths | Tradeoffs | Adapter priority |
|---|---|---|---|---|
| pgvector | PostgreSQL extension | One database for vectors and metadata; SQL filters; transactional deletes; easy local Docker setup | Not as specialized as dedicated vector DBs at very large scale; tuning HNSW/IVFFlat and filters matters | Already implemented |
| OpenSearch | Search engine | Strong keyword search, filters, aggregations, vector search, hybrid retrieval; open-source deployment | Operationally heavier than pgvector; mapping/index management; JVM/search-cluster complexity | High |
| Elasticsearch | Search engine | Mature search platform; dense vector and kNN search; excellent text search and ecosystem | License/edition choices matter; operational complexity; cloud features may differ from self-hosted | High |
| Qdrant | Vector database | Purpose-built vector search; strong payload filtering; simple API; good fit for metadata-filtered RAG | Less native full-text search than search engines; another service to operate | High |
| Weaviate | Vector database | Vector, keyword, hybrid search, named vectors, modules, multi-tenancy, RAG-oriented features | More feature surface to configure; schema/module decisions can become project-specific | Medium-high |
| Pinecone | Managed vector database | Low ops burden; managed scaling; metadata filters; namespaces for separation | External SaaS dependency; cost and data residency concerns; less control than self-hosting | Medium-high |
| Milvus | Vector database | Built for large vector collections; supports dense, sparse, hybrid, and multi-vector patterns | More moving parts; heavier operational profile; likely overkill for small/medium corpora | Medium |
| LanceDB | Embedded/serverless/lakehouse vector DB | Good local and data-lake style workflows; hybrid search; multimodal orientation | Less natural as a central enterprise service than Postgres/OpenSearch/Qdrant | Medium-low |
| Chroma | Vector DB / developer store | Very friendly for local development and prototypes; metadata/document filters | Not the first choice for strict enterprise operations, ACL-heavy retrieval, or large production corpora | Low |
| SQLite vec/vec1 | SQLite extension | Single-file local storage; good for edge/local tools; simple deployment | Limited enterprise concurrency/operations story; newer vector capabilities | Low |
| FAISS | Library | Very fast vector indexing/search; excellent for custom high-performance retrieval | Not a complete database: metadata, persistence, filtering, deletion, and replication need extra design | Low as standalone |

---

## pgvector

pgvector is the natural default for this project because it keeps embeddings and
chunk metadata inside PostgreSQL. That fits the current crate architecture:
`rag-store-pgvector` can use SQL for source/document filters, document-level
deletes, source-level deletes, inspection helpers, and dimension guards.

Use pgvector when:

- the deployment already trusts PostgreSQL
- the corpus is small to moderately large
- source metadata and permission filters matter
- operators want simple backup, restore, and migration workflows
- transactional behavior is valuable

Avoid or revisit pgvector when:

- vector search volume is extremely high
- the team needs a specialized distributed vector database
- hybrid keyword/vector ranking becomes central and PostgreSQL full-text search
  is not enough

Implementation notes:

- HNSW is a good default approximate index for many RAG workloads.
- IVFFlat can be useful for larger indexes where index size/build behavior is
  more important.
- Metadata filters should be backed by normal SQL indexes where possible.
- The adapter should validate embedding dimension at startup, as the current
  implementation already does.

Official docs: [pgvector GitHub](https://github.com/pgvector/pgvector)

---

## OpenSearch

OpenSearch is a strong candidate when this project needs both search-engine
features and vector search. It can handle keyword search, filters,
aggregations, and kNN vector queries in one system. That matters for hybrid
search, where exact terms such as policy IDs, project names, ticket numbers,
and acronyms need to combine with semantic similarity.

Use OpenSearch when:

- hybrid search is a major requirement
- the organization already operates OpenSearch
- users need faceted search, aggregations, or search analytics
- exact keyword matching and semantic matching must work together
- search data is shared with dashboards or non-RAG applications

Avoid or revisit OpenSearch when:

- the project wants the simplest possible local deployment
- the team does not want to operate a search cluster
- the workload is pure vector search with modest metadata filtering

Implementation notes:

- Store each chunk as an indexed document.
- Store embedding in a vector field.
- Store citation metadata, permissions, source ID, document ID, and content type
  as filterable fields.
- Implement `delete_by_document` and `delete_by_source` as delete-by-query or
  deterministic ID deletion.
- Consider a hybrid retrieval mode outside the current `VectorStore` trait if
  BM25 + vector scoring becomes first-class.

Official docs: [OpenSearch k-NN query](https://docs.opensearch.org/latest/query-dsl/specialized/k-nn/index/)

---

## Elasticsearch

Elasticsearch plays a similar role to OpenSearch: mature search engine first,
vector database second. It is a good fit when an organization already uses the
Elastic stack or wants a managed Elastic deployment with strong observability
and search tooling.

Use Elasticsearch when:

- the organization already has Elastic expertise
- text search quality and operational tooling matter
- indexed chunks need to support both RAG and traditional search UI use cases
- managed Elastic is preferred over operating a separate vector DB

Avoid or revisit Elasticsearch when:

- licensing or edition constraints are a concern
- the project wants a permissive open-source-only stack
- the team only needs a simple vector store

Implementation notes:

- Similar to OpenSearch, each chunk maps naturally to one indexed document.
- The adapter should use dense vector / kNN support for semantic retrieval.
- Hybrid search may require an adapter-specific extension beyond the current
  trait if the caller needs combined BM25 and vector scores.

Official docs: [Elasticsearch kNN search](https://www.elastic.co/guide/en/elasticsearch/reference/current/knn-search.html)

---

## Qdrant

Qdrant is a purpose-built vector database with a clean model: points have
vectors and payload. That maps well to RAG chunks, where the vector is the
embedding and the payload is source metadata, permissions, citation data, and
chunk text.

Use Qdrant when:

- the project wants a dedicated open-source vector database
- metadata filtering is important
- operations should be simpler than a full search-engine cluster
- the team wants clear APIs and Rust-friendly concepts

Avoid or revisit Qdrant when:

- traditional keyword search and aggregations are equally important
- the organization already standardizes on PostgreSQL or OpenSearch
- the team does not want another service

Implementation notes:

- Store chunk IDs as point IDs where possible.
- Store source/document/content-type/permission fields in payload.
- Create payload indexes for fields used in filters.
- Use named vectors only if the project adds multi-embedding or multimodal
  retrieval later.

Official docs: [Qdrant filtering](https://qdrant.tech/documentation/concepts/filtering/)

---

## Weaviate

Weaviate is a feature-rich vector database with vector search, keyword search,
hybrid search, named vectors, modules, and multi-tenancy features. It can be a
good fit for RAG systems that want more database-level AI features rather than
keeping every decision in application code.

Use Weaviate when:

- hybrid search should be built into the backend
- named vectors or multimodal retrieval are likely
- multi-tenancy is a first-class requirement
- integrated vectorization/reranking modules are attractive

Avoid or revisit Weaviate when:

- the project wants minimal backend-specific behavior
- schema/module configuration would make deployments too variable
- operators prefer simpler PostgreSQL or Qdrant deployments

Implementation notes:

- Decide whether embeddings are always produced by `rag-core` or whether
  Weaviate modules may vectorize data.
- For consistency with this project, prefer externally produced embeddings at
  first.
- Treat integrated RAG/generative features as optional; the project should
  still return its own citation-ready `SearchResult` records.

Official docs: [Weaviate vector search](https://docs.weaviate.io/weaviate/concepts/search/vector-search), [Weaviate search concepts](https://docs.weaviate.io/weaviate/concepts/search)

---

## Pinecone

Pinecone is a managed vector database. Its main attraction is reducing
operations work: no Postgres tuning, no search cluster, no custom vector
database deployment.

Use Pinecone when:

- the team wants a managed vector database
- cloud/SaaS dependency is acceptable
- operational simplicity matters more than infrastructure control
- metadata filters and namespaces cover the permission/source model

Avoid or revisit Pinecone when:

- documents or embeddings cannot leave a private environment
- strict data residency rules prevent SaaS use
- cost predictability is more important than managed scaling
- the project needs SQL-style joins or search-engine-style aggregations

Implementation notes:

- Keep chunk text and citation metadata either in Pinecone metadata or in a
  separate durable document store.
- Watch metadata size and shape limits.
- Use namespaces carefully: they can help with tenant/source separation, but
  caller permissions may still need explicit metadata filters.

Official docs: [Pinecone indexing and metadata filtering](https://docs.pinecone.io/docs/metadata-filtering)

---

## Milvus

Milvus is a large-scale open-source vector database. It is a serious candidate
when vector volume or performance requirements outgrow simpler stores.

Use Milvus when:

- vector collections are very large
- specialized vector index choices matter
- dense, sparse, hybrid, or multi-vector search is central
- the team has the operational capacity for a larger vector database stack

Avoid or revisit Milvus when:

- the project is still early
- PostgreSQL or Qdrant is operationally sufficient
- the corpus is not large enough to justify the added moving parts

Implementation notes:

- The adapter should start with one dense vector field and scalar metadata
  filters.
- Sparse/hybrid/multi-vector support should be a later extension, not part of
  the first adapter.
- Deletion and compaction behavior should be tested carefully for source-level
  reindexing.

Official docs: [Milvus overview](https://blog.milvus.io/docs/overview.md), [Milvus filtered search](https://blog.milvus.io/docs/filtered-search.md)

---

## LanceDB

LanceDB is interesting for embedded, local, lakehouse, and multimodal workflows.
It supports vector search and hybrid search patterns, and it fits teams that
want vector search close to file/table-oriented data.

Use LanceDB when:

- local or embedded workflows matter
- multimodal data is likely
- data-lake style storage is attractive
- batch/offline research workflows need fast vector search without a central
  database service

Avoid or revisit LanceDB when:

- the main target is a central multi-user enterprise RAG service
- PostgreSQL, OpenSearch, or Qdrant already meet operational needs
- strict MCP server deployments prefer a conventional service backend

Implementation notes:

- A LanceDB adapter may be most useful for local development, evaluation, and
  offline indexing experiments.
- It may pair well with the evaluation workflows described in
  [Using rust-rag-mcp in Larger Systems](using-rag-in-larger-systems.md).

Official docs: [LanceDB hybrid search](https://lancedb.github.io/lancedb/hybrid_search/hybrid_search/)

---

## Chroma

Chroma is developer-friendly and common in prototypes. It is useful when a team
wants to get a RAG demo running quickly, inspect data locally, and iterate on
chunking or embedding choices.

Use Chroma when:

- local prototyping is the goal
- the corpus is small
- Python ecosystem convenience matters
- production operations are not yet the focus

Avoid or revisit Chroma when:

- the system needs strict enterprise operations
- permission-aware filtered retrieval is the most important feature
- the corpus is large or highly concurrent

Implementation notes:

- Chroma may be better as a dev/test adapter than a first production adapter.
- If added, it should still implement the same chunk metadata and deletion
  semantics as other stores.

Official docs: [Chroma filtering](https://cookbook.chromadb.dev/core/filters/)

---

## SQLite Vector Extensions

SQLite vector extensions are compelling for single-file local tools, edge
deployments, and demos. SQLite's official `vec1` extension provides approximate
nearest-neighbor vector search through SQLite virtual tables.

Use SQLite vector search when:

- deployment must be a single local file
- the corpus is small
- the system runs on laptops, edge devices, or test fixtures
- zero service operations is more important than scale

Avoid or revisit SQLite vector search when:

- the deployment is multi-user and write-heavy
- the corpus is large
- source-level ACL filtering must be highly optimized
- centralized observability and operations matter

Implementation notes:

- A SQLite adapter could be useful for examples and local test fixtures.
- It should not displace pgvector as the main durable store.

Official docs: [SQLite vec1](https://sqlite.org/vec1)

---

## FAISS

FAISS is a vector search library rather than a full database. It is excellent
for high-performance similarity search, but by itself it does not solve the
whole `VectorStore` problem for this project.

Use FAISS when:

- maximum vector search performance is needed
- the project can build its own persistence and metadata layer
- in-memory or custom-index workflows are acceptable
- evaluation or benchmarking needs a strong search-library baseline

Avoid or revisit FAISS when:

- the project needs a normal durable service backend
- metadata filtering, ACLs, deletes, and inspection need to be simple
- operations should be boring and database-like

Implementation notes:

- A FAISS-backed adapter would need a companion metadata store.
- Source/document deletes would need careful index rebuild or tombstone logic.
- It is better viewed as a lower-level building block than as a direct
  enterprise RAG store.

Official docs: [FAISS documentation](https://faiss.ai/), [FAISS GitHub](https://github.com/facebookresearch/faiss)

---

## Adapter Design Guidance

Every adapter should preserve the same project-level semantics even if the
backend has richer features:

1. `upsert_chunks` must write text, embedding, source ID, document ID, chunk
   index, citation metadata, and permissions.
2. `search` must accept a query embedding, `k`, and optional `SearchFilter`.
3. `delete_by_document` and `delete_by_source` must be reliable enough for
   reindexing.
4. `count_chunks` should be cheap enough for health checks.
5. Scores should be normalized or documented so callers can compare behavior.
6. Backend-specific hybrid search should not leak into `rag-core` until the
   trait intentionally grows to support it.
7. Permission filtering must remain correct even when backend filtering is
   approximate or limited.

If a backend cannot express the full `SearchFilter`, the adapter should fail
clearly or overfetch and apply a safe application-level filter. It should never
return unauthorized chunks for convenience.

---

## Suggested Implementation Order

1. Keep hardening `rag-store-pgvector`.
2. Add `rag-store-opensearch` for hybrid keyword/vector search.
3. Add `rag-store-qdrant` for dedicated vector database deployments.
4. Add `rag-store-pinecone` if managed cloud vector search is desired.
5. Revisit Weaviate, Milvus, LanceDB, Chroma, SQLite, and FAISS based on real
   user deployments.

This order keeps the project grounded: start with boring durable storage, then
add search-engine hybrid retrieval, then add specialized vector databases.

---

## Source Links

- [pgvector](https://github.com/pgvector/pgvector)
- [OpenSearch k-NN query](https://docs.opensearch.org/latest/query-dsl/specialized/k-nn/index/)
- [Elasticsearch kNN search](https://www.elastic.co/guide/en/elasticsearch/reference/current/knn-search.html)
- [Qdrant filtering](https://qdrant.tech/documentation/concepts/filtering/)
- [Weaviate vector search](https://docs.weaviate.io/weaviate/concepts/search/vector-search)
- [Weaviate search concepts](https://docs.weaviate.io/weaviate/concepts/search)
- [Pinecone indexing and metadata filtering](https://docs.pinecone.io/docs/metadata-filtering)
- [Milvus overview](https://blog.milvus.io/docs/overview.md)
- [Milvus filtered search](https://blog.milvus.io/docs/filtered-search.md)
- [LanceDB hybrid search](https://lancedb.github.io/lancedb/hybrid_search/hybrid_search/)
- [Chroma filtering](https://cookbook.chromadb.dev/core/filters/)
- [SQLite vec1](https://sqlite.org/vec1)
- [FAISS documentation](https://faiss.ai/)
