//! Redaction pass: deterministic detectors, a `.troveignore` allow and deny list,
//! an optional coding-agent review, and a fail-closed verification step.
//!
//! The flow is two-phase so that a value found anywhere is removed everywhere:
//! `scan` every text first (detection only, values are registered), then `apply`
//! every text (detected spans plus every other occurrence of a known value).

pub mod agent;

use std::collections::BTreeMap;

use regex::{Regex, RegexBuilder};
use serde::Serialize;

// The detector registry, TroveIgnore rules and shared types live in the
// trove-redact crate; re-exported here so every historical
// `trove_ship::redact::*` path (incl. `redact::detectors::builtin`) keeps
// working unchanged.
pub use trove_redact::detectors;
pub use trove_redact::detectors::{Detection, LocalIdentity};
pub use trove_redact::troveignore;
pub use trove_redact::troveignore::TroveIgnore;
pub use trove_redact::{Category, Confidence};

/// Where one occurrence was removed from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Location {
    pub origin: String,
    pub line: usize,
}

/// A distinct private value and everything we know about why it was removed.
/// The value itself is kept in memory only and never serialized.
#[derive(Debug, Clone)]
struct KnownValue {
    value: String,
    category: Category,
    confidence: Confidence,
    detector: String,
    reason: String,
    replacement: String,
    occurrences: Vec<Location>,
    sweeper: Option<Regex>,
}

/// One row of the redaction report: a distinct removed value.
#[derive(Debug, Clone, Serialize)]
pub struct ReportItem {
    pub placeholder: String,
    pub category: Category,
    pub confidence: Confidence,
    pub detector: String,
    pub reason: String,
    /// Length of the removed value. The value itself is never written out.
    pub length: usize,
    pub occurrences: usize,
    pub locations: Vec<Location>,
}

/// A match that was kept because a `.troveignore` rule allows it.
#[derive(Debug, Clone, Serialize)]
pub struct AllowedItem {
    pub rule: String,
    pub category: Category,
    pub detector: String,
    pub occurrences: usize,
}

/// A high-confidence secret the allowlist tried to keep. Secrets cannot be
/// allowlisted, so these are redacted anyway and called out in the report.
#[derive(Debug, Clone, Serialize)]
pub struct OverriddenAllow {
    pub rule: String,
    pub detector: String,
}

#[derive(Debug, Clone, Default)]
pub struct RedactConfig {
    pub ignore: TroveIgnore,
    pub identity: Option<LocalIdentity>,
    /// Absolute paths (such as the repository root) replaced with a fixed label.
    pub path_labels: Vec<(String, String)>,
}

pub struct Redactor {
    ignore: TroveIgnore,
    /// Detectors built from config; the built-in set always runs first.
    extra: Vec<detectors::Detector>,
    values: Vec<KnownValue>,
    counters: BTreeMap<Category, usize>,
    allowed: BTreeMap<(String, Category, String), usize>,
    overridden: Vec<OverriddenAllow>,
}

impl Redactor {
    pub fn new(config: RedactConfig) -> Self {
        let mut extra = detectors::from_ignore(&config.ignore);
        if let Some(identity) = &config.identity {
            extra.extend(detectors::from_identity(identity));
        }
        extra.extend(detectors::from_path_labels(&config.path_labels));
        Self {
            ignore: config.ignore,
            extra,
            values: Vec::new(),
            counters: BTreeMap::new(),
            allowed: BTreeMap::new(),
            overridden: Vec::new(),
        }
    }

    /// Detection only: register every private value found in `text`.
    pub fn scan(&mut self, text: &str) {
        for det in self.detect(text) {
            let value = &text[det.start..det.end];
            if let Some(rule) = self.ignore.allows(value, det.category) {
                if det.category == Category::Secret && det.confidence == Confidence::High {
                    if !self.overridden.iter().any(|o| o.rule == rule) {
                        self.overridden.push(OverriddenAllow {
                            rule,
                            detector: det.detector.to_string(),
                        });
                    }
                } else {
                    *self
                        .allowed
                        .entry((rule, det.category, det.detector.to_string()))
                        .or_insert(0) += 1;
                    continue;
                }
            }
            self.register(
                value,
                det.category,
                det.confidence,
                det.detector,
                det.reason,
                det.replacement.clone(),
            );
        }
    }

