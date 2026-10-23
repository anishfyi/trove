//! `.troveignore`: per-project rules for what ships and what is redacted.
//!
//! ```text
//! # Bare lines are gitignore-style paths that never enter the docs.
//! secrets/
//! *.pem
//!
//! # Directives tune the redaction pass. One per line, no trailing comments.
//! allow: oss@acme.dev
//! allow-regex: build-\d+\.lan
//! allow-domain: acme.dev
//! deny: Project Nightjar
//! deny-regex: TICKET-\d+
//! customer: Globex
//! internal-domain: acme-billing.com
//! ```
//!
//! `allow` keeps an exact value, `allow-regex` keeps values that fully match,
//! `allow-domain` keeps emails and hosts on a domain. `deny` and `deny-regex`
//! always redact, `customer` names a customer, `internal-domain` marks hosts on
//! a domain as internal.
//!
//! High-confidence secrets can never be allowlisted.

use std::path::Path;

use anyhow::{bail, Context, Result};
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use regex::{Regex, RegexBuilder};

use super::Category;

pub const FILE_NAME: &str = ".troveignore";

#[derive(Debug, Clone, Default)]
pub struct TroveIgnore {
    pub allow: Vec<(usize, String)>,
    pub allow_regex: Vec<(usize, Regex)>,
    pub allow_domains: Vec<(usize, String)>,
    pub deny: Vec<String>,
    pub deny_regex: Vec<Regex>,
    pub customers: Vec<String>,
    pub internal_domains: Vec<String>,
    pub path_patterns: Vec<String>,
    paths: Option<Gitignore>,
}

impl TroveIgnore {
    /// Load `<repo>/.troveignore` if it exists.
    pub fn load(repo_root: &Path) -> Result<Self> {
        let path = repo_root.join(FILE_NAME);
        if !path.exists() {
            return Ok(Self::default());
        }
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        Self::parse(&text).with_context(|| format!("parse {}", path.display()))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let mut out = Self::default();
        let mut builder = GitignoreBuilder::new("");
        for (idx, raw) in text.lines().enumerate() {
            let lineno = idx + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key, value)) = split_directive(line) {
                let value = value.trim();
                if value.is_empty() {
                    bail!("line {lineno}: `{key}:` needs a value");
                }
                match key {
                    "allow" => out.allow.push((lineno, value.to_string())),
                    "allow-regex" => out.allow_regex.push((lineno, anchored(value, lineno)?)),
                    "allow-domain" => out
                        .allow_domains
                        .push((lineno, value.trim_start_matches('.').to_ascii_lowercase())),
                    "deny" => out.deny.push(value.to_string()),
                    "deny-regex" => out.deny_regex.push(
                        Regex::new(value).with_context(|| format!("line {lineno}: bad regex"))?,
                    ),
                    "customer" => out.customers.push(value.to_string()),
                    "internal-domain" => out
                        .internal_domains
                        .push(value.trim_start_matches('.').to_ascii_lowercase()),
                    other => bail!("line {lineno}: unknown directive `{other}:`"),
                }
                continue;
            }
            builder
                .add_line(None, line)
                .with_context(|| format!("line {lineno}: bad path pattern"))?;
            out.path_patterns.push(line.to_string());
        }
        if !out.path_patterns.is_empty() {
            out.paths = Some(builder.build()?);
        }
        Ok(out)
    }

    /// Whether a repo-relative path is excluded from the docs.
    pub fn excludes_path(&self, rel: &str, is_dir: bool) -> bool {
        match &self.paths {
            Some(gi) => gi
                .matched_path_or_any_parents(rel.trim_start_matches("./"), is_dir)
                .is_ignore(),
            None => false,
        }
    }

    /// The rule that allows `value`, described without repeating the value.
    pub fn allows(&self, value: &str, category: Category) -> Option<String> {
        if let Some((line, _)) = self
            .allow
            .iter()
            .find(|(_, a)| a.eq_ignore_ascii_case(value))
        {
            return Some(format!("{FILE_NAME}:{line} (allow)"));
        }
        if let Some((line, _)) = self.allow_regex.iter().find(|(_, re)| re.is_match(value)) {
            return Some(format!("{FILE_NAME}:{line} (allow-regex)"));
        }
        let host = match category {
            Category::Email => value.rsplit_once('@').map(|(_, d)| d),
            Category::InternalHost => Some(value),
            _ => None,
        }?;
        let host = host.to_ascii_lowercase();
        self.allow_domains
            .iter()
            .find(|(_, d)| host == *d || host.ends_with(&format!(".{d}")))
            .map(|(line, _)| format!("{FILE_NAME}:{line} (allow-domain)"))
    }
}

