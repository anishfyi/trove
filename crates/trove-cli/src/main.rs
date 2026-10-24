use trove_cli::{ask, llm, paths, remember, repl, tools};

use std::path::PathBuf;

use anyhow::Result;
use chrono::Utc;
use clap::{Parser, Subcommand};
use serde_json::json;
use trove_core::execution::ExecutionEvent;
use trove_core::patch::PatchRecord;
use trove_core::store::TroveStore;
use trove_index::{IndexOptions, Indexer};
use trove_retrieve::budget::ContextBudget;
use trove_retrieve::pipeline::{import_historical, RetrievalPipeline};

use ask::{AskOptions, Knowledge};
use llm::Provider;
use paths::{find_repo_root, home_dir};

#[derive(Subcommand)]
enum McpCommand {
    /// Run the stdio MCP server (read-only: trove_ask, trove_query, trove_status, trove_search_tools)
    Serve,
}

#[derive(Parser)]
#[command(
    name = "trove",
    about = "Memory, semantic and context layer for coding agents",
    long_about = "Memory, semantic and context layer for coding agents.\n\nRun `trove` with no arguments for an interactive prompt.",
    version
)]
struct Cli {
    /// Repository root (default: current dir; ask and the prompt search upward for .trove or .git)
    #[arg(long, global = true)]
    repo: Option<PathBuf>,

    /// Machine-readable JSON output
    #[arg(long, global = true)]
    json: bool,

    /// Token budget for assembled context
    #[arg(long, global = true, value_name = "TOKENS")]
    budget: Option<usize>,