    /// Register a value found outside the deterministic detectors (for example by
    /// the agent review). Returns false when the value is allowlisted or empty.
    pub fn add_value(
        &mut self,
        value: &str,
        category: Category,
        confidence: Confidence,
        detector: &str,
        reason: &str,
    ) -> bool {
        let value = value.trim();
        if value.is_empty() || value.starts_with("[REDACTED:") {
            return false;
        }
        if let Some(rule) = self.ignore.allows(value, category) {
            *self
                .allowed
                .entry((rule, category, detector.to_string()))
                .or_insert(0) += 1;
            return false;
        }
        self.register(value, category, confidence, detector, reason, None);
        true
    }

    fn register(
        &mut self,
        value: &str,
        category: Category,
        confidence: Confidence,
        detector: &str,
        reason: &str,
        replacement: Option<String>,
    ) {
        if let Some(existing) = self.find_value_mut(value, category) {
            if confidence > existing.confidence {
                existing.confidence = confidence;
                existing.detector = detector.to_string();
                existing.reason = reason.to_string();
            }
            return;
        }
        let replacement = replacement.unwrap_or_else(|| {
            let n = self.counters.entry(category).or_insert(0);
            *n += 1;
            format!("[REDACTED:{}-{}]", category.tag(), n)
        });
        let sweeper = sweeper_for(value, category);
        self.values.push(KnownValue {
            value: value.to_string(),
            category,
            confidence,
            detector: detector.to_string(),
            reason: reason.to_string(),
            replacement,
            occurrences: Vec::new(),
            sweeper,
        });
    }

    fn find_value_mut(&mut self, value: &str, category: Category) -> Option<&mut KnownValue> {
        self.values.iter_mut().find(|k| {
            if k.category.case_insensitive() || category.case_insensitive() {
                k.value.eq_ignore_ascii_case(value)
            } else {
                k.value == value
            }
        })
    }

    fn detect(&self, text: &str) -> Vec<Detection> {
        let mut out = Vec::new();
        for d in detectors::builtin().iter().chain(&self.extra) {
            d.run(text, &mut out);
        }
        out
    }

    /// Replace every known value in `text`, recording where each one was.
    pub fn apply(&mut self, origin: &str, text: &str) -> String {
        // Candidate spans: every occurrence of every known value.
        let mut spans: Vec<(usize, usize, usize)> = Vec::new();
        for (idx, kv) in self.values.iter().enumerate() {
            match &kv.sweeper {
                Some(re) => {
                    for m in re.find_iter(text) {
                        spans.push((m.start(), m.end(), idx));
                    }
                }
                None => {
                    let mut from = 0;
                    while let Some(pos) = text[from..].find(&kv.value) {
                        let start = from + pos;
                        spans.push((start, start + kv.value.len(), idx));
                        from = start + kv.value.len().max(1);
                    }
                }
            }
        }
        // Never touch text inside a value the allowlist keeps (for example the
        // domain part of an allowed email).
        let protected = self.allowed_spans(text);
        spans.retain(|&(s, e, _)| !protected.iter().any(|&(ps, pe)| ps <= s && e <= pe));
        let chosen = resolve_overlaps(spans, |idx| {
            (self.values[idx].confidence, self.values[idx].category)
        });

        let mut out = String::with_capacity(text.len());
        let mut cursor = 0;
        for (start, end, idx) in chosen {
            out.push_str(&text[cursor..start]);
            out.push_str(&self.values[idx].replacement);
            let line = text[..start].matches('\n').count() + 1;
            self.values[idx].occurrences.push(Location {
                origin: origin.to_string(),
                line,
            });
            cursor = end;
        }
        out.push_str(&text[cursor..]);
        out
    }

    fn allowed_spans(&self, text: &str) -> Vec<(usize, usize)> {
        self.detect(text)
            .into_iter()
            .filter(|d| !(d.category == Category::Secret && d.confidence == Confidence::High))
            .filter(|d| {
                self.ignore
                    .allows(&text[d.start..d.end], d.category)
                    .is_some()
            })
            .map(|d| (d.start, d.end))
            .collect()
    }

