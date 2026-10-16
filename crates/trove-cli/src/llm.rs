//! Optional answer step. Retrieval never needs this; it only runs when a
//! provider is chosen. HTTP goes through the system `curl` so the binary
//! stays free of TLS dependencies, and secrets are passed to curl on stdin,
//! never in argv.

use std::io::Write;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use clap::ValueEnum;
use serde_json::{json, Value};

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum Provider {
    /// Retrieval only, fully offline.
    None,
    /// Claude via the Anthropic Messages API (ANTHROPIC_API_KEY).
    Anthropic,
    /// A local Ollama server (OLLAMA_HOST, default localhost:11434).
    Ollama,
    /// Any command reading the prompt on stdin (TROVE_LLM_CMD, default `claude -p`).
    Cmd,
}

impl Provider {
    pub fn label(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Anthropic => "anthropic",
            Self::Ollama => "ollama",
            Self::Cmd => "cmd",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        <Self as ValueEnum>::from_str(s, true).ok()
    }
}

pub const SYSTEM_PROMPT: &str = "You answer questions about the user's code, tools and notes using only the numbered sources provided. Cite sources inline as [n]. Be concise and concrete: name files, symbols and commands. If the sources do not contain the answer, say so plainly.";

const ANTHROPIC_DEFAULT_MODEL: &str = "claude-opus-5-5";
const OLLAMA_DEFAULT_MODEL: &str = "llama3.2";
const TIMEOUT_SECS: &str = "300";

pub fn complete(
    provider: Provider,
    model: Option<&str>,
    system: &str,
    user: &str,
) -> Result<String> {
    match provider {
        Provider::None => bail!("no provider selected"),
        Provider::Anthropic => anthropic(model, system, user),
        Provider::Ollama => ollama(model, system, user),
        Provider::Cmd => command(model, system, user),
    }
}

fn anthropic(model: Option<&str>, system: &str, user: &str) -> Result<String> {
    let model = model.unwrap_or(ANTHROPIC_DEFAULT_MODEL);
    let base =
        std::env::var("ANTHROPIC_BASE_URL").unwrap_or_else(|_| "https://api.anthropic.com".into());
    let mut headers = vec![
        "content-type: application/json".to_string(),
        "anthropic-version: 2023-06-01".to_string(),
    ];
    if let Some(key) = env_nonempty("ANTHROPIC_API_KEY") {
        headers.push(format!("x-api-key: {key}"));
    } else if let Some(token) = env_nonempty("ANTHROPIC_AUTH_TOKEN") {
        headers.push(format!("authorization: Bearer {token}"));
    } else {
        bail!("set ANTHROPIC_API_KEY (or ANTHROPIC_AUTH_TOKEN) to use --provider anthropic");
    }

    let mut body = json!({
        "model": model,
        "max_tokens": 16000,
        "system": system,
        "messages": [{"role": "user", "content": user}],
    });
    // Route refusals to a fallback model on models that support it.
    if matches!(
        model,
        "claude-opus-5-5" | "claude-opus-5" | "claude-fable-5-1" | "claude-sonnet-5-5"
    ) {
        headers.push("anthropic-beta: server-side-fallback-2026-07-01".into());
        body["fallbacks"] = json!("default");
    }

    let url = format!("{}/v1/messages", base.trim_end_matches('/'));
    let resp = curl_post(&url, &headers, &serde_json::to_vec(&body)?)?;
    if let Some(err) = resp.get("error") {
        bail!(
            "anthropic: {}",
            err.get("message")
                .and_then(Value::as_str)
                .unwrap_or("request failed")
        );
    }
    if resp.get("stop_reason").and_then(Value::as_str) == Some("refusal") {
        bail!("anthropic: the model declined to answer");
    }
    let text: Vec<&str> = resp
        .get("content")
        .and_then(Value::as_array)
        .map(|blocks| {
            blocks
                .iter()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|b| b.get("text").and_then(Value::as_str))
                .collect()
        })
        .unwrap_or_default();
    if text.is_empty() {
        bail!("anthropic: empty response");
    }
    Ok(text.join("").trim().to_string())
}

