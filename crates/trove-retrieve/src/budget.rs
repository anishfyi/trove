use serde::{Deserialize, Serialize};

/// Context budget allocation for a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContextBudget {
    pub total_tokens: usize,
    pub working_memory: usize,
    pub architecture: usize,
    pub active_files: usize,
    pub dependency_expansion: usize,
    pub code: usize,
    pub tool_output: usize,
}

impl ContextBudget {
    pub fn for_total(total: usize) -> Self {
        Self {
            total_tokens: total,
            working_memory: total * 10 / 100,
            architecture: total * 15 / 100,
            active_files: total * 20 / 100,
            dependency_expansion: total * 20 / 100,
            code: total * 20 / 100,
            tool_output: total * 15 / 100,
        }
    }

    pub fn estimate_tokens(text: &str) -> usize {
        // Rough heuristic: ~4 chars per token
        text.len() / 4 + 1
    }
}