    /// Scan every text, then apply to every text, so a value found in one document
    /// is also removed from all the others.
    pub fn redact_all(&mut self, docs: &mut [(String, String)]) {
        for (_, text) in docs.iter() {
            self.scan(text);
        }
        for (origin, text) in docs.iter_mut() {
            *text = self.apply(origin, text);
        }
    }

    /// Replace known values in a short label (such as a file path in the report)
    /// without recording occurrences.
    pub fn sanitize_label(&self, label: &str) -> String {
        let mut out = label.to_string();
        for kv in &self.values {
            out = match &kv.sweeper {
                Some(re) => re.replace_all(&out, kv.replacement.as_str()).into_owned(),
                None => out.replace(&kv.value, &kv.replacement),
            };
        }
        out
    }

    pub fn items(&self) -> Vec<ReportItem> {
        let mut items: Vec<ReportItem> = self
            .values
            .iter()
            .filter(|k| !k.occurrences.is_empty())
            .map(|k| ReportItem {
                placeholder: k.replacement.clone(),
                category: k.category,
                confidence: k.confidence,
                detector: k.detector.clone(),
                reason: k.reason.clone(),
                length: k.value.chars().count(),
                occurrences: k.occurrences.len(),
                locations: dedup_locations(&k.occurrences),
            })
            .collect();
        let number = |p: &str| -> usize {
            p.trim_end_matches(']')
                .rsplit('-')
                .next()
                .and_then(|n| n.parse().ok())
                .unwrap_or(0)
        };
        items.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then(a.category.cmp(&b.category))
                .then(number(&a.placeholder).cmp(&number(&b.placeholder)))
                .then(a.detector.cmp(&b.detector))
        });
        items
    }

    pub fn allowed(&self) -> Vec<AllowedItem> {
        self.allowed
            .iter()
            .map(|((rule, category, detector), n)| AllowedItem {
                rule: rule.clone(),
                category: *category,
                detector: detector.clone(),
                occurrences: *n,
            })
            .collect()
    }

    pub fn overridden(&self) -> &[OverriddenAllow] {
        &self.overridden
    }
}

/// A secret that survived into the output. Carries no part of the value.
#[derive(Debug, Clone, Serialize)]
pub struct Survivor {
    pub file: String,
    pub line: usize,
    pub detector: String,
}

/// Fail-closed check over the exact bytes about to be written. Detection runs on
/// the raw text and on a normalized view (Markdown escapes and zero-width
/// characters removed), because a renderer will show the normalized form.
pub fn verify(files: &[(String, String)]) -> Result<(), Vec<Survivor>> {
    let dets = detectors::builtin();
    let mut survivors = Vec::new();
    for (name, content) in files {
        let normalized = normalize(content);
        for view in [content.as_str(), normalized.as_str()] {
            let mut found = Vec::new();
            for d in dets {
                d.run(view, &mut found);
            }
            for f in found {
                if f.category == Category::Secret && f.confidence == Confidence::High {
                    let line = view[..f.start].matches('\n').count() + 1;
                    let s = Survivor {
                        file: name.clone(),
                        line,
                        detector: f.detector.to_string(),
                    };
                    if !survivors.iter().any(|x: &Survivor| {
                        x.file == s.file && x.line == s.line && x.detector == s.detector
                    }) {
                        survivors.push(s);
                    }
                }
            }
        }
    }
    if survivors.is_empty() {
        Ok(())
    } else {
        Err(survivors)
    }
}

/// Remove Markdown backslash escapes, HTML entity escapes for underscores and
/// zero-width characters so that `sk\_live\_...` is seen as `sk_live_...`.
pub fn normalize(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{200B}' | '\u{200C}' | '\u{200D}' | '\u{2060}' | '\u{FEFF}' | '\u{00AD}' => {}
            '\\' => match chars.peek() {
                Some(n) if n.is_ascii_punctuation() => {
                    out.push(*n);
                    chars.next();
                }
                _ => out.push(c),
            },
            _ => out.push(c),
        }
    }
    out.replace("&#95;", "_").replace("&lowbar;", "_")
}

