use serde::{Deserialize, Serialize};

/// L0 through L6 memory hierarchy levels.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryLevel {
    L0Working = 0,
    L1ActiveSymbols = 1,
    L2Module = 2,
    L3Subsystem = 3,
    L4Architecture = 4,
    L5Historical = 5,
    L6Repository = 6,
}

impl MemoryLevel {
    pub fn label(self) -> &'static str {
        match self {
            Self::L0Working => "L0 Working Memory",
            Self::L1ActiveSymbols => "L1 Active Symbols",
            Self::L2Module => "L2 Module Memory",
            Self::L3Subsystem => "L3 Subsystem Memory",
            Self::L4Architecture => "L4 Architecture Memory",
            Self::L5Historical => "L5 Historical Memory",
            Self::L6Repository => "L6 Repository Memory",
        }
    }

    pub fn default_token_budget(self) -> usize {
        match self {
            Self::L0Working => 20_000,
            Self::L1ActiveSymbols => 30_000,
            Self::L2Module => 40_000,
            Self::L3Subsystem => 30_000,
            Self::L4Architecture => 30_000,
            Self::L5Historical => 20_000,
            Self::L6Repository => 10_000,
        }
    }
}
