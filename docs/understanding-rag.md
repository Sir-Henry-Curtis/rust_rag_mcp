# Understanding RAG: Why Smart Search Makes AI Assistants Dramatically More Useful

*Written for developers and technical stakeholders who are new to working with large language models.*

---

## The Problem We Are Solving

When you ask Claude a question, it can only consider information that fits inside its **context window** — the block of text it has available while it is generating a response. Think of it like a very intelligent person who can only read a certain number of pages at one time before they have to put some down to pick up new ones.

Modern context windows are large — often 200,000 tokens or more, which is roughly 150,000 words. That sounds like a lot until you realise:

- A single 500-page policy manual is around 125,000 words.
- A SharePoint library with 300 such documents is 37.5 million words.
- An organisation's full document corpus might span hundreds of millions of words.

No context window is big enough to hold all of that. Something has to decide **which parts of that information are relevant to a specific question** before the model ever sees them.

That is the job of a RAG system.

---

## What RAG Is

**RAG** stands for **Retrieval-Augmented Generation**. The name describes the three-step process:

1. **Retrieval** — before the model answers anything, a search system finds the most relevant passages from a large document corpus.
2. **Augmentation** — those passages are added to the model's context window alongside the user's question.
3. **Generation** — the model reads the question and the retrieved passages together and generates an answer.

The model does not have to know everything. It just has to reason well about the information it is given. RAG makes the right information available at the right time.

### The Research Assistant Analogy

Imagine hiring a brilliant analyst to answer questions about your company's policies, contracts, and reports. You have two options:

**Option A — No RAG:** You hand the analyst the keys to the file room and say "answer this question." The analyst has to go read every document that might be relevant, bring it back, and then form a response. For simple questions this works. For complex questions spanning many documents, it is slow, expensive, and the analyst might miss things or exceed the time they have available.

**Option B — With RAG:** You hire a research librarian who has already read every document in your company, underlined every meaningful passage, and filed those passages by topic. When your analyst gets a question, the librarian instantly hands them the ten most relevant excerpts from across all 300 documents. The analyst reads those ten excerpts and answers the question. Faster, cheaper, and drawing on a far larger knowledge base.

The RAG system is the librarian. The LLM is the analyst.

---

## What "Embedding" Means and Why It Matters

The most important technique behind RAG is **text embedding**. An embedding model converts a passage of text into a list of numbers — typically hundreds or thousands of decimal values — that encode the *meaning* of that text, not just its words.

Two passages that mean the same thing in different words will produce very similar number lists. Two passages that are about completely different topics will produce very different number lists.

This is what makes RAG *semantic* search rather than *keyword* search:

| Keyword search | Semantic search |
|---|---|
| "thermal constraints" finds documents containing those exact words | "heat dissipation limits" also finds documents about thermal constraints |
| Misses synonyms and paraphrases | Understands that concepts match even when words differ |
| Returns documents sorted by keyword frequency | Returns passages sorted by conceptual relevance |

When a user asks a question, the same embedding process runs on the question itself, producing a number list. The system then finds the stored passages whose number lists are mathematically closest to the question's number list. Those are the most semantically relevant passages.

---

## An MCP Server vs. a RAG System

This project works alongside the `sharepoint_rest_api-rs` MCP server. Understanding why we need a separate RAG layer — rather than simply using the MCP server directly — is important.

### What the SharePoint MCP Server Does

The `sharepoint_rest_api-rs` server exposes 275 MCP tools that wrap SharePoint's REST API. Tools like `sp_get_file_content`, `sp_search`, and `sp_get_folder_files_recursive` let Claude directly interact with SharePoint as if it were calling the API itself.

This is genuinely useful. Claude can:

- List files in a library
- Download a specific document
- Run a keyword search across SharePoint's built-in search index
- Read metadata about a file or a list item
- Check a user's permissions

### Where the MCP Server Runs Into Limits

The MCP server gives Claude *access* to SharePoint. It does not give Claude *understanding* of the corpus.

Consider this real-world scenario:

> "Summarise everything our company policy documentation says about remote work, travel reimbursement, and expense reporting — and tell me how these policies interact."

To answer this with only the MCP server, Claude would need to:

1. Search SharePoint for "remote work policy" and get back a list of document titles.
2. Download each document — potentially dozens.
3. Read each one in full, inside the context window.
4. Somehow fit all of that text into a single context window alongside the user's question.
5. Reason across all of it to form a coherent answer.

In practice this fails for several reasons:

