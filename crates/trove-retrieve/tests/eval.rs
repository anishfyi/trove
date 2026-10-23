//! Retrieval eval harness: baseline measurement of the two rankers over the
//! committed fixture in `evals/fixture` with the golden queries in
//! `evals/queries.json`. Measurement only — nothing here tunes ranking.
//!
//! Two systems, measured independently on the same queries:
//! - `ask`: BM25 over Docs, built exactly like `trove ask` builds its corpus
//!   (`trove_entries` + `repo_docs` + `engine_index` from sources.rs).
//! - `query`: the RetrievalPipeline (level-major assembly, substring scoring
//!   over MemoryObjects), seeded by importing the fixture trove entries as
//!   Historical objects via the production `import_historical` path.
//!
//! Metrics (see evals/README.md for the formulas):
//! rank (1-based, miss = none), recall@1/3/5, MRR, nDCG@5 (binary, single
//! relevant item). The test FAILS on harness/fixture breakage (empty corpora,
//! missing expected items, out-of-range metrics, exact-term ask recall@5
//! below 1.0) and never on honest weak scores.
//!
//! Run: cargo test -p trove-retrieve --test eval -- --nocapture

use std::path::{Path, PathBuf};
use std::time::Instant;

use trove_core::object::MemoryKind;
use trove_core::store::TroveStore;
use trove_index::{IndexOptions, Indexer};
use trove_retrieve::ask::{Corpus, SearchOptions, SourceKind};
use trove_retrieve::budget::ContextBudget;
use trove_retrieve::pipeline::{import_historical, RetrievalPipeline};
use trove_retrieve::sources::{engine_index, repo_docs, trove_entries};

struct GoldenQuery {
    id: String,
    class: String,
    query: String,
    ask_expected: String,
    query_expected: String,
}

struct ClassMetrics {
    n: usize,
    recall1: f64,
    recall3: f64,
    recall5: f64,
    mrr: f64,
    ndcg5: f64,
}

impl ClassMetrics {
    fn compute(ranks: &[Option<usize>]) -> ClassMetrics {
        let n = ranks.len() as f64;
        let mut m = ClassMetrics {
            n: ranks.len(),
            recall1: 0.0,
            recall3: 0.0,
            recall5: 0.0,
            mrr: 0.0,
            ndcg5: 0.0,
        };
        for r in ranks.iter().flatten() {
            let r = *r;
            m.recall1 += (r <= 1) as usize as f64;
            m.recall3 += (r <= 3) as usize as f64;
            m.recall5 += (r <= 5) as usize as f64;
            m.mrr += 1.0 / r as f64;
            if r <= 5 {
                // Binary relevance, single relevant item: DCG = 1/log2(r+1), IDCG = 1.
                m.ndcg5 += 1.0 / (r as f64 + 1.0).log2();
            }
        }
        m.recall1 /= n;
        m.recall3 /= n;
        m.recall5 /= n;
        m.mrr /= n;
        m.ndcg5 /= n;
        m
    }
}

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/fixture")
}

fn queries_path() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/queries.json")
}

/// Recursively copy the committed fixture into a temp dir so the indexer and
/// store writes never touch the repo copy.
fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap().flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn load_queries() -> Vec<GoldenQuery> {
    let raw = std::fs::read_to_string(queries_path()).expect("evals/queries.json is committed");
    let root: serde_json::Value = serde_json::from_str(&raw).expect("queries.json parses");
    let queries = root["queries"].as_array().expect("queries array");
    let out: Vec<GoldenQuery> = queries
        .iter()
        .map(|q| GoldenQuery {
            id: q["id"].as_str().unwrap().to_string(),
            class: q["class"].as_str().unwrap().to_string(),
            query: q["query"].as_str().unwrap().to_string(),
            ask_expected: q["ask_expected"].as_str().unwrap().to_string(),
            query_expected: q["query_expected"].as_str().unwrap().to_string(),
        })
        .collect();
    assert!(
        out.len() >= 16,
        "fixture must carry at least 16 golden queries"
    );
    for class in ["exact", "synonym", "symbol", "why"] {
        assert!(
            out.iter().filter(|q| q.class == class).count() >= 4,
            "class {class} needs at least 4 queries"
        );
    }
    out
}

/// Lowercased whole-word tokens of length >= 4 (same granularity the BM25
/// tokenizer works at) — used only to prove the synonym class actually avoids
/// its queries' literal terms.
fn tokens(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() >= 4)
        .map(str::to_string)
        .collect()
}

