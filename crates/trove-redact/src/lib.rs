//! Shared redaction detection, extracted verbatim from trove-ship: the
//! deterministic detector registry (detectors.rs), the `.troveignore`
//! allow/deny rules that tune it (troveignore.rs), and the shared types.
//! trove-ship keeps only the redaction pipeline (Redactor, verification,
//! agent review) and re-exports everything from here.

pub mod detectors;
pub mod troveignore;

use std::collections::BTreeMap;

use serde::Serialize;

pub use detectors::{Detection, Detector, LocalIdentity};
pub use troveignore::TroveIgnore;

/// What kind of private information a finding is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Secret,
    Email,
    Phone,
    InternalHost,
    IpAddress,
    LocalPath,
    Username,
    CustomerName,
    Denylisted,
    AgentFlagged,
}

impl Category {
    pub fn label(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::InternalHost => "internal-host",
            Self::IpAddress => "ip-address",
            Self::LocalPath => "local-path",
            Self::Username => "username",
            Self::CustomerName => "customer-name",
            Self::Denylisted => "denylisted",
            Self::AgentFlagged => "agent-flagged",
        }
    }

    /// Short tag used inside placeholders such as `[REDACTED:email-1]`.
    pub fn tag(self) -> &'static str {
        match self {
            Self::Secret => "secret",
            Self::Email => "email",
            Self::Phone => "phone",
            Self::InternalHost => "host",
            Self::IpAddress => "ip",
            Self::LocalPath => "path",
            Self::Username => "user",
            Self::CustomerName => "customer",
            Self::Denylisted => "term",
            Self::AgentFlagged => "private",
        }
    }

    /// Whether two spellings that differ only in case are the same value.
    pub fn case_insensitive(self) -> bool {
        !matches!(self, Self::Secret)
    }

    pub fn parse(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().replace('_', "-").as_str() {
            "secret" | "token" | "credential" | "password" | "key" => Self::Secret,
            "email" => Self::Email,
            "phone" | "phone-number" => Self::Phone,
            "host" | "hostname" | "internal-host" | "domain" | "url" => Self::InternalHost,
            "ip" | "ip-address" => Self::IpAddress,
            "path" | "local-path" => Self::LocalPath,
            "user" | "username" | "person" | "name" => Self::Username,
            "customer" | "customer-name" | "client" | "company" => Self::CustomerName,
            _ => Self::AgentFlagged,
        }
    }
}

/// How sure a detector is. High-confidence secrets are what the fail-closed
/// verification refuses to let through.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    Low,
    Medium,
    High,
}

/// Result of redacting a provider-bound prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmRedaction {
    /// The text with every required detection replaced by placeholders.
    pub text: String,
    /// Total number of replacements made.
    pub total: usize,
    /// Replacements per category tag ("secret", "email", ...).
    pub by_category: BTreeMap<String, usize>,
}

/// Redact secret-shaped text before it is sent to a provider. Secrets
/// (Category::Secret) are ALWAYS replaced — there is no opt-out. Other
/// private categories are replaced as well unless `keep_non_secret` is set.
/// Placeholders carry only the category tag and an index; they cannot
/// contain any part of the original value. After replacement the text is
/// re-scanned; any residual Secret finding — or any internal inconsistency
/// — aborts with an error, so nothing is ever sent unredacted.
pub fn redact_for_llm(text: &str, keep_non_secret: bool) -> anyhow::Result<LlmRedaction> {
    redact_with(
        &detectors::builtin().iter().collect::<Vec<_>>(),
        text,
        keep_non_secret,
    )
}