fn ollama(model: Option<&str>, system: &str, user: &str) -> Result<String> {
    let model = model
        .map(str::to_string)
        .or_else(|| env_nonempty("OLLAMA_MODEL"))
        .unwrap_or_else(|| OLLAMA_DEFAULT_MODEL.into());
    let host = env_nonempty("OLLAMA_HOST").unwrap_or_else(|| "http://localhost:11434".into());
    let host = if host.starts_with("http://") || host.starts_with("https://") {
        host
    } else {
        format!("http://{host}")
    };
    let body = json!({
        "model": model,
        "stream": false,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user},
        ],
    });
    let url = format!("{}/api/chat", host.trim_end_matches('/'));
    let resp = curl_post(
        &url,
        &["content-type: application/json".into()],
        &serde_json::to_vec(&body)?,
    )?;
    if let Some(err) = resp.get("error").and_then(Value::as_str) {
        bail!("ollama: {err}");
    }
    resp.pointer("/message/content")
        .and_then(Value::as_str)
        .map(|s| s.trim().to_string())
        .ok_or_else(|| anyhow!("ollama: unexpected response"))
}

fn command(model: Option<&str>, system: &str, user: &str) -> Result<String> {
    let cmd = env_nonempty("TROVE_LLM_CMD").unwrap_or_else(|| "claude -p".into());
    let mut child = Command::new("sh");
    child
        .arg("-c")
        .arg(&cmd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(m) = model {
        child.env("TROVE_MODEL", m);
    }
    let mut child = child.spawn().with_context(|| format!("run `{cmd}`"))?;
    let prompt = format!("{system}\n\n{user}");
    child
        .stdin
        .take()
        .ok_or_else(|| anyhow!("no stdin"))?
        .write_all(prompt.as_bytes())?;
    let out = child.wait_with_output()?;
    if !out.status.success() {
        bail!(
            "`{cmd}` failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    let text = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if text.is_empty() {
        bail!("`{cmd}` returned nothing");
    }
    Ok(text)
}

/// POST JSON with curl. Headers travel in a curl config read from stdin; the
/// body goes through a private temp file.
fn curl_post(url: &str, headers: &[String], body: &[u8]) -> Result<Value> {
    let body_path = std::env::temp_dir().join(format!(
        "trove-ask-{}-{}.json",
        std::process::id(),
        chrono::Utc::now().timestamp_nanos_opt().unwrap_or(0)
    ));
    write_private(&body_path, body)?;
    let config: String = headers
        .iter()
        .map(|h| {
            format!(
                "header = \"{}\"\n",
                h.replace('\\', "\\\\").replace('"', "\\\"")
            )
        })
        .collect();

    let result = (|| -> Result<Value> {
        let mut child = Command::new("curl")
            .args([
                "-sS",
                "--max-time",
                TIMEOUT_SECS,
                "-X",
                "POST",
                url,
                "--config",
                "-",
            ])
            .arg("--data-binary")
            .arg(format!("@{}", body_path.display()))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .context("run curl (is it installed?)")?;
        child
            .stdin
            .take()
            .ok_or_else(|| anyhow!("no stdin"))?
            .write_all(config.as_bytes())?;
        let out = child.wait_with_output()?;
        if !out.status.success() {
            bail!("curl: {}", String::from_utf8_lossy(&out.stderr).trim());
        }
        serde_json::from_slice(&out.stdout).with_context(|| {
            format!(
                "unexpected response: {}",
                String::from_utf8_lossy(&out.stdout)
                    .chars()
                    .take(200)
                    .collect::<String>()
            )
        })
    })();
    let _ = std::fs::remove_file(&body_path);
    result
}

fn write_private(path: &std::path::Path, data: &[u8]) -> Result<()> {
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    opts.open(path)?.write_all(data)?;
    Ok(())
}

fn env_nonempty(key: &str) -> Option<String> {
    std::env::var(key).ok().filter(|v| !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_names() {
        assert_eq!(Provider::parse("anthropic"), Some(Provider::Anthropic));
        assert_eq!(Provider::parse("CMD"), Some(Provider::Cmd));
        assert_eq!(Provider::parse("nope"), None);
    }

    #[cfg(unix)]
    #[test]
    fn command_provider_pipes_prompt() {
        std::env::set_var("TROVE_LLM_CMD", "wc -c");
        let out = complete(Provider::Cmd, None, "sys", "user").unwrap();
        std::env::remove_var("TROVE_LLM_CMD");
        assert_eq!(out.trim(), "9");
    }
}
