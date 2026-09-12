use serde::{Deserialize, Serialize};

/// Progressive compression tiers. Models pick the smallest useful representation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompressionTier {
    Raw,
    Summary,
    UltraSummary,
    Keywords,
    Embedding,
}

impl CompressionTier {
    pub fn target_tokens(self) -> usize {
        match self {
            Self::Raw => usize::MAX,
            Self::Summary => 400,
            Self::UltraSummary => 100,
            Self::Keywords => 20,
            Self::Embedding => 0,
        }
    }

    pub fn next(self) -> Option<Self> {
        match self {
            Self::Raw => Some(Self::Summary),
            Self::Summary => Some(Self::UltraSummary),
            Self::UltraSummary => Some(Self::Keywords),
            Self::Keywords => Some(Self::Embedding),
            Self::Embedding => None,
        }
    }
}

/// Multi-resolution text stored on every memory object.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CompressedText {
    pub raw: Option<String>,
    pub summary: Option<String>,
    pub ultra_summary: Option<String>,
    pub keywords: Vec<String>,
    /// Opaque embedding vector (populated by semantic layer when available).
    pub embedding: Option<Vec<f32>>,
}

impl CompressedText {
    pub fn from_raw(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        Self {
            raw: Some(raw),
            ..Default::default()
        }
    }

    pub fn at_tier(&self, tier: CompressionTier) -> Option<&str> {
        match tier {
            CompressionTier::Raw => self.raw.as_deref(),
            CompressionTier::Summary => self.summary.as_deref().or(self.raw.as_deref()),
            CompressionTier::UltraSummary => self
                .ultra_summary
                .as_deref()
                .or(self.summary.as_deref())
                .or(self.raw.as_deref()),
            CompressionTier::Keywords => None, // keywords returned via keywords_line()
            CompressionTier::Embedding => None,
        }
    }

    pub fn keywords_line(&self) -> String {
        self.keywords.join(", ")
    }

    /// Heuristic compression without an LLM: extract structure from raw text.
    pub fn compress_heuristic(&mut self) {
        let Some(raw) = self.raw.as_ref() else {
            return;
        };

        if self.summary.is_none() {
            self.summary = Some(heuristic_summary(raw, 400));
        }
        if self.ultra_summary.is_none() {
            let summary = self.summary.as_deref().unwrap_or(raw);
            self.ultra_summary = Some(heuristic_summary(summary, 100));
        }
        if self.keywords.is_empty() {
            self.keywords = extract_keywords(raw);
        }
    }
}

fn heuristic_summary(text: &str, max_chars: usize) -> String {
    let lines: Vec<&str> = text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with("//") && !l.starts_with('#'))
        .take(12)
        .collect();

    let joined = lines.join(" ");
    if joined.len() <= max_chars {
        joined
    } else {
        format!("{}...", &joined[..max_chars.saturating_sub(3)])
    }
}

fn extract_keywords(text: &str) -> Vec<String> {
    let re = regex::Regex::new(r"\b[A-Za-z_][A-Za-z0-9_]{2,}\b").unwrap();
    let mut counts = std::collections::HashMap::new();
    for cap in re.find_iter(text) {
        let word = cap.as_str();
        if is_stopword(word) {
            continue;
        }
        *counts.entry(word.to_lowercase()).or_insert(0usize) += 1;
    }

    let mut ranked: Vec<_> = counts.into_iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    ranked.into_iter().take(5).map(|(w, _)| w).collect()
}

fn is_stopword(word: &str) -> bool {
    matches!(
        word.to_lowercase().as_str(),
        "the" | "and" | "for" | "with" | "this" | "that" | "from" | "return" | "true" | "false"
            | "null" | "none" | "some" | "let" | "mut" | "pub" | "use" | "fn" | "impl" | "struct"
            | "enum" | "mod" | "const" | "async" | "await" | "self" | "super" | "crate"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compression_tier_chain() {
        assert_eq!(CompressionTier::Raw.next(), Some(CompressionTier::Summary));
        assert_eq!(CompressionTier::Embedding.next(), None);
    }

    #[test]
    fn heuristic_compression_populates_tiers() {
        let mut text = CompressedText::from_raw(
            "fn authenticate(user: &User) -> Result<Session> {\n    verify_password(user)\n}",
        );
        text.compress_heuristic();
        assert!(text.summary.is_some());
        assert!(text.ultra_summary.is_some());
        assert!(!text.keywords.is_empty());
    }
}