/// `key: value` where key is a lowercase word, as opposed to a path pattern.
fn split_directive(line: &str) -> Option<(&str, &str)> {
    let (key, value) = line.split_once(':')?;
    let is_key = !key.is_empty() && key.chars().all(|c| c.is_ascii_lowercase() || c == '-');
    (is_key && (value.is_empty() || value.starts_with([' ', '\t']))).then_some((key, value))
}

fn anchored(pattern: &str, lineno: usize) -> Result<Regex> {
    RegexBuilder::new(&format!("^(?:{pattern})$"))
        .case_insensitive(true)
        .build()
        .with_context(|| format!("line {lineno}: bad regex"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
# comment
secrets/
*.pem
allow: oss@acme.dev
allow-regex: build-\\d+\\.lan
allow-domain: acme.dev
deny: Project Nightjar
deny-regex: TICKET-\\d+
customer: Globex
internal-domain: .acme-billing.com
";

    #[test]
    fn parses_every_directive() {
        let t = TroveIgnore::parse(SAMPLE).unwrap();
        assert_eq!(t.allow.len(), 1);
        assert_eq!(t.allow_regex.len(), 1);
        assert_eq!(t.allow_domains[0].1, "acme.dev");
        assert_eq!(t.deny, vec!["Project Nightjar"]);
        assert_eq!(t.deny_regex.len(), 1);
        assert_eq!(t.customers, vec!["Globex"]);
        assert_eq!(t.internal_domains, vec!["acme-billing.com"]);
        assert_eq!(t.path_patterns, vec!["secrets/", "*.pem"]);
    }

    #[test]
    fn path_patterns_exclude_files_and_dirs() {
        let t = TroveIgnore::parse(SAMPLE).unwrap();
        assert!(t.excludes_path("secrets/prod.env", false));
        assert!(t.excludes_path("secrets", true));
        assert!(t.excludes_path("deploy/server.pem", false));
        assert!(!t.excludes_path("src/main.rs", false));
        assert!(!TroveIgnore::default().excludes_path("secrets/x", false));
    }

    #[test]
    fn allow_rules_match_by_kind() {
        let t = TroveIgnore::parse(SAMPLE).unwrap();
        assert_eq!(
            t.allows("OSS@acme.dev", Category::Email).as_deref(),
            Some(".troveignore:4 (allow)")
        );
        assert!(t
            .allows("build-12.lan", Category::InternalHost)
            .unwrap()
            .contains("allow-regex"));
        assert!(t
            .allows("build-12.lan.evil", Category::InternalHost)
            .is_none());
        assert!(t
            .allows("jane@docs.acme.dev", Category::Email)
            .unwrap()
            .contains("allow-domain"));
        assert!(t.allows("ci.acme.dev", Category::InternalHost).is_some());
        assert!(t.allows("acme.dev", Category::Phone).is_none());
        assert!(t.allows("jane@notacme.dev", Category::Email).is_none());
    }

    #[test]
    fn rejects_unknown_directives_and_bad_regex() {
        assert!(TroveIgnore::parse("alow: x").is_err());
        assert!(TroveIgnore::parse("deny-regex: (").is_err());
        assert!(TroveIgnore::parse("deny:").is_err());
        assert!(TroveIgnore::parse("C:/odd/path").is_ok());
    }
}
