use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Confidence {
    pub score: f32,
    pub reason: String,
    pub retrieval_path: Vec<String>,
    pub distance: Option<f32>,
    pub last_verified: Option<String>,
}

impl Confidence {
    pub fn high(reason: impl Into<String>, path: Vec<String>) -> Self {
        Self {
            score: 0.9,
            reason: reason.into(),
            retrieval_path: path,
            distance: None,
            last_verified: None,
        }
    }

    pub fn medium(reason: impl Into<String>, path: Vec<String>) -> Self {
        Self {
            score: 0.6,
            reason: reason.into(),
            retrieval_path: path,
            distance: None,
            last_verified: None,
        }
    }
}
