//! Question answering over everything Trove knows: personal and project
//! troves, repo docs, the engine index and the tool catalog. Retrieval is
//! fully offline: BM25 over a small in-memory corpus, with title and
//! coverage boosts and a per-source prior.

use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::budget::ContextBudget;

/// Where a document came from. Order is display order for counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SourceKind {
    Personal,
    Project,
    Doc,
    Architecture,
    Subsystem,
    Module,
    Symbol,
    Tool,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Personal => "personal",
            Self::Project => "project",
            Self::Doc => "doc",
            Self::Architecture => "arch",
            Self::Subsystem => "subsystem",
            Self::Module => "module",
            Self::Symbol => "symbol",
            Self::Tool => "tool",
        }
    }

    /// Curated memory is trusted a little more than generated summaries;
    /// tools are plentiful and short, so they are damped.
    fn prior(self) -> f32 {
        match self {
            Self::Personal | Self::Project => 1.25,
            Self::Doc => 1.1,
            Self::Architecture | Self::Subsystem | Self::Module => 1.0,
            Self::Symbol => 0.9,
            Self::Tool => 0.6,
        }
    }
}

/// One retrievable unit.
#[derive(Debug, Clone, Serialize)]
pub struct Doc {
    pub kind: SourceKind,
    /// File path (repo-relative when inside the repo) or tool path.
    pub path: String,
    /// Line the document starts at, when it maps to a file location.
    pub line: Option<usize>,
    pub title: String,
    pub text: String,
    /// `text` is a verbatim slice of the file starting at `line`, so a
    /// snippet's line can be cited exactly.
    #[serde(skip)]
    pub verbatim: bool,
    /// Per-document multiplier on top of the kind prior.
    #[serde(skip)]
    pub weight: f32,
    /// Extra searchable words that never show up in snippets.
    #[serde(skip)]
    pub keywords: String,
}

impl Doc {
    pub fn new(kind: SourceKind, path: impl Into<String>, title: impl Into<String>) -> Self {
        Self {
            kind,
            path: path.into(),
            line: None,
            title: title.into(),
            text: String::new(),
            verbatim: false,
            weight: 1.0,
            keywords: String::new(),
        }
    }

    pub fn with_keywords(mut self, keywords: impl Into<String>) -> Self {
        self.keywords = keywords.into();
        self
    }

    pub fn weighted(mut self, weight: f32) -> Self {
        self.weight = weight;
        self
    }

    pub fn with_text(mut self, text: impl Into<String>) -> Self {
        self.text = text.into();
        self
    }

    pub fn at_line(mut self, line: usize, verbatim: bool) -> Self {
        self.line = Some(line);
        self.verbatim = verbatim;
        self
    }
}

/// A ranked result with a citation and the most relevant line.
#[derive(Debug, Clone, Serialize)]
pub struct Hit {
    pub kind: SourceKind,
    pub cite: String,
    pub title: String,
    pub snippet: String,
    pub score: f32,
    #[serde(skip)]
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SearchOptions {
    pub limit: usize,
    /// Max hits from any one kind, so one source cannot crowd out the rest.
    pub per_kind: usize,
    /// Tighter cap for tools.
    pub tools: usize,
}

impl Default for SearchOptions {
    fn default() -> Self {
        Self {
            limit: 12,
            per_kind: 6,
            tools: 3,
        }
    }
}

const K1: f32 = 1.2;
const B: f32 = 0.75;
const TITLE_WEIGHT: f32 = 1.5;
const SNIPPET_CHARS: usize = 200;

pub struct Corpus {
    docs: Vec<Doc>,
    tf: Vec<HashMap<String, u32>>,
    lens: Vec<u32>,
    titles: Vec<HashSet<String>>,
    df: HashMap<String, u32>,
    avg_len: f32,
}

impl Corpus {
    pub fn new(docs: Vec<Doc>) -> Self {
        let mut tf = Vec::with_capacity(docs.len());
        let mut lens = Vec::with_capacity(docs.len());
        let mut titles = Vec::with_capacity(docs.len());
        let mut df: HashMap<String, u32> = HashMap::new();
        for doc in &docs {
            let title: HashSet<String> = tokenize(&doc.title).into_iter().collect();
            let mut counts: HashMap<String, u32> = HashMap::new();
            let mut tokens = tokenize(&doc.text);
            tokens.extend(tokenize(&doc.keywords));
            lens.push((tokens.len() + title.len()) as u32);
            for t in tokens.into_iter().chain(title.iter().cloned()) {
                *counts.entry(t).or_default() += 1;
            }
            for t in counts.keys() {
                *df.entry(t.clone()).or_default() += 1;
            }
            tf.push(counts);
            titles.push(title);
        }
        let avg_len = if lens.is_empty() {
            1.0
        } else {
            (lens.iter().map(|&l| l as f32).sum::<f32>() / lens.len() as f32).max(1.0)
        };
        Self {
            docs,
            tf,
            lens,
            titles,
            df,
            avg_len,
        }
    }

