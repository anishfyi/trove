# Retrieval baseline

Measured by `cargo test -p trove-retrieve --test eval -- --nocapture`.
This file is the reference point for every future ranking change: re-run the
harness, paste the new table next to this one, and justify every class-level
delta. Ranking-irrelevant fixture/query edits must be called out separately.

- Run date: 2026-10-23
- Branch base: S4 (`feat/retrieval-evals`), post PR #33
- Query count: 16 (4 exact, 4 synonym, 4 symbol, 4 why)

## Budgets (production defaults, both cited from code)

- **ask**: BM25 search with `SearchOptions::default()` — limit 12, per_kind 6
  (ask carries no ContextBudget; its context budget default is
  `ask::DEFAULT_BUDGET` in crates/trove-cli/src/main.rs).
- **query**: `ContextBudget::for_total(200_000)` — the `trove query`
  production default (`cli.budget.unwrap_or(200_000)`,
  crates/trove-cli/src/main.rs). (Measured identical ranks at 1_500: the
  fixture never comes close to either budget's stop thresholds.)

## Corpus sizes

- ask corpus: 46 docs — 10 Project entries, 3 Doc, 1 Architecture,
  2 Subsystem, 20 Module, 10 Symbol.
- query corpus: 10 Historical objects (imported via `import_historical`).

## Measured table

| system/class | n | recall@1 | recall@3 | recall@5 | MRR | nDCG@5 |
|---|---|---|---|---|---|---|
| ask/exact    | 4 | 0.000 | 1.000 | 1.000 | 0.458 | 0.598 |
| ask/synonym  | 4 | 0.000 | 0.500 | 0.500 | 0.208 | 0.283 |
| ask/symbol   | 4 | 0.000 | 1.000 | 1.000 | 0.417 | 0.565 |
| ask/why      | 4 | 0.000 | 0.250 | 0.750 | 0.227 | 0.329 |
| ask/ALL      | 16 | 0.000 | 0.688 | 0.812 | 0.328 | 0.444 |
| query/exact  | 4 | 0.000 | 0.000 | 0.000 | 0.077 | 0.000 |
| query/synonym | 4 | 0.000 | 0.000 | 0.000 | 0.042 | 0.000 |
| query/symbol | 4 | 0.000 | 0.000 | 0.000 | 0.076 | 0.000 |
| query/why    | 4 | 0.000 | 0.000 | 0.000 | 0.076 | 0.000 |
| query/ALL    | 16 | 0.000 | 0.000 | 0.000 | 0.068 | 0.000 |

Timing (this run): ask 14 ms, query 84 ms (16 queries; run-to-run jitter of a
few ms is normal — ranks and metrics are what matter).

Per-query ranks:
`ask=[Some(2), Some(2), Some(2), Some(3), Some(3), None, Some(2), None, Some(3), Some(3), Some(2), Some(2), Some(4), Some(3), Some(8), Some(5)]`
`query=[Some(14), Some(13), Some(12), Some(13), Some(12), None, Some(12), None, Some(13), Some(13), Some(12), Some(15), Some(14), Some(12), Some(12), Some(15)]`

## Findings this baseline records (no tuning in this slice)

1. **ask recall@1 is 0.000 everywhere.** Expected docs consistently rank
   2nd–3rd: BM25 ties are common in the small fixture and Doc/SectionDoc
   bodies overlap entry text (e.g. the ADR discussing WAL outranks the entry).
2. **Synonym queries halve recall** (0.500 @5): pure lexical matching, as
   expected.
3. **Why-questions are weak** (0.250 @3): no semantic or embedding path.
4. **query puts expected historical items at rank 12–15 every time** — the
   assembled `items` list is level-major (L4→L5), so Historical objects land
   after Architecture/Subsystem/Module/Symbols regardless of score; the two
   synonym queries whose terms never occur in the entry score 0 and do not
   surface at all. The level-ordered output is part of the measured system.
