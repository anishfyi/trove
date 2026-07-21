use std::collections::HashMap;

use trove_core::object::MemoryObject;

/// Lightweight semantic layer: keyword overlap scoring (embedding-ready).
#[derive(Debug, Default)]
pub struct SemanticIndex {
    vectors: HashMap<String, Vec<String>>,
}

impl SemanticIndex {
    pub fn from_objects(objects: &[MemoryObject]) -> Self {
        let mut vectors = HashMap::new();
        for obj in objects {
            let keywords = if !obj.text.keywords.is_empty() {
                obj.text.keywords.clone()
            } else {
                obj.text
                    .ultra_summary
                    .as_deref()
                    .or(obj.text.summary.as_deref())
                    .map(|s| {
                        s.split_whitespace()
                            .take(10)
                            .map(|w| w.to_lowercase())
                            .collect()
                    })
                    .unwrap_or_default()
            };
            vectors.insert(obj.id.clone(), keywords);
        }
        Self { vectors }
    }

    pub fn score(&self, object_id: &str, query_terms: &[String]) -> f32 {
        let Some(keywords) = self.vectors.get(object_id) else {
            return 0.0;
        };
        if query_terms.is_empty() {
            return 0.0;
        }
        let hits = query_terms
            .iter()
            .filter(|t| keywords.iter().any(|k| k.contains(t.as_str())))
            .count();
        hits as f32 / query_terms.len() as f32
    }
}