/// Overlap resolution. Overlapping spans are merged into one group so no part
/// of any detected value is left exposed; the group is replaced with the
/// placeholder of its longest member (ties: higher confidence, then the more
/// sensitive category). Returns the groups sorted by start.
fn resolve_overlaps<F>(mut spans: Vec<(usize, usize, usize)>, rank: F) -> Vec<(usize, usize, usize)>
where
    F: Fn(usize) -> (Confidence, Category),
{
    spans.retain(|s| s.0 < s.1);
    spans.sort_by_key(|s| (s.0, std::cmp::Reverse(s.1)));
    let better = |a: (usize, usize, usize), b: (usize, usize, usize)| {
        let (ca, ka) = rank(a.2);
        let (cb, kb) = rank(b.2);
        (a.1 - a.0)
            .cmp(&(b.1 - b.0))
            .then(ca.cmp(&cb))
            .then(kb.cmp(&ka))
            .is_gt()
    };
    // Each group: (merged start, merged end) and its representative span.
    type Group = ((usize, usize), (usize, usize, usize));
    let mut groups: Vec<Group> = Vec::new();
    for s in spans {
        match groups.last_mut() {
            Some((range, rep)) if s.0 < range.1 => {
                range.1 = range.1.max(s.1);
                if better(s, *rep) {
                    *rep = s;
                }
            }
            _ => groups.push(((s.0, s.1), s)),
        }
    }
    groups
        .into_iter()
        .map(|((start, end), rep)| (start, end, rep.2))
        .collect()
}

/// Case-insensitive, word-bounded matcher for values where that makes sense.
/// Secrets and values with no word characters at the edges use exact matching.
fn sweeper_for(value: &str, category: Category) -> Option<Regex> {
    if !category.case_insensitive() {
        return None;
    }
    let first = value.chars().next()?;
    let last = value.chars().last()?;
    let lead = if first.is_alphanumeric() || first == '_' {
        r"\b"
    } else {
        ""
    };
    let trail = if last.is_alphanumeric() || last == '_' {
        r"\b"
    } else {
        ""
    };
    RegexBuilder::new(&format!("{lead}{}{trail}", regex::escape(value)))
        .case_insensitive(true)
        .build()
        .ok()
}