    pub fn len(&self) -> usize {
        self.docs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.docs.is_empty()
    }

    pub fn counts(&self) -> Vec<(SourceKind, usize)> {
        let mut m: HashMap<SourceKind, usize> = HashMap::new();
        for d in &self.docs {
            *m.entry(d.kind).or_default() += 1;
        }
        let mut v: Vec<_> = m.into_iter().collect();
        v.sort();
        v
    }

    fn idf(&self, term: &str) -> f32 {
        let n = self.docs.len() as f32;
        let df = *self.df.get(term).unwrap_or(&0) as f32;
        (1.0 + (n - df + 0.5) / (df + 0.5)).ln()
    }

    /// Score one document; 0 when no query term matches.
    pub fn score(&self, i: usize, terms: &[String]) -> f32 {
        let tf = &self.tf[i];
        let len_norm = 1.0 - B + B * self.lens[i] as f32 / self.avg_len;
        let mut score = 0.0;
        let mut matched = 0usize;
        for t in terms {
            let Some(&f) = tf.get(t) else { continue };
            matched += 1;
            let idf = self.idf(t);
            let f = f as f32;
            score += idf * f * (K1 + 1.0) / (f + K1 * len_norm);
            if self.titles[i].contains(t) {
                score += idf * TITLE_WEIGHT;
            }
        }
        if matched == 0 {
            return 0.0;
        }
        let coverage = matched as f32 / terms.len() as f32;
        score * (0.4 + 0.6 * coverage) * self.docs[i].kind.prior() * self.docs[i].weight
    }

    /// Total number of documents matching at least one query term.
    pub fn match_count(&self, query: &str) -> usize {
        let terms = query_terms(query);
        (0..self.docs.len())
            .filter(|&i| terms.iter().any(|t| self.tf[i].contains_key(t)))
            .count()
    }

    pub fn search(&self, query: &str, opts: &SearchOptions) -> Vec<Hit> {
        let terms = query_terms(query);
        if terms.is_empty() {
            return Vec::new();
        }
        let mut scored: Vec<(usize, f32)> = (0..self.docs.len())
            .map(|i| (i, self.score(i, &terms)))
            .filter(|(_, s)| *s > 0.0)
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then_with(|| a.0.cmp(&b.0)));

        let mut per_kind: HashMap<SourceKind, usize> = HashMap::new();
        let mut hits = Vec::new();
        for (i, score) in scored {
            if hits.len() >= opts.limit {
                break;
            }
            let doc = &self.docs[i];
            let cap = if doc.kind == SourceKind::Tool {
                opts.tools
            } else {
                opts.per_kind
            };
            let n = per_kind.entry(doc.kind).or_default();
            if *n >= cap {
                continue;
            }
            *n += 1;
            hits.push(make_hit(doc, score, &terms));
        }
        hits
    }
}

