# neocortex Retrieval Quality Evaluation Suite

## Overview

This evaluation dataset evaluates retrieval quality for **neocortex 0.1.4**, a bi-temporal, graph-augmented long-term memory engine for AI agents. It benchmarks how effectively neocortex retrieves relevant facts, handles evolving information across time, traverses entity knowledge graphs, handles synonyms, and suppresses obsolete knowledge.

The dataset simulates an AI agent accumulating episodic and semantic memories while assisting a software engineer (Alex Mercer) over an 18-month duration (April 2025 through October 2026). The benchmark anchor time is:
```text
now = 2026-10-21T15:00:00Z (Wednesday)
```

## Dataset Composition

- **Total Memories**: 95 items (chronologically sorted by `valid_from` between 2025-04-02 and 2026-10-20).
  - **Structured Memories**: 38 (40.0%) containing explicit `entities` and `relations`.
  - **Plain Text Memories**: 57 (60.0%) simulating unextracted observations or quick agent notes.
- **Memory Kinds**:
  - `fact`: 23
  - `event`: 29
  - `decision`: 5
  - `preference`: 10
  - `mistake`: 6
  - `procedure`: 17
  - `note`: 4
  - `observation`: 1
- **Total Queries**: 84 queries across 11 distinct benchmark categories (each category has >= 7 queries).

---

## File Format

The evaluation data is serialized as a JSON object in `eval/fixtures.json`:

```json
{
  "now": "2026-10-21T15:00:00Z",
  "memories": [
    {
      "key": "m001",
      "content": "Alex's primary email address for professional technical correspondence is alex.mercer.dev@example.org.",
      "kind": "fact",
      "tags": ["contact", "personal"],
      "importance": 0.8,
      "valid_from": "2025-04-02",
      "valid_to": "2025-08-20",
      "entities": [
        {
          "name": "Alex Mercer",
          "type": "person",
          "aliases": ["Alex"],
          "description": "the user"
        }
      ],
      "relations": [
        {
          "source": "Alex Mercer",
          "predicate": "has email",
          "target": "alex.mercer.dev@example.org",
          "fact": "Alex Mercer has primary email alex.mercer.dev@example.org",
          "exclusive": true,
          "valid_from": "2025-04-02"
        }
      ],
      "supersedes": []
    }
  ],
  "queries": [
    {
      "q": "current active email for Alex",
      "category": "current-vs-superseded",
      "expect": ["m020"],
      "expect_absent": ["m001"],
      "as_of": "2026-10-21",
      "include_history": false,
      "k": 5,
      "note": "current email is alex@mercer-labs.net; alex.mercer.dev@example.org superseded"
    }
  ]
}
```

### Memory Schema

| Field | Type | Required | Description |
|---|---|---|---|
| `key` | string | Yes | Unique identifier formatted as `mNNN` (e.g. `m001`). |
| `content` | string | Yes | Atomic 1-2 sentence statement. |
| `kind` | string | Yes | One of: `fact`, `event`, `decision`, `preference`, `mistake`, `procedure`, `observation`, `note`. |
| `tags` | string[] | Yes | Categorical topic tags. |
| `importance` | float | Yes | Salience weighting from `0.0` to `1.0`. |
| `valid_from` | string | Yes | ISO date (`YYYY-MM-DD`) when the fact became true in the real world. |
| `valid_to` | string | No | ISO date (`YYYY-MM-DD`) when the fact ceased being true (half-open `[valid_from, valid_to)`). |
| `entities` | object[] | No | Agent-extracted entities (`name`, `type`, `aliases`, `description`). |
| `relations` | object[] | No | Subject-predicate-object triples with exclusivity and validity anchors. |
| `supersedes` | string[] | No | Keys of older memories that this memory replaces or invalidates. |

### Query Schema

| Field | Type | Required | Default | Description |
|---|---|---|---|---|
| `q` | string | Yes | - | Agent query string (realistic, concise search phrasing). |
| `category` | string | Yes | - | Evaluation category name. |
| `expect` | string[] | Yes | - | Target memory keys that must be retrieved. |
| `expect_absent` | string[] | No | `[]` | Outdated, superseded, or distractor memory keys that must NOT outrank expected memories. |
| `as_of` | string | No | null | Point-in-time date for world-time snapshot queries. |
| `include_history` | bool | No | `false` | Whether expired / superseded memories should be retrieved without penalty. |
| `k` | integer | No | `5` | Retrieval cutoff depth (default 5; 8 for aggregation). |
| `note` | string | Yes | - | Human rationale describing the test objective. |

---

## Category Definitions