fn dedup_locations(locs: &[Location]) -> Vec<Location> {
    let mut out: Vec<Location> = Vec::new();
    for l in locs {
        if !out.contains(l) {
            out.push(l.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn redact(text: &str) -> (String, Vec<ReportItem>) {
        let mut r = Redactor::new(RedactConfig::default());
        let mut docs = vec![("doc.md".to_string(), text.to_string())];
        r.redact_all(&mut docs);
        (docs.remove(0).1, r.items())
    }

    #[test]
    fn same_value_gets_same_placeholder_everywhere() {
        let mut r = Redactor::new(RedactConfig::default());
        let mut docs = vec![
            ("a.md".into(), "mail jane.doe@acme-billing.com today".into()),
            ("b.md".into(), "cc JANE.DOE@acme-billing.com".into()),
        ];
        r.redact_all(&mut docs);
        assert_eq!(docs[0].1, "mail [REDACTED:email-1] today");
        assert_eq!(docs[1].1, "cc [REDACTED:email-1]");
        let items = r.items();
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].occurrences, 2);
        assert_eq!(items[0].locations.len(), 2);
    }

    #[test]
    fn report_never_contains_the_value() {
        let secret = format!("ghp_{}", "Ab1".repeat(12));
        let (out, items) = redact(&format!("token {secret}"));
        assert!(!out.contains(&secret));
        let json = serde_json::to_string(&items).unwrap();
        assert!(!json.contains(&secret));
        assert_eq!(items[0].length, secret.len());
    }

    #[test]
    fn line_numbers_are_recorded() {
        let (_, items) = redact("one\ntwo\ncall +1 415 555 0132\n");
        assert_eq!(items[0].locations[0].line, 3);
    }

    #[test]
    fn overlaps_merge_so_nothing_leaks() {
        let rank = |i: usize| match i {
            0 => (Confidence::Medium, Category::Email),
            1 => (Confidence::High, Category::InternalHost),
            _ => (Confidence::Low, Category::IpAddress),
        };
        // An email containing a high-confidence domain: the email wins.
        assert_eq!(
            resolve_overlaps(vec![(0, 20, 0), (8, 20, 1)], rank),
            vec![(0, 20, 0)]
        );
        // Partial overlap: the union is replaced.
        assert_eq!(
            resolve_overlaps(vec![(0, 10, 1), (5, 30, 2)], rank),
            vec![(0, 30, 2)]
        );
        // Touching spans stay separate.
        assert_eq!(
            resolve_overlaps(vec![(0, 5, 0), (5, 9, 1)], rank),
            vec![(0, 5, 0), (5, 9, 1)]
        );
    }

    #[test]
    fn allowed_email_keeps_its_internal_domain() {
        let ignore =
            TroveIgnore::parse("allow: oss@acme-billing.com\ninternal-domain: acme-billing.com\n")
                .unwrap();
        let mut r = Redactor::new(RedactConfig {
            ignore,
            ..Default::default()
        });
        let mut docs = vec![(
            "a.md".into(),
            "mail oss@acme-billing.com or jane@acme-billing.com via ci.acme-billing.com".into(),
        )];
        r.redact_all(&mut docs);
        assert_eq!(
            docs[0].1,
            "mail oss@acme-billing.com or [REDACTED:email-1] via [REDACTED:host-2]"
        );
    }

    #[test]
    fn allowlist_keeps_values_but_not_secrets() {
        let secret = format!("glpat-{}", "x7Y".repeat(8));
        let ignore =
            TroveIgnore::parse(&format!("allow: oss@acme-billing.com\nallow: {secret}\n")).unwrap();
        let mut r = Redactor::new(RedactConfig {
            ignore,
            ..Default::default()
        });
        let mut docs = vec![(
            "a.md".into(),
            format!("write to oss@acme-billing.com, token {secret}"),
        )];
        r.redact_all(&mut docs);
        assert!(docs[0].1.contains("oss@acme-billing.com"));
        assert!(!docs[0].1.contains(&secret));
        assert_eq!(r.allowed().len(), 1);
        assert_eq!(r.overridden().len(), 1);
    }

    #[test]
    fn agent_values_are_swept() {
        let mut r = Redactor::new(RedactConfig::default());
        assert!(r.add_value(
            "Project Nightjar",
            Category::AgentFlagged,
            Confidence::Medium,
            "agent:test",
            "internal codename"
        ));
        let out = r.apply("a.md", "Ship project nightjar by Friday.");
        assert_eq!(out, "Ship [REDACTED:private-1] by Friday.");
        assert!(!r.add_value(
            "[REDACTED:x-1]",
            Category::Secret,
            Confidence::High,
            "a",
            "b"
        ));
    }

    #[test]
    fn verify_fails_closed_on_surviving_secret() {
        let secret = format!("sk_live_{}", "4eC39HqLyjWDarjtT1zdp7dc");
        let files = vec![("ok.md".to_string(), "nothing here".to_string())];
        assert!(verify(&files).is_ok());
        let files = vec![("bad.md".to_string(), format!("a\nkey {secret}"))];
        let err = verify(&files).unwrap_err();
        assert_eq!(err[0].file, "bad.md");
        assert_eq!(err[0].line, 2);
    }

    #[test]
    fn verify_sees_through_markdown_escapes() {
        let escaped = format!("sk\\_live\\_{}", "4eC39HqLyjWDarjtT1zdp7dc");
        let files = vec![("x.md".to_string(), escaped)];
        assert!(verify(&files).is_err());
    }

    #[test]
    fn normalize_strips_escapes_and_zero_width() {
        assert_eq!(normalize("a\\_b\u{200B}c &#95; \\d"), "a_bc _ \\d");
    }

    #[test]
    fn sanitize_label_replaces_known_values() {
        let ignore = TroveIgnore::parse("customer: Globex").unwrap();
        let mut r = Redactor::new(RedactConfig {
            ignore,
            ..Default::default()
        });
        r.scan("Globex");
        assert_eq!(
            r.sanitize_label("src/globex/billing.rs"),
            "src/[REDACTED:customer-1]/billing.rs"
        );
    }
}
