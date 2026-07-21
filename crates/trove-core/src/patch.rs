use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatchRecord {
    pub id: String,
    pub path: String,
    pub before_hash: String,
    pub after_hash: String,
    pub reason: String,
    pub related_issue: Option<String>,
    pub related_pr: Option<String>,
    pub author: Option<String>,
    pub date: DateTime<Utc>,
    pub future_impact: Option<String>,
}