fn make_hit(doc: &Doc, score: f32, terms: &[String]) -> Hit {
    let (offset, snippet) = best_line(&doc.text, terms);
    let cite = match doc.line {
        Some(line) if doc.verbatim => format!("{}:{}", doc.path, line + offset),
        Some(line) => format!("{}:{}", doc.path, line),
        None => doc.path.clone(),
    };
    Hit {
        kind: doc.kind,
        cite,
        title: doc.title.clone(),
        snippet,
        score: (score * 100.0).round() / 100.0,
        text: doc.text.clone(),
    }
}

/// The line with the most distinct query terms, and its offset in `text`.
/// Body lines beat headings on ties, since the title already shows.
fn best_line(text: &str, terms: &[String]) -> (usize, String) {
    let mut best: Option<((usize, bool), usize, &str)> = None;
    for (i, line) in text.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed == "---" || trimmed.starts_with("```") {
            continue;
        }
        let toks: HashSet<String> = tokenize(trimmed).into_iter().collect();
        let n = terms.iter().filter(|t| toks.contains(*t)).count();
        let key = (n, !trimmed.starts_with('#'));
        if best.is_none_or(|(bk, _, _)| key > bk) {
            best = Some((key, i, trimmed));
        }
    }
    match best {
        Some((_, i, line)) => (i, clip(line, SNIPPET_CHARS)),
        None => (0, String::new()),
    }
}

pub fn clip(s: &str, max: usize) -> String {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    if s.chars().count() <= max {
        return s;
    }
    let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
    out.push_str("...");
    out
}

/// Unique query terms, in order.
pub fn query_terms(query: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    tokenize(query)
        .into_iter()
        .filter(|t| seen.insert(t.clone()))
        .collect()
}

/// Lowercase word tokens. camelCase and PascalCase identifiers also yield
/// their parts, snake_case splits naturally, plurals are folded.
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in text.split(|c: char| !c.is_alphanumeric()) {
        if raw.is_empty() {
            continue;
        }
        let parts = camel_parts(raw);
        if parts.len() > 1 {
            for p in &parts {
                push_token(&mut out, p);
            }
        }
        push_token(&mut out, raw);
    }
    out
}

fn push_token(out: &mut Vec<String>, raw: &str) {
    let lower = raw.to_lowercase();
    if lower.chars().count() < 2 || is_stopword(&lower) {
        return;
    }
    out.push(stem(lower));
}

fn camel_parts(word: &str) -> Vec<&str> {
    let mut parts = Vec::new();
    let mut start = 0;
    let chars: Vec<(usize, char)> = word.char_indices().collect();
    for w in 1..chars.len() {
        let (i, c) = chars[w];
        let prev = chars[w - 1].1;
        let next_lower = chars.get(w + 1).is_some_and(|(_, n)| n.is_lowercase());
        let boundary = (c.is_uppercase() && prev.is_lowercase())
            || (c.is_uppercase() && prev.is_uppercase() && next_lower);
        if boundary {
            parts.push(&word[start..i]);
            start = i;
        }
    }
    parts.push(&word[start..]);
    parts
}

fn stem(word: String) -> String {
    let n = word.len();
    if n > 4 && word.ends_with("ies") {
        return format!("{}y", &word[..n - 3]);
    }
    if n > 3
        && word.ends_with('s')
        && !word.ends_with("ss")
        && !word.ends_with("us")
        && !word.ends_with("is")
    {
        return word[..n - 1].to_string();
    }
    word
}

