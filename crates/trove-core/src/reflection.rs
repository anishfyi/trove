use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// What a stored statement claims. Consumed by provenance invalidation in a
/// later milestone; recorded now so every event is attributable.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Observation,
    Hypothesis,
    Decision,
}

impl RecordKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Observation => "observation",
            Self::Hypothesis => "hypothesis",
            Self::Decision => "decision",
        }
    }
}

impl FromStr for RecordKind {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "observation" => Ok(Self::Observation),
            "hypothesis" => Ok(Self::Hypothesis),
            "decision" => Ok(Self::Decision),
            _ => Err(()),
        }
    }
}

/// Who produced a record or event and what it was derived from.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Provenance {
    pub project: String,
    pub agent: String,
    pub run_id: String,
    /// URIs, `file:line` refs, or memory object ids the statement draws on.
    pub source_refs: Vec<String>,
    /// Content hash of the source material (see `store::content_hash`).
    pub source_hash: Option<String>,
}

/// Input for `TroveStore::submit_event`. `submission_id` is the idempotency
/// key: resubmitting it returns the stored event instead of writing twice.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSubmission {
    pub submission_id: String,
    pub kind: RecordKind,
    pub body: String,
    /// `event_id` of the event this corrects. The original is never modified.
    pub supersedes: Option<String>,
    pub provenance: Provenance,
}

/// An immutable, append-only reflection event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReflectionEvent {
    pub event_id: String,
    pub submission_id: String,
    pub kind: RecordKind,
    pub body: String,
    pub supersedes: Option<String>,
    pub provenance: Provenance,
    pub created_at: DateTime<Utc>,
}

/// A mutable record guarded by optimistic revision checks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryRecord {
    pub record_id: String,
    pub revision: u64,
    pub kind: RecordKind,
    pub body: String,
    pub provenance: Provenance,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

/// Input for `TroveStore::create_record`. Fails if the id is already taken.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NewRecord {
    pub record_id: String,
    pub kind: RecordKind,
    pub body: String,
    pub provenance: Provenance,
}

/// Input for `TroveStore::update_record`; bumps `revision` by one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecordUpdate {
    pub kind: RecordKind,
    pub body: String,
    pub provenance: Provenance,
}
