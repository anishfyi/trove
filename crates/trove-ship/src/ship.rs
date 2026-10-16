//! The ship pipeline: index, collect, compose, redact, optionally review with a
//! coding agent, render, verify, and only then write.

use std::fmt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use trove_core::store::TROVE_DIR;
use trove_index::{IndexOptions, Indexer};

use crate::collect::{collect, CollectOptions};
use crate::docs;
use crate::output::{self, Files, Format};
use crate::redact::agent::{self, AgentConfig};
use crate::redact::{
    self, Category, Confidence, LocalIdentity, RedactConfig, Redactor, Survivor, TroveIgnore,
};
use crate::report::Report;

/// Label that replaces the absolute repository path everywhere.
pub const REPO_ROOT_LABEL: &str = "$REPO_ROOT";

#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Reindex {
    /// Index only when no index exists yet
    Auto,
    /// Refresh the index before shipping
    Always,
    /// Use whatever index exists, never write to .trove/
    Never,
}

#[derive(Debug, Clone)]
pub struct ShipOptions {
    pub repo: PathBuf,
    pub out: PathBuf,
    pub format: Format,
    pub reindex: Reindex,
    pub agent: Option<AgentConfig>,
    /// The local user whose name and home directory must not leak. `None`
    /// disables the identity detectors (useful for reproducible tests).
    pub identity: Option<LocalIdentity>,
    pub dry_run: bool,
    pub force: bool,
}

impl ShipOptions {
    pub fn new(repo: impl Into<PathBuf>) -> Self {
        let repo = repo.into();
        Self {
            out: repo.join(TROVE_DIR).join("ship"),
            repo,
            format: Format::Folder,
            reindex: Reindex::Auto,
            agent: None,
            identity: Some(LocalIdentity::from_env()),
            dry_run: false,
            force: false,
        }
    }
}

#[derive(Debug)]
pub struct ShipOutcome {
    pub report: Report,
    pub files: Files,
    pub written: Vec<PathBuf>,
}

/// Returned when a high-confidence secret is still present in the output. No
/// file has been written when this happens.
#[derive(Debug)]
pub struct FailClosed {
    pub survivors: Vec<Survivor>,
}

impl fmt::Display for FailClosed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(
            f,
            "refusing to write: {} high-confidence secret(s) survived redaction",
            self.survivors.len()
        )?;
        for s in &self.survivors {
            writeln!(f, "  {}:{} ({})", s.file, s.line, s.detector)?;
        }
        write!(
            f,
            "Fix the source (or exclude the file in .troveignore) and run again. Nothing was written."
        )
    }
}

impl std::error::Error for FailClosed {}

pub fn ship(opts: &ShipOptions) -> Result<ShipOutcome> {
    let repo = opts
        .repo
        .canonicalize()
        .with_context(|| format!("open {}", opts.repo.display()))?;
    let ignore = TroveIgnore::load(&repo)?;

    ensure_index(&repo, opts.reindex)?;

    let out_abs = absolute(&opts.out);
    let facts = collect(
        &repo,
        &ignore,
        &CollectOptions {
            exclude_dirs: vec![out_abs],
        },
    )?;
    let project = facts.name.clone();
    let files_excluded = facts.files_excluded;
    let mut documents = docs::build(&facts);

    let mut redactor = Redactor::new(RedactConfig {
        ignore,
        identity: opts.identity.clone(),
        path_labels: vec![(
            repo.to_string_lossy().to_string(),
            REPO_ROOT_LABEL.to_string(),
        )],
    });
    let mut texts: Vec<(String, String)> = documents
        .iter()
        .map(|d| (d.file.to_string(), d.body.clone()))
        .collect();
    redactor.redact_all(&mut texts);

    let (agent_label, agent_findings) = match &opts.agent {
        Some(cfg) => (cfg.label(), agent_review(cfg, &mut redactor, &mut texts)?),
        None => ("not run".to_string(), 0),
    };

    for (doc, (_, text)) in documents.iter_mut().zip(texts) {
        doc.body = text;
    }

    let project_label = redactor.sanitize_label(&project);
    let mut report = Report::new(
        &redactor,
        &project,
        opts.format.label(),
        agent_label,
        agent_findings,
        files_excluded,
    );
    let probe = output::render(opts.format, &project_label, &documents, &report);
    report.verification = format!(
        "passed: no high-confidence secret in {} output files",
        probe.len()
    );
    let files = output::render(opts.format, &project_label, &documents, &report);

    redact::verify(&files).map_err(|survivors| FailClosed { survivors })?;

    let written = if opts.dry_run {
        Vec::new()
    } else {
        output::write(&opts.out, &files, opts.force)?
    };
    Ok(ShipOutcome {
        report,
        files,
        written,
    })
}

fn ensure_index(repo: &Path, mode: Reindex) -> Result<()> {
    let has_index = repo.join(TROVE_DIR).join("manifest.json").exists();
    let run = match mode {
        Reindex::Always => true,
        Reindex::Auto => !has_index,
        Reindex::Never => false,
    };
    if run {
        Indexer::open(repo)?
            .index(IndexOptions { incremental: true })
            .context("index the repository")?;
    }
    Ok(())
}

/// Hand each redacted document to the agent, register what it finds, and
/// re-apply redaction. Any agent failure aborts the ship: an explicitly
/// requested review that did not run must not look like a clean one.
fn agent_review(
    cfg: &AgentConfig,
    redactor: &mut Redactor,
    texts: &mut [(String, String)],
) -> Result<usize> {
    let detector = format!("agent:{}", cfg.label());
    let mut added = 0;
    for (name, text) in texts.iter() {
        let findings = agent::review(cfg, name, text)
            .with_context(|| format!("agent review of {name} failed; nothing was written"))?;
        for f in findings {
            let reason = if f.reason.trim().is_empty() {
                "flagged by agent review".to_string()
            } else {
                f.reason.chars().take(120).collect()
            };
            let category = Category::parse(&f.category);
            if redactor.add_value(&f.text, category, Confidence::Medium, &detector, &reason) {
                added += 1;
            }
        }
    }
    if added > 0 {
        for (name, text) in texts.iter_mut() {
            *text = redactor.apply(name, text);
        }
    }
    Ok(added)
}

fn absolute(p: &Path) -> PathBuf {
    if p.is_absolute() {
        p.to_path_buf()
    } else {
        std::env::current_dir()
            .map(|c| c.join(p))
            .unwrap_or_else(|_| p.to_path_buf())
    }
}