/// The `redact_for_llm` boundary with an explicit detector set, so callers
/// (and tests) can extend the rule set without replacing the real ones.
pub fn redact_with(
    detectors: &[&Detector],
    text: &str,
    keep_non_secret: bool,
) -> anyhow::Result<LlmRedaction> {
    use anyhow::bail;

    // 1. Scan with the given (real, possibly extended) detector set.
    let mut found = Vec::new();
    for d in detectors {
        d.run(text, &mut found);
    }

    // 2. Select: secrets always; other private categories unless opted out.
    let mut spans: Vec<(usize, usize, Category, Option<String>)> = found
        .into_iter()
        .filter(|d| d.category == Category::Secret || !keep_non_secret)
        .map(|d| (d.start, d.end, d.category, d.replacement.clone()))
        .collect();
    if spans.is_empty() {
        return Ok(LlmRedaction {
            text: text.to_string(),
            total: 0,
            by_category: BTreeMap::new(),
        });
    }

    // Secrets sort first at equal start so a merged span is tagged secret.
    spans.sort_by_key(|(s, e, c, _)| (*s, *e, *c != Category::Secret));

    // 3. Merge overlaps, replace left to right. Placeholders carry only the
    // category tag and a per-tag index — never any part of the value.
    let mut merged: Vec<(usize, usize, Category, Option<String>)> = Vec::new();
    for (s, e, c, rep) in spans {
        match merged.last_mut() {
            Some(last) if s < last.1 => {
                if e > last.1 {
                    last.1 = e;
                }
            }
            _ => merged.push((s, e, c, rep)),
        }
    }
    let mut out = String::with_capacity(text.len());
    let mut cursor = 0usize;
    let mut total = 0usize;
    let mut counts: BTreeMap<Category, usize> = BTreeMap::new();
    let mut tags: BTreeMap<Category, usize> = BTreeMap::new();
    for (s, e, c, rep) in &merged {
        if *s < cursor || *e > text.len() || *s > *e {
            bail!("redaction span {s}..{e} is inconsistent; aborting before provider send");
        }
        out.push_str(&text[cursor..*s]);
        let replacement = match rep {
            Some(fixed) => fixed.clone(),
            None => {
                let n = tags.entry(*c).or_insert(0);
                *n += 1;
                format!("[REDACTED:{}-{}]", c.tag(), n)
            }
        };
        out.push_str(&replacement);
        *counts.entry(*c).or_insert(0) += 1;
        total += 1;
        cursor = *e;
    }
    out.push_str(&text[cursor..]);

    // 4. Re-scan the result; a residual secret means we never send.
    let mut residual = Vec::new();
    for d in detectors {
        d.run(&out, &mut residual);
    }
    let survivors: Vec<&Detection> = residual
        .iter()
        .filter(|d| d.category == Category::Secret)
        .collect();
    if !survivors.is_empty() {
        bail!(
            "{} secret-shaped finding(s) survived redaction; aborting before provider send",
            survivors.len()
        );
    }

    let by_category = counts
        .into_iter()
        .map(|(c, n)| (c.tag().to_string(), n))
        .collect();
    Ok(LlmRedaction {
        text: out,
        total,
        by_category,
    })
}

#[cfg(test)]
mod llm_redact_tests {
    use super::detectors::builtin;
    use super::*;

    fn cat(p: &[&str]) -> String {
        p.concat()
    }

    #[test]
    fn always_removes_secrets_and_counts() {
        let key = cat(&["sk-", &"Ab12Cd34Ef56Gh78".repeat(3)]);
        let phrase = cat(&["correct ", "horse staple ", "42"]);
        let text =
            format!("ops note: key {key} and password = \"{phrase}\" plus ops@northwind.io email");
        let red = redact_for_llm(&text, false).expect("redaction succeeds");
        assert!(red.total >= 3, "secret x2 + email: {}", red.total);
        assert!(red.text.contains("[REDACTED:secret-1]"), "{}", red.text);
        assert!(red.text.contains("[REDACTED:email-1]"), "{}", red.text);
        assert!(!red.text.contains(&key));
        assert!(!red.text.contains(&phrase));
        assert!(!red.text.contains("ops@northwind.io"));
        assert_eq!(red.by_category.get("secret"), Some(&2));
    }

    #[test]
    fn non_secret_opt_out_still_redacts_secrets() {
        let key = cat(&["sk-", &"Ab12Cd34Ef56Gh78".repeat(3)]);
        let text = format!("key {key} and ops@northwind.io email");
        let red = redact_for_llm(&text, true).expect("redaction succeeds");
        assert!(!red.text.contains(&key), "secrets have no opt-out");
        assert!(
            red.text.contains("ops@northwind.io"),
            "non-secret categories are kept when opted out"
        );
    }

    #[test]
    fn placeholders_never_contain_value_parts() {
        let key = cat(&["sk-", &"Ab12Cd34Ef56Gh78".repeat(3)]);
        let red = redact_for_llm(&format!("key {key}"), false).unwrap();
        // The placeholder is tag+index only: no fragment of the value survives.
        assert!(!red.text.contains("Ab12"));
        assert!(!red.text.contains("sk-"));
        assert!(red.text.contains("[REDACTED:secret-1]"));
    }

    #[test]
    fn injected_residual_secret_aborts_before_send() {
        // Real redaction machinery + real detectors, plus ONE extra rule that
        // flags the placeholder text itself as a Secret. After replacement
        // the re-scan must find the residual and abort — nothing is returned.
        let key = cat(&["sk-", &"Ab12Cd34Ef56Gh78".repeat(3)]);
        let extra = Detector::new(
            "test-placeholder-is-secret",
            Box::new(|text: &str, out: &mut Vec<Detection>| {
                if let Some(i) = text.find("[REDACTED:") {
                    out.push(Detection {
                        start: i,
                        end: text.len(),
                        category: Category::Secret,
                        confidence: Confidence::High,
                        detector: "test-placeholder-is-secret",
                        reason: "placeholder flagged by injected rule",
                        replacement: None,
                    });
                }
            }),
        );
        let mut dets: Vec<&Detector> = builtin().iter().collect();
        dets.push(&extra);
        let err = redact_with(&dets, &format!("key {key}"), false).unwrap_err();
        assert!(err.to_string().to_lowercase().contains("secret"), "{err}");
    }
}
