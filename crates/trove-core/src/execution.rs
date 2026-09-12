use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ExecutionEvent {
    OpenedFile { path: String, at: DateTime<Utc> },
    Search { query: String, at: DateTime<Utc> },
    Command { cmd: String, at: DateTime<Utc> },
    TestRun { cmd: String, passed: bool, at: DateTime<Utc> },
    Failure { message: String, at: DateTime<Utc> },
    ModifiedFile { path: String, at: DateTime<Utc> },
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ExecutionMemory {
    pub events: Vec<ExecutionEvent>,
}

impl ExecutionMemory {
    pub fn record(&mut self, event: ExecutionEvent) {
        self.events.push(event);
        if self.events.len() > 500 {
            let drain = self.events.len() - 500;
            self.events.drain(0..drain);
        }
    }

    pub fn recent_paths(&self) -> Vec<&str> {
        let mut paths = Vec::new();
        for event in self.events.iter().rev().take(50) {
            match event {
                ExecutionEvent::OpenedFile { path, .. }
                | ExecutionEvent::ModifiedFile { path, .. }
                    if !paths.contains(&path.as_str()) =>
                {
                    paths.push(path.as_str());
                }
                _ => {}
            }
        }
        paths
    }
}
