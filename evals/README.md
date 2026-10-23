# Retrieval evals

Baseline measurement of Trove's two retrieval paths. **Measurement only**:
nothing here tunes ranking. Every future ranking change reports its delta
against the table below (re-run the harness, paste both tables).

## What is measured

The same 16 golden queries (`queries.json`, 4 per class) run through both
systems, each fed by the real production loaders over the committed fixture
(`fixture/`, a miniature JS/Python service with docs, ADRs and a sectioned
trove; no secrets, all content invented):

- **ask** — BM25 over Docs, the corpus `trove ask` builds:
  `trove_entries` (project trove) + `repo_docs` + `engine_index` (a real
  `Indexer` run in a temp copy of the fixture).
- **query** — the `RetrievalPipeline` (`trove query`): level-major context
  assembly with substring scoring over MemoryObjects, seeded by importing the
  fixture trove entries as Historical objects via `import_historical`.

Query classes:

| class | proves |
|---|---|
| `exact` | literal term overlap retrieves (control) |
| `synonym` | retrieval without literal overlap — the harness asserts the expected entry contains **no** whole-word query token ≥4 chars |
| `symbol` | code identifiers retrieve their definitions |
| `why` | natural questions (no tuning target exists for these today) |

Each query names its expected item per system: `ask_expected` (corpus doc
path) and `query_expected` (memory object id, `historical:<slug>`).

## Run

```bash
cargo test -p trove-retrieve --test eval -- --nocapture
```

~1 s. The test fails on fixture/harness breakage (missing expected items,
out-of-range metrics, `ask` exact-term recall@5 below 1.0) and never on
honest weak scores.

## Metrics

`r` = 1-based rank of the expected item in the system's own output list
(`ask`: search hits; `query`: assembled `items`, which is level-ordered —
that ordering is part of what is being measured). A miss contributes nothing
to recall/MRR/nDCG.

- recall@k = 1 if r ≤ k (per class mean)
- MRR = mean of 1/r
- nDCG@5 = mean of 1/log2(r+1) for r ≤ 5 else 0 (binary relevance, single
  relevant item, so IDCG = 1)

## Baseline

The measured baseline lives in [BASELINE.md](BASELINE.md) (table, corpus
sizes, timing, per-query ranks, findings). Budgets used, both production
defaults: `ask` runs BM25 under `SearchOptions::default()` (limit 12,
per_kind 6; no ContextBudget — the CLI default there is `ask::DEFAULT_BUDGET`),
`query` runs at `ContextBudget::for_total(200_000)`, the `trove query`
default (`cli.budget.unwrap_or(200_000)` in crates/trove-cli/src/main.rs).
Every ranking change re-runs the harness and reports its delta against that
file.
