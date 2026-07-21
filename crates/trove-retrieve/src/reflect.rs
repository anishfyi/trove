use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reflection {
    pub lessons: Vec<String>,
    pub abstractions: Vec<String>,
    pub architecture_changes: Vec<String>,
    pub mistakes: Vec<String>,
    pub decisions: Vec<String>,
}

impl Reflection {
    pub fn from_session_summary(summary: &str) -> Self {
        Self {
            lessons: extract_bullets(summary, "lesson"),
            abstractions: extract_bullets(summary, "abstraction"),
            architecture_changes: extract_bullets(summary, "architecture"),
            mistakes: extract_bullets(summary, "mistake"),
            decisions: extract_bullets(summary, "decision"),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.lessons.is_empty()
            && self.abstractions.is_empty()
            && self.architecture_changes.is_empty()
            && self.mistakes.is_empty()
            && self.decisions.is_empty()
    }
}

fn extract_bullets(text: &str, keyword: &str) -> Vec<String> {
    text.lines()
        .filter(|l| l.to_lowercase().contains(keyword))
        .map(|l| l.trim().trim_start_matches('-').trim().to_string())
        .filter(|l| !l.is_empty())
        .take(5)
        .collect()
}