1. **`keyword`** (7 queries): Direct lexical overlap queries testing the BM25 full-text index channel.
2. **`paraphrase`** (7 queries): Queries rephrased with zero lexical leakage, requiring dense vector semantic similarity.
3. **`synonym`** (10 queries): Queries testing entity alias resolution and lexical normalization traps:
   - `house` vs `home` vs `apartment`
   - `mom` vs `mother`
   - `dad` vs `father`
   - `bros` vs `friends` vs `buddies`
   - `car` vs `automobile` vs `the Civic`
   - `Postgres` vs `PostgreSQL` vs `pg`
   - `NYC` vs `New York City`
   - `laptop` vs `MacBook` vs `notebook`
   - `doctor` vs `physician` vs `GP`
   - `k8s` vs `Kubernetes`
   - Guard tests against confusable pairs: mother vs guardian aunt, London vs Paris, Postgres vs MySQL, brother vs cousin.
4. **`multi-hop`** (9 queries): Queries where the answer memory shares no lexical overlap with the query and is accessible only through graph traversal (HippoRAG 2 style personalized PageRank across entities and relations).
5. **`temporal-window`** (8 queries): Queries containing relative temporal expressions anchored at `now` (Wednesday, 2026-10-21):
   - `"yesterday"` (2026-10-20)
   - `"last week"` (week ending before 2026-10-18)
   - `"two weeks ago"` (~2026-10-07)
   - `"last month"` (September 2026)
   - `"in March"` (March 2026)
   - `"this year"` (2026)
   - `"in 2025"` (calendar year 2025)
   - `"since August"` (2026-08-01 onwards)
6. **`as-of`** (7 queries): Historical point-in-time world state retrieval using `as_of = "YYYY-MM-DD"`. Only memories whose validity interval `[valid_from, valid_to)` contains the specified date should be retrieved.
7. **`current-vs-superseded`** (8 queries): Queries testing present-day state where old facts were superseded by new events across 6 distinct evolution chains (city moves, job changes, database migrations, beverage preferences, phone/email contact updates, project deadlines). Obsolete memories appear in `expect_absent`.
8. **`history`** (7 queries): Historical recollection queries (`include_history: true` or past-tense indicators like "used to", "previously", "prior") requiring retrieval of superseded facts.
9. **`preference`** (7 queries): Retrieval of persistent habits, workspace setups, and technical conventions.
10. **`procedure/lesson`** (7 queries): Operational instructions, troubleshooting procedures, and postmortem mistakes ("what went wrong" and "how to prevent").
11. **`aggregation`** (7 queries): Broad topical retrieval over sets with `k: 8` measuring high-capacity multi-item recall.

---

## Scoring Rules

### 1. Recall@k
For each query $q$:
$$\text{Recall@k} = \frac{|\text{expect} \cap \text{top-k}|}{|\text{expect}|}$$

### 2. Mean Reciprocal Rank (MRR)
MRR measures how early the first relevant expected memory appears in the ranked results:
$$\text{RR} = \begin{cases} \frac{1}{\text{rank}_{\min}} & \text{if } \exists m \in \text{expect} \text{ in top-}k \\ 0 & \text{otherwise} \end{cases}$$
$$\text{MRR} = \frac{1}{|Q|} \sum_{q \in Q} \text{RR}(q)$$
where $\text{rank}_{\min}$ is the 1-based index of the highest-ranked expected item.

### 3. Superseded / Invalidation Constraint (`expect_absent`)
If a query defines `expect_absent`:
- Let $\text{rank}_{\min}(\text{expect})$ be the rank of the first expected item retrieved.
- Let $\text{rank}_{\min}(\text{expect\_absent})$ be the rank of the first absent item retrieved.
- If $\text{rank}_{\min}(\text{expect\_absent}) \le \text{rank}_{\min}(\text{expect})$ (i.e. an invalid or superseded memory outranks the correct expected memory), the query **fails immediately**:
$$\text{Recall@k} = 0, \quad \text{RR} = 0$$

### 4. Overall & Per-Category Reporting Tables
The benchmark driver (`neocortex eval`) generates summary tables:
- **Per-Category**: Breakdown of total queries, Mean Recall@k, and MRR.
- **Overall**: Aggregated Mean Recall@k and Mean MRR across the entire benchmark. Target: Overall Recall@5 $\ge 0.90$.

### 5. Channel Ablation Protocol
To measure the marginal contribution of each retrieval channel, the benchmark is executed across 5 configurations:
1. **Full Fusion**: $w_{\text{bm25}} = 1.0, w_{\text{vec}} = 1.0, w_{\text{graph}} = 1.0, w_{\text{temporal}} = 1.0$.
2. **Ablate BM25**: $w_{\text{bm25}} = 0.0$.
3. **Ablate Dense Vector**: $w_{\text{vec}} = 0.0$.
4. **Ablate Graph (PPR)**: $w_{\text{graph}} = 0.0$.
5. **Ablate Temporal**: $w_{\text{temporal}} = 0.0$.

Each ablation run must demonstrate either an expected drop in performance for its target categories (e.g. graph ablation dropping multi-hop recall; temporal ablation dropping temporal-window recall) or provide empirical justification for channel tuning.
