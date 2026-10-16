use std::path::PathBuf;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use trove_ship::redact::agent::{AgentConfig, AgentKind};
use trove_ship::redact::detectors::builtin_names;
use trove_ship::redact::LocalIdentity;
use trove_ship::{ship, FailClosed, Format, Reindex, ShipOptions};

/// Turn a project and its trove into developer handoff docs, with secrets and
/// private information stripped before anything is written.
#[derive(Parser)]
#[command(name = "trove-ship", version)]
struct Cli {
    /// Repository to ship
    #[arg(long, default_value = ".")]
    repo: PathBuf,

    /// Output directory (default: <repo>/.trove/ship)
    #[arg(long)]
    out: Option<PathBuf>,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Folder)]
    format: Format,

    /// When to (re)build the engine index in .trove/
    #[arg(long, value_enum, default_value_t = Reindex::Auto)]
    reindex: Reindex,

    /// Run a coding-agent review after the deterministic pass
    #[arg(long, value_enum)]
    agent: Option<AgentKind>,

    /// Shell command for the agent review; gets the prompt on stdin, prints a JSON array
    #[arg(long)]
    agent_cmd: Option<String>,

    /// Seconds to wait for each agent call
    #[arg(long, default_value_t = 300)]
    agent_timeout: u64,

    /// Redact and print the report without writing any files
    #[arg(long)]
    dry_run: bool,

    /// Write into a non-empty directory that trove-ship did not create
    #[arg(long)]
    force: bool,

    /// Do not treat the current user's login name and home directory as private
    #[arg(long)]
    no_local_identity: bool,

    /// Print the redaction report as JSON
    #[arg(long)]
    json: bool,

    /// List the built-in detectors and exit
    #[arg(long)]
    list_detectors: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if cli.list_detectors {
        for name in builtin_names() {
            println!("{name}");
        }
        return ExitCode::SUCCESS;
    }

    let mut opts = ShipOptions::new(&cli.repo);
    if let Some(out) = cli.out {
        opts.out = out;
    }
    opts.format = cli.format;
    opts.reindex = cli.reindex;
    opts.dry_run = cli.dry_run;
    opts.force = cli.force;
    opts.identity = (!cli.no_local_identity).then(LocalIdentity::from_env);
    if cli.agent.is_some() || cli.agent_cmd.is_some() {
        opts.agent = Some(AgentConfig {
            kind: cli.agent,
            command: cli.agent_cmd,
            timeout: Duration::from_secs(cli.agent_timeout),
        });
    }

    match ship(&opts) {
        Ok(outcome) => {
            if cli.json {
                print!("{}", outcome.report.to_json());
            } else if cli.dry_run {
                print!("{}", outcome.report.to_markdown());
                println!(
                    "Dry run: {} files rendered, nothing written.",
                    outcome.files.len()
                );
            } else {
                let r = &outcome.report;
                println!(
                    "Shipped {} ({}) to {}",
                    r.project,
                    r.format,
                    opts.out.display()
                );
                println!(
                    "Redacted {} distinct values ({} high confidence). Report: {}",
                    r.items.len(),
                    r.high_confidence,
                    opts.out
                        .join(if r.format == "site" {
                            "redaction-report.html"
                        } else {
                            "REDACTION_REPORT.md"
                        })
                        .display()
                );
                println!("Verification {}", r.verification);
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            if let Some(fc) = err.downcast_ref::<FailClosed>() {
                eprintln!("trove-ship: {fc}");
                return ExitCode::from(2);
            }
            eprintln!("trove-ship: {err:#}");
            ExitCode::FAILURE
        }
    }
}