| Problem | Why it happens |
|---|---|
| **Context overflow** | 30 policy documents × 50 pages each = more text than any context window can hold |
| **Cost and latency** | Downloading and processing 30 documents per query is slow and expensive |
| **Keyword mismatch** | SharePoint's built-in search is keyword-based; it will miss documents that use different terminology |
| **No cross-document synthesis** | Getting results back document-by-document makes it hard to reason about how policies relate to each other |
| **No persistence** | Every query starts from scratch; nothing is learned or pre-processed between conversations |

The SharePoint MCP server is excellent at what it does: it is a precise, reliable, read-safe interface to SharePoint's API. It is not designed to be a semantic search engine over a large pre-indexed corpus.

### What the RAG System Adds

The RAG system runs a separate preparation step — **indexing** — that happens before any user query arrives:

```
INDEXING (runs once, then incrementally on changes)
─────────────────────────────────────────────────────
SharePoint library
    │
    ▼
Connector discovers documents → downloads each file
    │
    ▼
Document loader extracts plain text from PDF / DOCX / XLSX / etc.
    │
    ▼
Chunker splits the text into overlapping passages (~1500 chars each)
    │
    ▼
Embedder converts each passage to a vector (list of numbers)
    │
    ▼
VectorStore saves passage + vector + metadata to PostgreSQL/pgvector
─────────────────────────────────────────────────────

QUERY (runs on every user question, in milliseconds)
─────────────────────────────────────────────────────
User: "What is the policy on remote work expense reimbursement?"
    │
    ▼
Embedder converts the question to a vector
    │
    ▼
VectorStore finds the 10 passages whose vectors are mathematically
closest to the question's vector — across ALL indexed documents
    │
    ▼
Permission filter removes passages the asking user cannot see
    │
    ▼
Claude receives: question + 10 relevant passages + their citations
    │
    ▼
Claude answers the question, citing which document each claim came from
```

The result is that Claude can effectively "know" the contents of your entire SharePoint library and answer questions about it quickly, accurately, and with proper attribution — without downloading any documents at query time.

### Side-by-Side Comparison

| Capability | SharePoint MCP only | SharePoint MCP + RAG |
|---|---|---|
| Find a specific known document | Excellent | Excellent |
| Search by keyword | Good (SharePoint search) | Excellent (semantic) |
| Answer questions that span many documents | Poor (context limits) | Excellent |
| Answer questions where terminology varies | Poor (keyword mismatch) | Excellent |
| Include citations for every claim | Manual | Automatic |
| Respect document-level permissions | Yes (real-time check) | Yes (baked in at index time) |
| Response latency for complex questions | Seconds to minutes | Milliseconds |
| Works without a live SharePoint connection | No | Yes (after indexing) |
| Keeps up with new and changed documents | Real-time | Near-real-time (incremental sync) |

The two systems complement each other. The MCP server is used by the RAG layer to *index* documents. At query time, the RAG system answers from its pre-built index, and can optionally call back to the MCP server to fetch a fresh copy of a specific document if needed.

---

## How the Two Systems Work Together in Practice

Here is the full picture of how `sharepoint_rest_api-rs` and `rust-rag-mcp` work alongside each other:

```
                    ┌──────────────────────────────┐
                    │  Claude / MCP Client          │
                    └──────┬───────────────┬────────┘
                           │               │
            rag_search()   │               │  sp_get_file()
            rag_get_doc()  │               │  sp_search()
                           │               │  (275 tools)
                    ┌──────▼───────┐ ┌─────▼──────────────────┐
                    │  rag-mcp     │ │  sharepoint_rest_api-rs  │
                    │  (this repo) │ │  (companion MCP server)  │
                    └──────┬───────┘ └─────┬──────────────────-┘
                           │               │
                    ┌──────▼───────────────▼──────────────────┐
                    │  rag-core (indexing + retrieval)         │
                    │  rag-connectors uses the SharePoint MCP  │
                    │  client library to crawl and index docs  │
                    └──────────────────────────────────────────┘
```

Claude uses `rag_search` to get semantic search results and `sp_get_file` when it needs the raw bytes of a specific document. Neither system replaces the other; they cover different retrieval patterns.

---

## On Connectors: Why Built-In Instead of Fully Generic?

This is a design question that often comes up: *If the goal is to build a general-purpose RAG library, why build specific SharePoint, filesystem, and S3 connectors into it? Why not just expose a generic plugin interface and let everyone wire up their own source?*

The short answer is: **the Connector trait IS the generic interface**. Anyone can implement it for any source. But built-in connectors exist because a truly useful connector requires deep knowledge of the source system — and a generic lowest-common-denominator interface produces a mediocre experience for every source.

Here is what that means in practice.

### What a Connector Has to Do

The `Connector` trait in `rag-core` defines three operations:

