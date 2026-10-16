//! `trove ask`: grounded answers with citations.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;
use trove_retrieve::ask::{
    self, compact_cost, Corpus, Doc, Hit, Packed, SearchOptions, SourceKind,
};
use trove_retrieve::budget::ContextBudget;
use trove_retrieve::sources;
use trove_tools::{Catalog, DiscoveryEnv, RefreshOptions};

use crate::llm::{self, Provider};
use crate::paths::{display_path, home_dir};

pub const DEFAULT_BUDGET: usize = 1500;
/// Characters of source text sent to a provider per hit, before budgeting.
const EXCERPT_CHARS: usize = 1600;

/// Everything `ask` searches, loaded once and reused by the REPL.
pub struct Knowledge {
    pub corpus: Corpus,
    pub notes: Vec<String>,
}

impl Knowledge {
    pub fn load(repo: &Path, with_tools: bool) -> Self {
        let home = home_dir();
        let shown = |p: &Path| display_path(p, repo, &home);
        let mut docs: Vec<Doc> = Vec::new();
        let mut notes = Vec::new();

        let personal = home.join(".claude/trove");
        let project = repo.join(".claude/trove");
        docs.extend(sources::trove_entries(
            &personal,
            SourceKind::Personal,
            &shown,
        ));
        if project != personal {
            docs.extend(sources::trove_entries(
                &project,
                SourceKind::Project,
                &shown,
            ));
        }
        docs.extend(sources::repo_docs(repo));

        let engine = sources::engine_index(repo);
        if engine.is_empty() {
            notes.push(
                "no engine index for this repo; run `trove index` for symbol citations".into(),
            );
        }
        docs.extend(engine);

        if with_tools {
            match Catalog::open(&DiscoveryEnv::from_process(), &RefreshOptions::default()) {
                Ok((cat, _)) => docs.extend(cat.tools().into_iter().map(|t| {
                    let summary = t.summary.clone().unwrap_or_default();
                    Doc::new(SourceKind::Tool, t.path.clone(), t.name.clone())
                        .with_text(format!("{} {summary}", t.name))
                })),
                Err(e) => notes.push(format!("tool catalog unavailable: {e}")),
            }
        }

        Self {
            corpus: Corpus::new(docs),
            notes,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AskOptions {
    pub budget: usize,
    pub limit: usize,
    pub provider: Provider,
    pub model: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AskResult {
    pub query: String,
    pub mode: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub answer_error: Option<String>,
    pub sources: Vec<Hit>,
    pub tokens: usize,
    pub budget: usize,
    pub matches: usize,
    pub searched: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

pub fn ask(k: &Knowledge, question: &str, opts: &AskOptions) -> AskResult {
    let search = SearchOptions {
        limit: opts.limit,
        ..SearchOptions::default()
    };
    let hits = k.corpus.search(question, &search);
    let use_llm = opts.provider != Provider::None && !hits.is_empty();
    let packed: Packed = if use_llm {
        ask::pack(hits, opts.budget, |h| {
            ContextBudget::estimate_tokens(&excerpt(h))
        })
    } else {
        ask::pack(hits, opts.budget, compact_cost)
    };

    let (answer, answer_error) = if use_llm {
        let prompt = build_prompt(question, &packed.hits);
        match llm::complete(
            opts.provider,
            opts.model.as_deref(),
            llm::SYSTEM_PROMPT,
            &prompt,
        ) {
            Ok(a) => (Some(a), None),
            Err(e) => (None, Some(e.to_string())),
        }
    } else {
        (None, None)
    };

    AskResult {
        query: question.to_string(),
        mode: if answer.is_some() { "llm" } else { "retrieval" },
        provider: (opts.provider != Provider::None).then(|| opts.provider.label()),
        answer,
        answer_error,
        matches: k.corpus.match_count(question),
        searched: k.corpus.len(),
        tokens: packed.tokens,
        budget: packed.budget,
        sources: packed.hits,
        notes: k.notes.clone(),
    }
}

fn excerpt(hit: &Hit) -> String {
    ask::clip(&hit.text, EXCERPT_CHARS)
}

fn build_prompt(question: &str, hits: &[Hit]) -> String {
    let mut out = format!("Question: {question}\n\nSources:\n");
    for (i, h) in hits.iter().enumerate() {
        out.push_str(&format!(
            "\n[{}] {} {} ({})\n{}\n",
            i + 1,
            h.kind.label(),
            h.cite,
            h.title,
            excerpt(h)
        ));
    }
    out
}

pub fn print(result: &AskResult, json: bool) -> Result<()> {
    if json {
        println!("{}", serde_json::to_string_pretty(result)?);
        return Ok(());
    }
    if let Some(answer) = &result.answer {
        println!("{answer}\n");
    }
    if result.sources.is_empty() {
        println!(
            "no matches for \"{}\" in {} sources",
            result.query, result.searched
        );
    }
    for (i, h) in result.sources.iter().enumerate() {
        let title = if h.title.is_empty() || h.cite.ends_with(&h.title) {
            String::new()
        } else {
            format!("  {}", h.title)
        };
        println!("{:>2} {:<9} {}{}", i + 1, h.kind.label(), h.cite, title);
        if !h.snippet.is_empty() && h.snippet != h.title {
            println!("   {}", h.snippet);
        }
    }
    let mode = match (&result.answer, &result.answer_error) {
        (Some(_), _) => format!("answer by {}", result.provider.unwrap_or("llm")),
        (None, Some(e)) => format!("retrieval only ({e})"),
        (None, None) if result.provider.is_some() => "no sources to answer from".into(),
        (None, None) => "offline retrieval; add --provider for a written answer".into(),
    };
    println!(
        "-- {} of {} matches | ~{}/{} tokens | {}",
        result.sources.len(),
        result.matches,
        result.tokens,
        result.budget,
        mode
    );
    for n in &result.notes {
        println!("-- {n}");
    }
    Ok(())
}