fn is_stopword(w: &str) -> bool {
    matches!(
        w,
        "the"
            | "and"
            | "for"
            | "with"
            | "this"
            | "that"
            | "from"
            | "into"
            | "are"
            | "was"
            | "were"
            | "is"
            | "be"
            | "to"
            | "of"
            | "in"
            | "on"
            | "at"
            | "by"
            | "an"
            | "or"
            | "it"
            | "its"
            | "as"
            | "do"
            | "does"
            | "did"
            | "how"
            | "what"
            | "where"
            | "when"
            | "which"
            | "who"
            | "why"
            | "can"
            | "could"
            | "should"
            | "would"
            | "will"
            | "my"
            | "me"
            | "we"
            | "our"
            | "you"
            | "your"
            | "there"
            | "here"
            | "about"
            | "use"
            | "used"
            | "using"
            | "work"
            | "works"
    )
}

/// Hits that fit a token budget, in rank order.
#[derive(Debug, Clone, Serialize)]
pub struct Packed {
    pub hits: Vec<Hit>,
    pub tokens: usize,
    pub budget: usize,
    pub dropped: usize,
}

/// Keep hits in rank order while they fit; skip ones that do not and keep
/// trying smaller ones. The top hit is always kept so a tiny budget still
/// answers something.
pub fn pack(hits: Vec<Hit>, budget: usize, cost: impl Fn(&Hit) -> usize) -> Packed {
    let mut out = Vec::new();
    let mut used = 0usize;
    let mut dropped = 0usize;
    for hit in hits {
        let c = cost(&hit);
        if out.is_empty() || used + c <= budget {
            used += c;
            out.push(hit);
        } else {
            dropped += 1;
        }
    }
    Packed {
        hits: out,
        tokens: used,
        budget,
        dropped,
    }
}

