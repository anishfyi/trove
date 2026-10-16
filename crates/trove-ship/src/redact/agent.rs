//! Optional coding-agent review. After the deterministic pass, each document is
//! handed to a local coding agent CLI (Claude Code, Codex or pi) that lists any
//! private information the detectors missed. The agent never edits text: it
//! returns exact substrings, which go through the same redaction machinery.
//!
//! Only already-redacted text is sent to the agent.

use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::{anyhow, bail, Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, clap::ValueEnum)]
#[serde(rename_all = "lowercase")]
pub enum AgentKind {
    Claude,
    Codex,
    Pi,
}

impl AgentKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Claude => "claude",
            Self::Codex => "codex",
            Self::Pi => "pi",
        }
    }

    /// Program and arguments for a non-interactive run that reads the prompt
    /// from stdin and prints the answer.
    fn command(self) -> (&'static str, &'static [&'static str]) {
        match self {
            Self::Claude => ("claude", &["-p", "--output-format", "text"]),
            Self::Codex => ("codex", &["exec", "-"]),
            Self::Pi => ("pi", &["-p"]),
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub kind: Option<AgentKind>,
    /// Shell command overriding the default invocation. Receives the prompt on
    /// stdin and must print a JSON array.
    pub command: Option<String>,
    pub timeout: Duration,
}

impl AgentConfig {
    pub fn label(&self) -> String {
        match (&self.kind, &self.command) {
            (Some(k), None) => k.label().to_string(),
            (Some(k), Some(_)) => format!("{} (custom command)", k.label()),
            (None, Some(_)) => "custom command".to_string(),
            (None, None) => "none".to_string(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct AgentFinding {
    pub text: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub reason: String,
}

const PROMPT: &str = "You are reviewing developer handoff documentation before it is shared \
outside the team. A deterministic pass already replaced obvious secrets and PII with \
placeholders like [REDACTED:email-1]. Find anything private that is still present: \
credentials, tokens, personal names of non-public people, customer or client names, \
internal project codenames, internal hostnames or URLs, IP addresses, phone numbers, emails, \
physical addresses, account numbers. Do not flag public open-source project names, \
programming terms, or placeholders.\n\n\
Answer with ONLY a JSON array. Each item: {\"text\": \"<exact substring from the document>\", \
\"category\": \"secret|email|phone|host|ip|path|user|customer|other\", \"reason\": \"<short reason>\"}. \
Answer [] if nothing is left.\n\nDocument `";

pub fn build_prompt(name: &str, text: &str) -> String {
    format!("{PROMPT}{name}`:\n\n<document>\n{text}\n</document>\n")
}

/// Ask the agent about one document. Only findings that occur verbatim in the
/// document (ignoring case) are returned.
pub fn review(config: &AgentConfig, name: &str, text: &str) -> Result<Vec<AgentFinding>> {
    let prompt = build_prompt(name, text);
    let output = run(config, &prompt)?;
    let findings = parse_findings(&output)?;
    let lower = text.to_lowercase();
    Ok(findings
        .into_iter()
        .filter(|f| {
            let t = f.text.trim();
            t.chars().count() >= 3 && !t.contains("[REDACTED:") && lower.contains(&t.to_lowercase())
        })
        .collect())
}

fn run(config: &AgentConfig, prompt: &str) -> Result<String> {
    let mut cmd = match (&config.command, config.kind) {
        (Some(shell), _) => shell_command(shell),
        (None, Some(kind)) => {
            let (program, args) = kind.command();
            let mut c = Command::new(program);
            c.args(args);
            c
        }
        (None, None) => bail!("no agent configured"),
    };
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .with_context(|| format!("start agent `{}`", config.label()))?;

    let mut stdin = child.stdin.take().ok_or_else(|| anyhow!("agent stdin"))?;
    let prompt = prompt.to_string();
    let writer = std::thread::spawn(move || {
        let _ = stdin.write_all(prompt.as_bytes());
    });
    let mut stdout = child.stdout.take().ok_or_else(|| anyhow!("agent stdout"))?;
    let reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stdout.read_to_string(&mut buf);
        buf
    });
    let mut stderr = child.stderr.take().ok_or_else(|| anyhow!("agent stderr"))?;
    let err_reader = std::thread::spawn(move || {
        let mut buf = String::new();
        let _ = stderr.read_to_string(&mut buf);
        buf
    });

    let started = Instant::now();
    let status = loop {
        if let Some(status) = child.try_wait()? {
            break status;
        }
        if started.elapsed() > config.timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!(
                "agent `{}` timed out after {}s",
                config.label(),
                config.timeout.as_secs()
            );
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let _ = writer.join();
    let out = reader.join().unwrap_or_default();
    let err = err_reader.join().unwrap_or_default();
    if !status.success() {
        let tail: String = err.lines().rev().take(5).collect::<Vec<_>>().join(" | ");
        bail!("agent `{}` exited with {status}: {tail}", config.label());
    }
    Ok(out)
}

#[cfg(unix)]
fn shell_command(shell: &str) -> Command {
    let mut c = Command::new("sh");
    c.arg("-c").arg(shell);
    c
}

#[cfg(not(unix))]
fn shell_command(shell: &str) -> Command {
    let mut c = Command::new("cmd");
    c.arg("/C").arg(shell);
    c
}

/// Pull the JSON array out of an agent reply, tolerating prose or code fences
/// around it.
pub fn parse_findings(output: &str) -> Result<Vec<AgentFinding>> {
    let start = output.find('[');
    let end = output.rfind(']');
    let (Some(start), Some(end)) = (start, end) else {
        bail!("agent reply had no JSON array");
    };
    if end < start {
        bail!("agent reply had no JSON array");
    }
    serde_json::from_str(&output[start..=end])
        .context("agent reply was not a JSON array of findings")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_fenced_reply() {
        let out = "Sure.\n```json\n[{\"text\": \"Project Nightjar\", \"category\": \"other\", \"reason\": \"codename\"}]\n```";
        let f = parse_findings(out).unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].text, "Project Nightjar");
        assert!(parse_findings("[]").unwrap().is_empty());
        assert!(parse_findings("nothing").is_err());
    }

    #[test]
    fn prompt_contains_document() {
        let p = build_prompt("setup.md", "hello");
        assert!(p.contains("`setup.md`"));
        assert!(p.contains("<document>\nhello\n</document>"));
    }

    #[cfg(unix)]
    #[test]
    fn runs_custom_command_and_filters_findings() {
        let cfg = AgentConfig {
            kind: None,
            command: Some(
                r#"cat >/dev/null; echo '[{"text":"project nightjar","category":"other","reason":"codename"},{"text":"not in doc","reason":"x"},{"text":"[REDACTED:email-1]"}]'"#
                    .into(),
            ),
            timeout: Duration::from_secs(10),
        };
        let f = review(&cfg, "a.md", "Ship Project Nightjar to [REDACTED:email-1].").unwrap();
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].text, "project nightjar");
    }

    #[cfg(unix)]
    #[test]
    fn failing_or_slow_agent_is_an_error() {
        let fail = AgentConfig {
            kind: None,
            command: Some("cat >/dev/null; echo boom >&2; exit 3".into()),
            timeout: Duration::from_secs(10),
        };
        let err = review(&fail, "a.md", "x").unwrap_err().to_string();
        assert!(err.contains("boom"), "{err}");
        let slow = AgentConfig {
            kind: None,
            command: Some("sleep 5".into()),
            timeout: Duration::from_millis(200),
        };
        assert!(review(&slow, "a.md", "x")
            .unwrap_err()
            .to_string()
            .contains("timed out"));
    }
}
