# Using rust-rag-mcp in Larger Systems

`rust-rag-mcp` is designed to be the reliable retrieval layer inside larger
AI systems, not the whole agent framework by itself. The project indexes
enterprise documents, enforces retrieval-time permissions, and returns
citation-ready passages through a small MCP or HTTP-facing tool surface.

That makes it useful as a stable foundation for systems that need LLMs to
reason over private corpora without giving the LLM direct responsibility for
crawling, parsing, indexing, permission checks, or vector storage.

The larger system can be a LangGraph workflow, a custom Python service, a CI
evaluation harness, a human approval queue, a Claude Desktop workflow, or an
internal agent platform. In each case, `rust-rag-mcp` should stay focused on
search, context, citations, indexing, and source state.

---

## System Role

In a larger architecture, this project sits between source systems and agentic
or workflow code:

```text
SharePoint / filesystem / S3 / Git
        |
        v
rust-rag-mcp
  - connector sync
  - document parsing via extension workers
  - chunking and embedding
  - pgvector storage
  - permission-aware retrieval
  - citation-ready MCP/HTTP tools
        |
        v
larger application
  - evaluation workflow
  - human approval workflow
  - agentic research workflow
  - reporting or compliance workflow
```

The important boundary is this:

| Layer | Owns |
|---|---|
| `rust-rag-mcp` | Indexing, sync, embeddings, vector search, citations, permission filtering, source metadata |
| Larger workflow | Goals, policy, retries, comparison between strategies, human review, final generation |

This keeps the retrieval engine deterministic and testable while allowing
agent frameworks to remain replaceable.

---

## Integration Surfaces

Larger projects should integrate through one of three surfaces.

### MCP tools

The MCP layer is the best fit for AI assistants and agent hosts. A workflow can
call:

| Tool | Larger-system use |
|---|---|
| `rag_search` | Retrieve ranked passages for a query |
| `rag_get_context` | Fetch prompt-ready context with citations and token budgeting |
| `rag_get_document` | Inspect a full document after a search hit |
| `rag_list_sources` | Show source health before running a workflow |
| `rag_index_source` | Trigger a full index after approval |
| `rag_sync_source` | Trigger an incremental sync after approval |
| `rag_explain_match` | Debug why a passage was returned |

### HTTP API

The optional HTTP layer is the best fit for non-MCP services, CI jobs, batch
evaluation, dashboards, and existing REST-based applications.

### Rust library API

Rust applications can use `rag-core` directly when they want to embed retrieval
inside the same process. This is useful for tests, small internal tools, and
custom services that do not need MCP or HTTP.

---

## Pattern 1: Evaluation Workflows

Evaluation workflows measure whether retrieval quality is good enough for a
specific corpus and use case. They are especially useful before changing
chunking, embedding models, search filters, reranking, or permissions.

The larger workflow owns the experiment. `rust-rag-mcp` owns each retrieval
run.

### Typical Flow

```text
1. Select or generate evaluation questions.
2. For each question, call rag_search or rag_get_context.
3. Ask an evaluator to judge whether the returned passages are relevant.
4. Check whether citations support the expected answer.
5. Retry with alternate retrieval settings.
6. Compare runs and store metrics.
```

The evaluator can be a human reviewer, an LLM judge, or a mixed approach where
an LLM scores the first pass and humans review low-confidence cases.

### What to Measure

| Metric | Meaning |
|---|---|
| Recall at k | Did the right source appear in the top k results? |
| Citation support | Does the cited passage actually support the answer? |
| Answerability | Is there enough retrieved context to answer the question? |
| Permission correctness | Were restricted documents excluded for the caller? |
| Latency | How long did retrieval take at the chosen k and filters? |
| Stability | Do small query changes return consistent supporting sources? |

### Example Evaluation Loop

```text
for each question in eval_set:
    baseline = rag_get_context(query=question, k=8)
    expanded = rag_get_context(query=expanded_question, k=12)

    baseline_judgement = llm_judge(question, baseline.context, baseline.citations)
    expanded_judgement = llm_judge(question, expanded.context, expanded.citations)

    store_result({
        question,
        baseline_citations,
        expanded_citations,
        baseline_score,
        expanded_score,
        latency_ms,
    })
```

The retrieval settings can vary by:

- `k`
- source filters
- document filters
- content-type filters
- embedding model
- chunk size and overlap
- reranker enabled or disabled
- query rewriting enabled or disabled in the outer workflow

### Why This Belongs Outside the Core

Evaluation is inherently experimental. Different teams will want different
judges, rubrics, datasets, and pass/fail thresholds. Keeping it outside
`rag-core` avoids turning the retrieval engine into a lab notebook. The core
only needs to expose enough metadata for repeatable measurement: query, source
IDs, chunk IDs, scores, snippets, citations, and timing.

---

## Pattern 2: Human Approval Workflows

Some actions should not be performed just because an agent or automation script
decided they are useful. Full re-indexing, source registration, source deletion,
corpus migration, and permission model changes can be expensive or risky.

In these cases, `rust-rag-mcp` should expose the action, but the larger system
should own the approval policy.

### Risky Actions

| Action | Risk |
|---|---|
| Registering a new source | May index sensitive or irrelevant content |
| Full source indexing | Can create high API load and embedding cost |
| Incremental sync | Usually safer, but still touches external systems |
| Deleting a source | Can remove searchable knowledge unexpectedly |
| Re-indexing after embedding model change | Can invalidate previous vectors and metrics |
| Corpus migration | Can change citation IDs, source URLs, or access assumptions |

