use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::compression::CompressedText;
use crate::hierarchy::MemoryLevel;
use crate::memory::Confidence;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Working,
    Symbol,
    Module,
    Subsystem,
    Architecture,
    Historical,
    Repository,
    Patch,
    Execution,
}

impl MemoryKind {
    pub const ALL: [Self; 9] = [
        Self::Working,
        Self::Symbol,
        Self::Module,
        Self::Subsystem,
        Self::Architecture,
        Self::Historical,
        Self::Repository,
        Self::Patch,
        Self::Execution,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Symbol => "symbol",
            Self::Module => "module",
            Self::Subsystem => "subsystem",
            Self::Architecture => "architecture",
            Self::Historical => "historical",
            Self::Repository => "repository",
            Self::Patch => "patch",
            Self::Execution => "execution",
        }
    }

    pub fn level(self) -> MemoryLevel {
        match self {
            Self::Working => MemoryLevel::L0Working,
            Self::Symbol => MemoryLevel::L1ActiveSymbols,
            Self::Module => MemoryLevel::L2Module,
            Self::Subsystem => MemoryLevel::L3Subsystem,
            Self::Architecture => MemoryLevel::L4Architecture,
            Self::Historical => MemoryLevel::L5Historical,
            Self::Repository => MemoryLevel::L6Repository,
            Self::Patch | Self::Execution => MemoryLevel::L5Historical,
        }
    }
}

/// Universal memory object stored at every hierarchy layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MemoryObject {
    pub id: String,
    pub kind: MemoryKind,
    pub path: Option<String>,
    pub text: CompressedText,
    pub dependencies: Vec<String>,
    pub symbols: Vec<String>,
    pub hash: String,
    pub updated_at: DateTime<Utc>,
    pub importance: f32,
    pub confidence: Option<Confidence>,
    pub risk: f32,
    pub git_commit: Option<String>,
}

impl MemoryObject {
    pub fn new(id: impl Into<String>, kind: MemoryKind, hash: impl Into<String>) -> Self {
        Self {
            id: id.into(),
            kind,
            path: None,
            text: CompressedText::default(),
            dependencies: Vec::new(),
            symbols: Vec::new(),
            hash: hash.into(),
            updated_at: Utc::now(),
            importance: 0.5,
            confidence: None,
            risk: 0.0,
            git_commit: None,
        }
    }
}
