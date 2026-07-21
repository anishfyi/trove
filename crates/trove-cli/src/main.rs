use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use trove_core::execution::ExecutionEvent;
use trove_core::patch::PatchRecord;
use trove_core::store::TroveStore;
use trove_index::{IndexOptions, Indexer};
use trove_retrieve::budget::ContextBudget;
use trove_retrieve::pipeline::{import_historical, RetrievalPipeline};

#[derive(Parser)]
#[command(name = "trove", about = "Repository memory engine for LLMs", version)]
struct Cli {
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Index the repository: symbols, modules, subsystems, architecture
    Index {
        #[arg(long)]
        full: bool,
    },
    /// Show index status
    Status,
    /// Progressive retrieval for a natural-language query
    Query {
        query: String,
        #[arg(long, default_value_t = 200_000)]
        budget: usize,
        #[arg(long)]
        json: bool,
    },
    /// Import L5 historical entries from ~/.claude/trove or ./.claude/trove
    ImportHistorical,
    /// Record execution memory event
    Record {
        #[command(subcommand)]
        event: RecordEvent,
    },
    /// Record a patch (before/after hash + reason)
    Patch {
        path: String,
        before: String,
        after: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Subcommand)]
enum RecordEvent {
    Open { path: String },
    Search { query: String },
    Command { cmd: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::Index { full } => {
            let indexer = Indexer::open(&cli.repo)?;
            let report = indexer.index(IndexOptions {
                incremental: !full,
            })?;
            println!(
                "Indexed {} files, {} symbols, {} modules, {} subsystems ({} stale rebuilt)",
                report.files_scanned,
                report.symbols_indexed,
                report.modules_built,
                report.subsystems_built,
                report.stale_rebuilt,
            );
        }
        Commands::Status => {
            let store = TroveStore::open(&cli.repo)?;
            match store.read_manifest()? {
                Some(m) => {
                    println!("Trove v{}", m.version);
                    println!("Repo:   {}", m.repo_root);
                    println!("Indexed: {}", m.indexed_at);
                    println!(
                        "Counts: {} symbols, {} modules, {} subsystems",
                        m.symbol_count, m.module_count, m.subsystem_count
                    );
                }
                None => println!("No index found. Run: trove index"),
            }
        }
        Commands::Query { query, budget, json } => {
            let store = TroveStore::open(&cli.repo)?;
            let pipeline = RetrievalPipeline::new(store);
            let ctx = pipeline.query(&query, ContextBudget::for_total(budget))?;

            if json {
                println!("{}", serde_json::to_string_pretty(&QueryOutput::from(&ctx))?);
            } else {
                println!("# Query: {}\n", ctx.query);
                println!(
                    "Estimated tokens: {} / {} (stopped early: {})\n",
                    ctx.estimated_tokens, ctx.budget.total_tokens, ctx.stopped_early
                );
                for item in &ctx.items {
                    println!(
                        "## {} [{}] {}\n{}\n",
                        item.level.label(),
                        format!("{:?}", item.tier).to_lowercase(),
                        item.object_id,
                        item.content
                    );
                    println!(
                        "_confidence: {} | path: {}_\n",
                        item.confidence_reason,
                        item.retrieval_path.join(" -> ")
                    );
                }
            }
        }
        Commands::ImportHistorical => {
            let store = TroveStore::open(&cli.repo)?;
            let project = cli.repo.join(".claude/trove");
            let user = dirs_home().join(".claude/trove");
            let mut total = 0usize;
            if project.join("INDEX.md").exists() {
                total += import_historical(&store, &project)?;
            }
            if user.join("INDEX.md").exists() {
                total += import_historical(&store, &user)?;
            }
            println!("Imported {total} historical entries");
        }
        Commands::Record { event } => {
            let store = TroveStore::open(&cli.repo)?;
            let mut memory = store.read_execution()?;
            let now = Utc::now();
            match event {
                RecordEvent::Open { path } => {
                    memory.record(ExecutionEvent::OpenedFile { path, at: now });
                }
                RecordEvent::Search { query } => {
                    memory.record(ExecutionEvent::Search { query, at: now });
                }
                RecordEvent::Command { cmd } => {
                    memory.record(ExecutionEvent::Command { cmd, at: now });
                }
            }
            store.write_execution(&memory)?;
            println!("Recorded execution event");
        }
        Commands::Patch {
            path,
            before,
            after,
            reason,
        } => {
            let store = TroveStore::open(&cli.repo)?;
            let id = format!("patch:{}:{}", path, Utc::now().timestamp());
            let patch = PatchRecord {
                id,
                path,
                before_hash: before,
                after_hash: after,
                reason,
                related_issue: None,
                related_pr: None,
                author: None,
                date: Utc::now(),
                future_impact: None,
            };
            store.append_patch(&patch)?;
            println!("Recorded patch {}", patch.id);
        }
    }

    Ok(())
}

fn dirs_home() -> PathBuf {
    std::env::var("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("."))
}

#[derive(serde::Serialize)]
struct QueryOutput<'a> {
    query: &'a str,
    estimated_tokens: usize,
    stopped_early: bool,
    items: &'a [trove_retrieve::pipeline::RetrievedItem],
}

impl<'a> From<&'a trove_retrieve::pipeline::AssembledContext> for QueryOutput<'a> {
    fn from(ctx: &'a trove_retrieve::pipeline::AssembledContext) -> Self {
        Self {
            query: &ctx.query,
            estimated_tokens: ctx.estimated_tokens,
            stopped_early: ctx.stopped_early,
            items: &ctx.items,
        }
    }
}
