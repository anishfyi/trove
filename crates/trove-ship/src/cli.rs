//! The one ship implementation behind both front doors: the `trove-ship`
//! binary and the `trove ship` subcommand flatten this CLI and call
//! [`run`], so their behavior, output and exit codes are identical by
//! construction.

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;

use crate::redact::agent::{AgentConfig, AgentKind};
use crate::redact::detectors::builtin_names;
use crate::redact::LocalIdentity;
use crate::{ship, FailClosed, Format, Reindex, ShipOptions};

/// Turn a project and its trove into developer handoff docs, with secrets and
/// private information stripped before anything is written.
#[derive(Debug, Parser)]
#[command(name = "trove-ship", version)]
pub struct ShipCli {
    /// Repository to ship
    #[arg(long, default_value = ".")]
    pub repo: PathBuf,

    /// Output directory (default: <repo>/.trove/ship)
    #[arg(long)]
    pub out: Option<PathBuf>,

    /// Output format
    #[arg(long, value_enum, default_value_t = Format::Folder)]
    pub format: Format,

    /// When to (re)build the engine index in .trove/
    #[arg(long, value_enum, default_value_t = Reindex::Auto)]
    pub reindex: Reindex,

    /// Run a coding-agent review after the deterministic pass
    #[arg(long, value_enum)]
    pub agent: Option<AgentKind>,

    /// Shell command for the agent review; gets the prompt on stdin, prints a JSON array
    #[arg(long)]
    pub agent_cmd: Option<String>,

    /// Seconds to wait for each agent call
    #[arg(long, default_value_t = 300)]
    pub agent_timeout: u64,

    /// Redact and print the report without writing any files
    #[arg(long)]
    pub dry_run: bool,

    /// Write into a non-empty directory that trove-ship did not create
    #[arg(long)]
    pub force: bool,

    /// Do not treat the current user's login name and home directory as private
    #[arg(long)]
    pub no_local_identity: bool,

    /// Print the redaction report as JSON
    #[arg(long)]
    pub json: bool,

    /// List the built-in detectors and exit
    #[arg(long)]
    pub list_detectors: bool,
}

/// Run one ship invocation. `bin` labels error messages ("trove-ship" or
/// "trove ship"). Exit codes: 0 success, 2 fail-closed (nothing written),
/// 1 any other error.
pub fn run(cli: &ShipCli, bin: &str) -> u8 {
    if cli.list_detectors {
        for name in builtin_names() {
            println!("{name}");
        }
        return 0;
    }

    let mut opts = ShipOptions::new(&cli.repo);
    if let Some(out) = cli.out.clone() {
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
            command: cli.agent_cmd.clone(),
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
            0
        }
        Err(err) => {
            if let Some(fc) = err.downcast_ref::<FailClosed>() {
                eprintln!("{bin}: {fc}");
                return 2;
            }
            eprintln!("{bin}: {err:#}");
            1
        }
    }
}