/// Default cost: the compact one-hit rendering.
pub fn compact_cost(hit: &Hit) -> usize {
    ContextBudget::estimate_tokens(&hit.cite)
        + ContextBudget::estimate_tokens(&hit.title)
        + ContextBudget::estimate_tokens(&hit.snippet)
        + 2
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(kind: SourceKind, path: &str, title: &str, text: &str) -> Doc {
        Doc::new(kind, path, title).with_text(text)
    }

    fn top(corpus: &Corpus, q: &str) -> Vec<String> {
        corpus
            .search(q, &SearchOptions::default())
            .into_iter()
            .map(|h| h.cite)
            .collect()
    }

    #[test]
    fn tokenizer_splits_identifiers_and_folds_plurals() {
        let t = tokenize("RetrievalPipeline token_budget HTTPServer entries the");
        assert!(t.contains(&"retrieval".to_string()));
        assert!(t.contains(&"pipeline".to_string()));
        assert!(t.contains(&"retrievalpipeline".to_string()));
        assert!(t.contains(&"token".to_string()));
        assert!(t.contains(&"budget".to_string()));
        assert!(t.contains(&"http".to_string()));
        assert!(t.contains(&"server".to_string()));
        assert!(t.contains(&"entry".to_string()));
        assert!(!t.contains(&"the".to_string()));
        assert_eq!(tokenize("class"), ["class"]);
    }

    #[test]
    fn title_match_outranks_body_mention() {
        let c = Corpus::new(vec![
            doc(
                SourceKind::Doc,
                "a.md",
                "Overview",
                "We mention the budget once among many other words here.",
            ),
            doc(
                SourceKind::Doc,
                "b.md",
                "Token budget",
                "How allocation is split across layers and other words here.",
            ),
        ]);
        assert_eq!(top(&c, "budget")[0], "b.md");
    }

    #[test]
    fn rare_terms_outweigh_common_ones() {
        let mut docs: Vec<Doc> = (0..8)
            .map(|i| {
                doc(
                    SourceKind::Module,
                    &format!("m{i}.rs"),
                    "",
                    "store store data",
                )
            })
            .collect();
        docs.push(doc(SourceKind::Module, "rare.rs", "", "freshness check"));
        docs.push(doc(
            SourceKind::Module,
            "common.rs",
            "",
            "store store store",
        ));
        let c = Corpus::new(docs);
        assert_eq!(top(&c, "store freshness")[0], "rare.rs");
    }

    #[test]
    fn covering_more_terms_beats_repeating_one() {
        let c = Corpus::new(vec![
            doc(
                SourceKind::Doc,
                "one.md",
                "",
                "index index index index index",
            ),
            doc(SourceKind::Doc, "both.md", "", "incremental index rebuild"),
        ]);
        assert_eq!(top(&c, "incremental index")[0], "both.md");
    }

    #[test]
    fn prior_prefers_curated_memory_and_caps_tools() {
        let mut docs = vec![
            doc(
                SourceKind::Personal,
                "~/.claude/trove/entries/pg.md",
                "Use pgx",
                "postgres driver choice",
            ),
            doc(
                SourceKind::Tool,
                "/usr/bin/postgres",
                "postgres",
                "postgres driver choice",
            ),
        ];
        for i in 0..5 {
            docs.push(doc(
                SourceKind::Tool,
                &format!("/usr/bin/pg{i}"),
                &format!("pg{i}"),
                "postgres",
            ));
        }
        let c = Corpus::new(docs);
        let hits = c.search("postgres driver", &SearchOptions::default());
        assert_eq!(hits[0].kind, SourceKind::Personal);
        assert_eq!(
            hits.iter().filter(|h| h.kind == SourceKind::Tool).count(),
            3
        );
    }

    #[test]
    fn weight_scales_score() {
        let c = Corpus::new(vec![
            doc(SourceKind::Symbol, "test.rs", "parse_test", "parse").weighted(0.5),
            doc(SourceKind::Symbol, "lib.rs", "parse_impl", "parse"),
        ]);
        assert_eq!(top(&c, "parse")[0], "lib.rs");
    }

    #[test]
    fn verbatim_docs_cite_the_matching_line() {
        let c = Corpus::new(vec![Doc::new(SourceKind::Doc, "README.md", "Setup")
            .with_text("## Setup\n\nSetup needs Rust first.\nThen run cargo build.\n")
            .at_line(10, true)]);
        let hits = c.search("cargo build", &SearchOptions::default());
        assert_eq!(hits[0].cite, "README.md:13");
        assert_eq!(hits[0].snippet, "Then run cargo build.");
        let hits = c.search("setup", &SearchOptions::default());
        assert_eq!(
            hits[0].cite, "README.md:12",
            "body line preferred over heading"
        );

        let c = Corpus::new(vec![Doc::new(SourceKind::Symbol, "src/a.rs", "parse")
            .with_text("function parse fn parse(x: &str)")
            .at_line(42, false)]);
        assert_eq!(top(&c, "parse")[0], "src/a.rs:42");
    }

    #[test]
    fn no_match_and_empty_query() {
        let c = Corpus::new(vec![doc(SourceKind::Doc, "a.md", "A", "alpha")]);
        assert!(c.search("zeta", &SearchOptions::default()).is_empty());
        assert!(c.search("the of", &SearchOptions::default()).is_empty());
        assert_eq!(c.match_count("alpha zeta"), 1);
    }

    #[test]
    fn pack_respects_budget_but_keeps_top_hit() {
        let hit = |t: &str| Hit {
            kind: SourceKind::Doc,
            cite: t.into(),
            title: String::new(),
            snippet: String::new(),
            score: 1.0,
            text: String::new(),
        };
        let hits = vec![hit("big"), hit("a"), hit("huge"), hit("b")];
        let cost = |h: &Hit| match h.cite.as_str() {
            "big" => 50,
            "huge" => 500,
            _ => 10,
        };
        let p = pack(hits.clone(), 75, cost);
        let cites: Vec<&str> = p.hits.iter().map(|h| h.cite.as_str()).collect();
        assert_eq!(cites, ["big", "a", "b"]);
        assert_eq!(p.tokens, 70);
        assert_eq!(p.dropped, 1);

        let p = pack(hits, 1, cost);
        assert_eq!(p.hits.len(), 1);
        assert_eq!(p.hits[0].cite, "big");
    }
}
