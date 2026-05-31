# Query Rewriting and Conversation-Aware Retrieval

Query rewriting, query expansion, and conversation-aware retrieval improve
recall when users ask short, ambiguous, or follow-up questions.

These features should sit above the basic `Retriever` pipeline, not replace it.
The existing retrieval flow remains:

```text
query -> embed query -> vector search -> permission filter -> citation results
```

The enhanced flow adds a query-planning step before retrieval:

```text
conversation + user query
    |
    v
query rewriting / expansion
    |
    v
one or more retrieval queries
    |
    v
merge, dedupe, rerank
    |
    v
citation-ready context
```

---

## Problems Solved

### Short queries

Users often ask:

```text
remote work expenses
```

That may need expansion into:

```text
remote work equipment reimbursement
home office expense policy
travel and remote work expense eligibility
```

### Ambiguous queries

Users may ask:

```text
What is the approval process?
```

The system needs to infer whether this means travel approval, expense approval,
source registration approval, document approval, or something else.

### Follow-up questions

Users may ask:

```text
What about the Q4 version?
```

This only makes sense if the retrieval layer can see recent conversation state,
such as the fact that the previous question was about a Q3 quarterly report.

---

## Query Rewriting

Query rewriting turns the user's raw question into a clearer standalone query.

Input:

```text
What about the Q4 version?
```

Conversation context:

```text
User previously asked about thermal constraints in the Q3 deployment report.
The top cited document was "Thermal Constraints Report Q3".
```

Rewritten query:

```text
thermal constraints Q4 deployment report
```

The rewritten query is usually the one embedded for vector search.

---

## Query Expansion

Query expansion creates multiple semantic variants from one query. This helps
when different documents use different language for the same concept.

Input:

```text
remote work expenses
```

Expanded variants:

```text
remote work expense reimbursement policy
home office equipment reimbursement
telework expenses and employee reimbursement
work from home supplies reimbursement
```

Each variant can be searched independently. Results are then merged, deduped,
and reranked.

---

## Conversation-Aware Retrieval

Conversation-aware retrieval uses recent dialogue to resolve references,
ellipsis, and intent.

It should not blindly dump the full conversation into the search query. Instead,
it should extract only retrieval-relevant state:

- current topic
- entities mentioned
- cited documents already used
- source IDs already searched
- time periods or versions
- unresolved user constraints
- follow-up intent

Example:

```text
Conversation state:
- Topic: thermal constraints
- Source: engineering reports
- Prior cited document: Thermal Constraints Report Q3
- User now asks: "what about the Q4 version?"

Standalone retrieval query:
"thermal constraints Q4 engineering report"
```

---

## Architecture

These features can be implemented as a new retrieval-planning layer.

```text
MCP tool call
    |
    v
QueryPlanner
    |
    | produces RetrievalPlan
    v
Retriever
    |
    | runs one or more searches
    v
ResultMerger
    |
    | dedupe + rerank + token budget
    v
SearchResult[] / context passages
```

Suggested components:

| Component | Responsibility |
|---|---|
| `QueryPlanner` | Rewrite or expand the query using conversation state |
| `RetrievalPlan` | Structured list of searches to run |
| `ResultMerger` | Combine, dedupe, and score results |
| `ConversationContext` | Minimal retrieval-relevant conversation state |
| `QueryRewriteWorker` | Optional external LLM worker for rewriting |

---

## Suggested Data Structures

### ConversationContext

```rust
pub struct ConversationContext {
    pub recent_user_messages: Vec<String>,
    pub recent_assistant_summaries: Vec<String>,
    pub cited_document_ids: Vec<DocumentId>,
    pub cited_source_ids: Vec<SourceId>,
    pub active_topic: Option<String>,
    pub active_time_period: Option<String>,
    pub extra: serde_json::Value,
}
```

This should be smaller than a full chat transcript. It exists to make retrieval
better, not to become long-term memory.

### RetrievalPlan

```rust
pub struct RetrievalPlan {
    pub original_query: String,
    pub rewritten_query: Option<String>,
    pub expanded_queries: Vec<String>,
    pub filters: Option<SearchFilter>,
    pub strategy: RetrievalStrategy,
}
```

### RetrievalStrategy

```rust
pub enum RetrievalStrategy {
    Single,
    Expanded {
        per_query_k: usize,
        final_k: usize,
    },
    ConversationAware {
        per_query_k: usize,
        final_k: usize,
    },
}
```

These structures are illustrative. The implementation should follow the actual
shape of `rag-core` once the feature moves out of the parking lot.

---

## MCP Tool Behavior

The cleanest API is to keep simple tools simple and add optional fields.

### `rag_search`

Potential future inputs:

```json
{
  "query": "what about the Q4 version?",
  "k": 8,
  "caller_context": {},
  "conversation_context": {
    "active_topic": "thermal constraints",
    "cited_document_ids": ["doc_q3_thermal"],
    "recent_user_messages": [
      "What were the thermal constraints in the Q3 deployment?"
    ]
  },
  "rewrite": true,
  "expand": true
}
```