#[test]
fn retrieval_eval_baseline() {
    let queries = load_queries();

    // --- Real production loading, on a throwaway copy of the fixture -------
    let tmp = std::env::temp_dir().join(format!("trove-eval-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&fixture_root(), &tmp.join("fixture"));
    let repo = tmp.join("fixture");

    // Index the fixture with the real indexer (writes .trove: symbols.json,
    // manifest.json, Module objects), exactly like `trove index`.
    Indexer::open(&repo)
        .expect("indexer opens the fixture")
        .index(IndexOptions::default())
        .expect("indexer runs");

    // ask corpus: the exact loader set `trove ask` uses.
    let display = |p: &Path| {
        p.strip_prefix(&repo)
            .unwrap_or(p)
            .to_string_lossy()
            .into_owned()
    };
    let mut docs = trove_entries(&repo.join(".claude/trove"), SourceKind::Project, &display);
    docs.extend(repo_docs(&repo));
    docs.extend(engine_index(&repo));
    let ask_cites: Vec<String> = docs.iter().map(|d| d.path.clone()).collect();
    let corpus = Corpus::new(docs);
    let counts = corpus.counts();
    println!("ask corpus: {} docs ({counts:?})", corpus.len());

    // query corpus: import the fixture entries as Historical memories through
    // the production import path, over the store the indexer just created.
    let store = TroveStore::open(&repo).expect("store opens");
    let imported = import_historical(&store, &repo.join(".claude/trove")).expect("entries import");
    let historicals = store.read_objects(MemoryKind::Historical).unwrap();
    println!("query corpus: {imported} historical objects imported");
    assert_eq!(imported, 10, "all ten fixture entries import");
    assert_eq!(historicals.len(), 10);

    // --- Fixture sanity: every expected identifier really exists -----------
    for q in &queries {
        assert!(
            ask_cites.iter().any(|c| cite_matches(c, &q.ask_expected)),
            "{}: expected ask item {:?} is not in the corpus — fixture/harness mismatch",
            q.id,
            q.ask_expected
        );
        assert!(
            historicals.iter().any(|o| o.id == q.query_expected),
            "{}: expected query item {:?} was not imported — fixture/harness mismatch",
            q.id,
            q.query_expected
        );
    }

    // The synonym class must prove something: no query token may appear in
    // the expected entry's own text (so a hit cannot be literal matching).
    for q in queries.iter().filter(|q| q.class == "synonym") {
        let text = std::fs::read_to_string(fixture_root().join(&q.ask_expected))
            .expect("expected entry file exists in fixture");
        let entry_tokens = tokens(&text);
        for t in tokens(&q.query) {
            assert!(
                !entry_tokens.contains(&t),
                "{}: synonym fixture leak: expected entry contains query token {t:?}",
                q.id
            );
        }
    }

    // --- Measurement --------------------------------------------------------
    let mut ask_ranks = Vec::new();
    let mut query_ranks = Vec::new();
    let t_ask = Instant::now();
    for q in &queries {
        let hits = corpus.search(&q.query, &SearchOptions::default());
        ask_ranks.push(
            hits.iter()
                .position(|h| cite_matches(&h.cite, &q.ask_expected))
                .map(|i| i + 1),
        );
    }
    let ask_ms = t_ask.elapsed().as_millis();

    let pipeline = RetrievalPipeline::new(store);
    let t_query = Instant::now();
    for q in &queries {
        // Production budget: `trove query` defaults to 200_000 tokens
        // (crates/trove-cli/src/main.rs: `cli.budget.unwrap_or(200_000)`).
        // 1_500 is `ask`'s world, not `query`'s.
        let ctx = pipeline
            .query(&q.query, ContextBudget::for_total(200_000))
            .expect("pipeline query runs");
        query_ranks.push(
            ctx.items
                .iter()
                .position(|i| i.object_id == q.query_expected)
                .map(|i| i + 1),
        );
    }
    let query_ms = t_query.elapsed().as_millis();

    // --- Table ---------------------------------------------------------------
    println!();
    println!("Retrieval baseline (n queries, ranks over the systems' own output lists)");
    println!(
        "{:<9} {:>3}  {:>9} {:>9} {:>9} {:>7} {:>8}",
        "system/class", "n", "recall@1", "recall@3", "recall@5", "MRR", "nDCG@5"
    );
    for (system, ranks) in [("ask", &ask_ranks), ("query", &query_ranks)] {
        for class in ["exact", "synonym", "symbol", "why"] {
            let class_ranks: Vec<Option<usize>> = queries
                .iter()
                .zip(ranks.iter())
                .filter(|(q, _)| q.class == class)
                .map(|(_, r)| *r)
                .collect();
            let m = ClassMetrics::compute(&class_ranks);
            println!(
                "{:<9} {:>3}  {:>9.3} {:>9.3} {:>9.3} {:>7.3} {:>8.3}",
                format!("{system}/{class}"),
                m.n,
                m.recall1,
                m.recall3,
                m.recall5,
                m.mrr,
                m.ndcg5
            );
        }
        let m = ClassMetrics::compute(ranks);
        println!(
            "{:<9} {:>3}  {:>9.3} {:>9.3} {:>9.3} {:>7.3} {:>8.3}",
            format!("{system}/ALL"),
            m.n,
            m.recall1,
            m.recall3,
            m.recall5,
            m.mrr,
            m.ndcg5
        );
    }
    println!(
        "timing: ask {ask_ms} ms, query {query_ms} ms ({} queries)",
        queries.len()
    );
    println!("per-query ranks: ask={ask_ranks:?} query={query_ranks:?}");
    println!();

    // --- Harness assertions (fixture health, not score floors) ---------------
    for (label, ranks) in [("ask", &ask_ranks), ("query", &query_ranks)] {
        let m = ClassMetrics::compute(ranks);
        for (name, v) in [
            ("recall@1", m.recall1),
            ("recall@3", m.recall3),
            ("recall@5", m.recall5),
            ("MRR", m.mrr),
            ("nDCG@5", m.ndcg5),
        ] {
            assert!((0.0..=1.0).contains(&v), "{label} {name}={v} out of range");
        }
    }
    let exact_ask: Vec<Option<usize>> = queries
        .iter()
        .zip(ask_ranks.iter())
        .filter(|(q, _)| q.class == "exact")
        .map(|(_, r)| *r)
        .collect();
    let m = ClassMetrics::compute(&exact_ask);
    assert_eq!(m.recall5, 1.0, "ask exact-term recall@5 must be 1.0 — if not, the harness or fixture is wrong, not the ranker");

    let _ = std::fs::remove_dir_all(&tmp);
}

fn cite_matches(cite: &str, expected: &str) -> bool {
    cite == expected
        || cite.starts_with(&format!("{expected}#"))
        || cite.starts_with(&format!("{expected}/"))
}