### Typical Flow

```text
1. Agent or operator proposes an action.
2. Workflow calls rag_list_sources and estimates impact.
3. Workflow prepares an approval request.
4. Human approves, rejects, or modifies the request.
5. Workflow calls rag_index_source or rag_sync_source if approved.
6. Workflow records the decision and resulting job status.
```

### Approval Request Contents

A useful approval request should include:

- source ID and source name
- action type
- estimated document count
- estimated embedding cost if known
- expected permission model
- whether the action is read-only or mutating
- whether it can be safely retried
- previous sync time or change token
- operator or agent that requested the action

### Read-Only Mode

`RAG_READ_ONLY=true` should be used when the RAG server is connected to an
assistant that should only search and read. In that mode, mutation tools such as
`rag_index_source` and `rag_sync_source` should return descriptive errors.

An approval service can run a separate privileged instance or temporarily issue
approved mutation calls through a controlled channel.

### Why This Belongs Outside the Core

Approval policies are organization-specific. One team may allow incremental
syncs freely but require approval for full indexing. Another may require legal
review before indexing a new SharePoint site. The RAG service should enforce
basic safety switches and clear errors; the larger system should decide who is
allowed to approve what.

---

## Pattern 3: Agentic Research Flows

Agentic research workflows use an assistant to investigate a question over
multiple search and reading steps. The assistant does not receive the entire
corpus. Instead, it repeatedly asks for relevant context, inspects citations,
and refines its next query.

This pattern is useful for reports, policy analysis, incident review,
compliance summaries, engineering research, and cross-document synthesis.

### Typical Flow

```text
1. User asks a broad research question.
2. Agent decomposes it into subquestions.
3. For each subquestion, agent calls rag_search or rag_get_context.
4. Agent reads citations and identifies gaps.
5. Agent asks follow-up retrieval questions.
6. Agent optionally calls rag_get_document for key sources.
7. Agent synthesizes a final answer with citations.
8. Agent records which sources were used.
```

### Example

User question:

```text
How do our remote work, travel, and expense policies interact for employees
who attend an out-of-state customer workshop?
```

Possible agent plan:

```text
1. Search for remote work eligibility and location restrictions.
2. Search for travel authorization requirements.
3. Search for expense reimbursement rules.
4. Search for customer workshop or client-site attendance language.
5. Compare dates and policy versions.
6. Produce a cited answer that separates facts from uncertainty.
```

The agent framework owns decomposition and synthesis. `rust-rag-mcp` owns the
evidence retrieval.

### Guardrails for Research Agents

Research agents should follow a few rules:

- Treat retrieved passages as evidence, not as final answers.
- Prefer `rag_get_context` for prompt-ready evidence.
- Use `rag_get_document` only when the full source is needed.
- Preserve citation labels and URLs in the final response.
- Distinguish supported claims from inferred claims.
- Re-query when top results are weak, stale, or contradictory.
- Pass caller context so permission filtering can be enforced.

### Where LangGraph Fits

LangGraph or a similar orchestration framework can be useful above
`rust-rag-mcp` when the research process has explicit state:

- current question
- subquestions already answered
- citations already collected
- unresolved gaps
- human review checkpoints
- final report draft

The graph should call RAG tools as external tools. It should not replace
`rag-core`, `PgVectorStore`, connector sync, or permission filtering.

---

## Recommended Boundaries

The safest design is to make `rust-rag-mcp` boring in the best possible way:
predictable inputs, predictable outputs, clear errors, and strong metadata.

Do this in `rust-rag-mcp`:

- expose search and context tools
- return structured citations
- preserve chunk IDs and document IDs
- enforce caller permissions
- validate configuration at startup
- record source and indexing status
- emit traces and metrics
- make mutation tools easy to block

Do this in the larger system:

- decide what question to ask next
- compare retrieval strategies
- ask LLMs or humans to judge quality
- require approvals for risky actions
- maintain workflow state
- generate final reports
- manage user-facing UX

---

## Design Implications for This Project

These larger workflows do not require `rust-rag-mcp` to become an agent
framework. They do imply a few useful product requirements:

1. Tool responses should be structured, not only prose.
2. Every search result should include stable IDs, source IDs, scores, snippets,
   citation labels, URLs, page numbers, and modified timestamps when available.
3. Indexing and sync tools should return job IDs or status records, not just
   success strings.
4. `rag_explain_match` should expose enough detail for evaluation and debugging.
5. Read-only mode should be easy to enable for assistant-facing deployments.
6. Observability should include trace IDs across MCP calls, retrieval, store
   queries, connector calls, and extension-worker calls.
7. Permission filtering should be testable with explicit caller contexts.
8. HTTP endpoints should mirror MCP tools so batch workflows do not need an MCP
   client.

These requirements support larger systems while keeping the core architecture
small and durable.

---

## Summary

`rust-rag-mcp` is most valuable as a trustworthy retrieval substrate. Larger
systems can use it to ground LLMs in private documents, but they should keep
their own workflow state, approval rules, evaluation loops, and synthesis logic.

That division lets the project serve many kinds of higher-level applications:

- evaluation harnesses that measure retrieval and citation quality
- approval workflows that control indexing and migration risk
- research agents that search, read, refine, and synthesize with citations
- internal services that need semantic document search without adopting a
  specific agent framework

The retrieval engine remains Rust-first, permission-aware, and citation-ready.
The surrounding application remains free to use whatever orchestration model
fits the job.
