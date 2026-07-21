use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Freshness {
    pub last_modified: DateTime<Utc>,
    pub git_commit: Option<String>,
    pub hash: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Staleness {
    pub score: f32,
    pub reason: String,
}

impl Freshness {
    pub fn new(hash: impl Into<String>) -> Self {
        Self {
            last_modified: Utc::now(),
            git_commit: None,
            hash: hash.into(),
            confidence: 1.0,
        }
    }

    pub fn staleness(&self, current_hash: &str) -> Staleness {
        if self.hash == current_hash {
            Staleness {
                score: 0.0,
                reason: "hash matches".into(),
            }
        } else {
            Staleness {
                score: 1.0,
                reason: "content hash changed".into(),
            }
        }
    }
}