```
list_documents()    → what documents exist in this source?
load_document()     → give me the text content of this document
changes_since(token) → what changed since the last time I checked?
```

This looks simple. For a toy system with five text files, it is simple. For a production SharePoint library with 50,000 documents, each of those operations involves significant source-specific knowledge.

### SharePoint Is Not a Generic File Server

**Document discovery** (`list_documents`) in SharePoint is not just "list files in a folder." It requires understanding:

- The SharePoint library hierarchy (sites → subsites → lists → folders → files)
- Which content types should be indexed (documents vs. list items vs. pages)
- How to handle files that have been checked out by an editor and should not be indexed yet
- How to filter by file extension efficiently at the API level rather than downloading everything

A generic "list files" abstraction would either require the caller to understand all of this, or it would silently miss large classes of content.

**Change tracking** (`changes_since`) is where the gap between generic and specific is largest.

SharePoint has a dedicated `GetChanges` API that returns a structured change feed tagged with a **change token** — a server-side cursor that marks exactly where you left off. If you do not use this API, the only alternative is to re-scan the entire library on every sync run. For a 50,000-document library that runs every 15 minutes, re-scanning is prohibitively expensive.

Implementing `changes_since` correctly for SharePoint means:
- Storing the change token from the previous run
- Calling `GetChanges` with that token
- Mapping each change record (`Created`, `Modified`, `Renamed`, `Deleted`) to a `ChangeEvent`
- Handling the edge cases: deleted files, renamed files, moved folders, version rollbacks

None of this is expressible in a generic "what changed?" interface without losing the efficiency that makes it viable at scale.

**Permissions** are the most consequential source-specific concern. SharePoint has a rich access control model:

- Permissions are inherited down the site hierarchy by default
- Any list, folder, or individual file can break inheritance and have its own access rules
- Permissions are assigned to users, SharePoint groups, and Azure AD security groups
- A user's effective permissions are the union of all group memberships and direct grants

To enforce "only show search results that the person asking can actually read," the RAG system needs to understand this model. The SharePoint connector is the right place to translate SharePoint's ACL model into the permission hints that `rag-core`'s `PermissionFilter` consumes. A generic connector has no way to do this.

### What "Specific Connectors" Actually Means

It does not mean the library is locked to SharePoint. It means:

1. The `Connector` trait is the stable, generic extension point. Any developer can implement it for Confluence, Notion, a SQL database, a Git repository, or any other source. The trait is intentionally minimal: three methods, clear semantics.

2. The built-in connectors — SharePoint, filesystem, S3, Git — are **reference quality implementations** of that trait. They exist because:
   - They are the sources most likely to be used by the organisations this library targets.
   - A high-quality built-in connector, written by people who understand the source system, will always outperform a generic abstraction at indexing accuracy, permission fidelity, and sync efficiency.
   - Having working, tested connectors makes the trait design better. You cannot design a good trait by reasoning in the abstract; you have to implement real cases and let them drive the interface.

3. The extension bus (via Zenoh) lets connectors run as **out-of-process workers**. A team that wants to add a Confluence connector does not need to fork this library. They implement the three methods in Python, register the worker over Zenoh, and the RAG runtime routes requests to them. The specific-vs-generic question becomes: "do you want to maintain the connector yourself, or do you want us to maintain a built-in one for you?"

### Why SharePoint First

SharePoint is the first serious connector because it is where the hard problems live:

- Large documents (PDFs, Word files, Excel spreadsheets — not just text files)
- Complex permissions that real organisations actually enforce
- Efficient incremental sync at scale (tens of thousands of documents)
- The companion `sharepoint_rest_api-rs` library already handles authentication, retries, and rate limiting

Getting SharePoint right proves the connector design. Every decision made to support SharePoint's permission model, change tracking, and file format diversity will make every future connector better.

---

## Summary

| Question | Answer |
|---|---|
| What does RAG do? | Pre-indexes a document corpus into a semantic vector database so that any query can retrieve the most relevant passages in milliseconds |
| Why not just use the SharePoint MCP server? | The MCP server gives Claude access to documents; RAG gives Claude *understanding* of the corpus. For large corpora and complex questions, only the RAG approach is fast, accurate, and scalable |
| Why are specific connectors built in? | The Connector trait is generic and extensible. Built-in connectors exist because real-world sources — especially SharePoint — require deep source-specific knowledge to do change tracking, permissions, and metadata correctly at production scale |
| Can I add my own connector? | Yes. Implement the three-method `Connector` trait in Rust, or run an out-of-process worker in any language via Zenoh |
| Is the MCP server still useful? | Yes. The RAG system uses it to index documents. Claude uses it directly for precise document retrieval when needed. They are complementary |