Potential future response metadata:

```json
{
  "original_query": "what about the Q4 version?",
  "rewritten_query": "thermal constraints Q4 deployment report",
  "expanded_queries": [
    "thermal constraints Q4 deployment report",
    "Q4 heat dissipation constraints server deployment",
    "Q4 cooling capacity deployment report"
  ],
  "results": []
}
```

### `rag_get_context`

`rag_get_context` should benefit the most because it already prepares passages
for an LLM prompt. It can include:

- rewritten query
- variants used
- final merged citation list
- note when the system inferred context from conversation

---

## Result Merging

Expansion means multiple searches may return overlapping chunks. The merger
should:

1. dedupe by `chunk_id`
2. preserve the best score per chunk
3. track which query variant found the chunk
4. prefer diversity across documents when scores are close
5. apply permission filtering before final output
6. truncate to the requested `k` or token budget

If a reranker is configured, it should run after dedupe and before final
truncation.

---

## Scoring Strategy

Simple first version:

```text
final_score = max(score from any query variant)
```

Better later version:

```text
final_score =
    best_vector_score
  + small_bonus_for_multiple_query_matches
  + small_bonus_for_recently_cited_document_when follow-up intent is clear
  - penalty_for_low_diversity
```

Scores should remain explainable through `rag_explain_match`.

---

## Worker Options

There are three practical implementation options.

### Rule-based rewriting

Good for the first version of conversation-aware follow-ups:

- detect pronouns and phrases like "what about that", "the Q4 version",
  "same thing for finance"
- combine with active topic and cited documents
- no LLM dependency

### LLM-based rewriting

Better for broad ambiguity:

- produce standalone query
- produce multiple variants
- identify filters such as source, time period, or document type

This can run as an extension worker so `rag-core` stays model-neutral.

### Hybrid approach

Use rules for safe obvious cases and an LLM worker for ambiguous cases.

This is probably the best production shape.

---

## Safety

Query rewriting can improve recall, but it can also change user intent. The
system should keep it transparent and conservative.

Recommended safeguards:

- always retain the original query
- return rewritten and expanded queries in debug metadata
- avoid adding facts not present in the conversation
- do not infer permissions from conversation text
- keep caller permission filtering mandatory
- allow rewriting/expansion to be disabled
- cap the number of query variants
- log rewrite decisions with trace IDs

If the query is high-stakes or ambiguous, the workflow can ask the user a
clarifying question instead of rewriting aggressively.

---

## Evaluation

This feature should be measured before it becomes default behavior.

Useful metrics:

| Metric | Meaning |
|---|---|
| Recall improvement | Did expansion retrieve relevant chunks missing from baseline? |
| Precision loss | Did expansion add too many weak or unrelated chunks? |
| Citation support | Do final citations still support the answer? |
| Follow-up resolution | Did conversation context resolve the user's reference correctly? |
| Latency impact | How much slower are multi-query retrievals? |
| Cost impact | How often does LLM rewriting run? |

Evaluation should compare:

- baseline raw query
- rewritten single query
- expanded multi-query
- conversation-aware rewritten query
- conversation-aware expanded query

---

## Implementation Plan

### Milestone A: Structured request/response metadata

- Add optional query planning metadata to MCP responses.
- Keep behavior identical by default.
- Expose original query, rewritten query, and expanded queries when used.

### Milestone B: Rule-based conversation rewriting

- Add a small `ConversationContext` input to `rag_search` and
  `rag_get_context`.
- Resolve obvious follow-up questions using active topic and cited documents.
- Add integration tests for follow-up questions.

### Milestone C: LLM query rewrite worker

- Add an extension-worker capability for query rewriting.
- Worker returns standalone query, variants, confidence, and reasoning summary.
- Keep the payload provider-neutral.

### Milestone D: Multi-query retrieval and merging

- Run searches for expanded variants.
- Dedupe and merge results.
- Track which query variant matched each result.
- Support optional reranking after merge.

### Milestone E: Evaluation gate

- Add an evaluation harness comparing baseline and expanded retrieval.
- Only enable query expansion by default when it improves recall without
  unacceptable precision loss.

---

## Open Questions

- Should query planning live in `rag-core`, `rag-mcp`, or a new crate?
- Should rewritten queries be visible by default or only in debug mode?
- How much conversation state should MCP clients pass?
- Should expansion happen before or after source/document filters are applied?
- Should the system ask clarifying questions instead of expanding very
  ambiguous queries?
- How should scores from different query variants be normalized?

---

## Summary

Query rewriting makes a user's question clearer. Query expansion searches for
multiple semantic phrasings. Conversation-aware retrieval turns follow-up
questions into standalone retrieval queries.

Together, they improve recall and make RAG feel more natural in real
conversations. The implementation should be conservative, transparent, and
measurable: preserve the original query, expose the variants used, keep
permission filtering mandatory, and evaluate whether expanded retrieval truly
improves citation quality.