    /// Answer step for ask: none keeps retrieval offline
    #[arg(
        long,
        global = true,
        value_enum,
        env = "TROVE_PROVIDER",
        default_value = "none"
    )]
    provider: Provider,

    /// Model for the provider (anthropic default: claude-opus-5-5)
    #[arg(long, global = true, env = "TROVE_MODEL")]
    model: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Answer a question with citations from troves, repo docs and the code index within a token budget
    Ask {
        /// The question
        #[arg(required = true, num_args = 1..)]
        question: Vec<String>,
        /// Max sources to consider before budgeting
        #[arg(long, default_value_t = 12)]
        limit: usize,
        /// Leave the tool catalog out
        #[arg(long)]
        no_tools: bool,
        /// Keep non-secret private categories (emails, hosts, paths) unredacted
        /// in provider prompts; secrets are always redacted
        #[arg(long)]
        no_redact_nonsecret: bool,
    },
    /// Serve trove memory read-only over the Model Context Protocol (stdio)
    Mcp {
        #[command(subcommand)]
        cmd: McpCommand,
    },
    /// Turn a project and its trove into developer handoff docs, with secrets and
    /// private information stripped before anything is written
    Ship {
        #[command(flatten)]
        ship: trove_ship::cli::ShipCli,
    },
    /// Capture a durable learning into the trove (one entry file plus an index line)
    Remember {
        /// Entry title
        #[arg(required = true, num_args = 1..)]
        title: Vec<String>,
        /// Entry type: convention, decision, gotcha, preference, reference, project, snippet, context
        #[arg(long = "type", value_name = "TYPE")]
        entry_type: String,
        /// Entry body (read from stdin when piped and not given)
        #[arg(long, value_name = "TEXT")]
        body: Option<String>,
        /// Comma-separated tags
        #[arg(long, value_name = "A,B")]
        tags: Option<String>,
        /// Target section override: conventions | memory | context
        #[arg(long, value_name = "SECTION")]
        section: Option<String>,
        /// Replace an existing entry with the same slug
        #[arg(long)]
        update: bool,
    },
    /// Installed CLIs: `tools`, `tools search <terms>`, `tools <name>`, `tools refresh`
    Tools {
        /// list | search <terms> | refresh | describe <name> | <name>
        args: Vec<String>,
        /// Run each tool's --help (guarded, 1.5s timeout) when it has no man summary
        #[arg(long)]
        probe: bool,
        /// Rescan every directory, ignoring the cache
        #[arg(long)]
        full: bool,
        /// Open the man page for `tools <name>`
        #[arg(long)]
        man: bool,
        /// Max rows (0 for all; list defaults to all)
        #[arg(long)]
        limit: Option<usize>,
    },
    /// Index the repository: symbols, modules, subsystems, architecture
    Index {
        #[arg(long)]
        full: bool,
    },
    /// Show index status
    Status,
    /// Assemble progressive context for a query within a token budget
    Query { query: String },
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
    let repo = cli.repo.clone().unwrap_or_else(|| PathBuf::from("."));
    let ask_repo = || match &cli.repo {
        Some(r) => r.canonicalize().unwrap_or_else(|_| r.clone()),
        None => find_repo_root(&PathBuf::from(".")),
    };
    let ask_opts = |limit: usize, no_redact_nonsecret: bool| AskOptions {
        budget: cli.budget.unwrap_or(ask::DEFAULT_BUDGET),
        limit,
        provider: cli.provider,
        model: cli.model.clone(),
        no_redact_nonsecret,
    };

    let Some(command) = cli.command else {
        return repl::Repl {
            repo: ask_repo(),
            json: cli.json,
            opts: ask_opts(12, false),
        }
        .run();
    };

    match command {
        Commands::Ask {
            question,
            limit,
            no_tools,
            no_redact_nonsecret,
        } => {
            let knowledge = Knowledge::load(&ask_repo(), !no_tools);
            let result = ask::ask(
                &knowledge,
                &question.join(" "),
                &ask_opts(limit, no_redact_nonsecret),
            );
            ask::print(&result, cli.json)?;
        }
        Commands::Remember {
            title,
            entry_type,
            body,
            tags,
            section,
            update,
        } => {
            use std::io::IsTerminal;
            let repo = ask_repo();
            let home = home_dir();
            let body = match body {
                Some(b) => b,
                None if !std::io::stdin().is_terminal() => {
                    let mut buf = String::new();
                    std::io::Read::read_to_string(&mut std::io::stdin(), &mut buf)?;
                    buf
                }
                None => anyhow::bail!("provide --body or pipe the entry body on stdin"),
            };
            let tags = tags
                .map(|t| {
                    t.split(',')
                        .map(str::trim)
                        .filter(|t| !t.is_empty())
                        .map(str::to_string)
                        .collect()
                })
                .unwrap_or_default();
            let opts = remember::RememberOptions {
                title: title.join(" "),
                body,
                entry_type,
                tags,
                section,
                update,
            };
            let r = remember::remember(&repo, &home, &opts)?;
            if cli.json {
                println!(
                    "{}",
                    json!({
                        "slug": r.slug,
                        "path": r.path,
                        "section": r.section,
                        "updated": r.updated,
                    })
                );
                return Ok(());
            }
            let where_ = if r.section.is_empty() {
                "user trove".to_string()
            } else {
                r.section.clone()
            };
            println!(
                "Remembered '{}' -> {} ({})",
                opts.title,
                crate::paths::display_path(&r.path, &repo, &home),
                where_
            );
        }
        Commands::Tools {
            args,
            probe,
            full,
            man,
            limit,
        } => {
            let is_list = args.is_empty() || args[0] == "list";
            tools::run(tools::ToolsArgs {
                limit: limit.unwrap_or(if is_list { 0 } else { 20 }),
                args,
                probe,
                full,
                man,
                json: cli.json,
            })?;
        }
        Commands::Index { full } => {
            let indexer = Indexer::open(&repo)?;
            let report = indexer.index(IndexOptions { incremental: !full })?;
            if cli.json {
                println!(
                    "{}",
                    json!({
                        "files_scanned": report.files_scanned,
                        "symbols": report.symbols_indexed,
                        "modules": report.modules_built,
                        "subsystems": report.subsystems_built,
                        "stale_rebuilt": report.stale_rebuilt,
                    })
                );
                return Ok(());
            }
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
            let store = TroveStore::open(&repo)?;
            let manifest = store.read_manifest()?;
            if cli.json {
                println!("{}", serde_json::to_string_pretty(&manifest)?);
                return Ok(());
            }
            match manifest {
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
        Commands::Mcp { cmd } => match cmd {
            McpCommand::Serve => {
                trove_cli::mcp::serve(&repo, &home_dir()).map_err(anyhow::Error::new)?;
            }
        },
        Commands::Ship { mut ship } => {
            // Global --repo feeds the ship target; global --json and the
            // flattened --json are the same switch. One shared implementation
            // behind both front doors.
            ship.repo = repo.clone();
            ship.json = ship.json || cli.json;
            let code = trove_ship::cli::run(&ship, "trove ship");
            if code != 0 {
                std::process::exit(i32::from(code));
            }
        }
        Commands::Query { query } => {
            let store = TroveStore::open(&repo)?;
            let pipeline = RetrievalPipeline::new(store);
            let budget = cli.budget.unwrap_or(200_000);
            let ctx = pipeline.query(&query, ContextBudget::for_total(budget))?;

            if cli.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&QueryOutput::from(&ctx))?
                );
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
            let store = TroveStore::open(&repo)?;
            let project = repo.join(".claude/trove");
            let user = home_dir().join(".claude/trove");
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
            let store = TroveStore::open(&repo)?;
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
            let store = TroveStore::open(&repo)?;
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
