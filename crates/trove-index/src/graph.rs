use std::collections::{HashMap, HashSet, VecDeque};
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::symbol::Symbol;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNode {
    pub id: String,
    pub path: String,
    pub kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DependencyGraph {
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

impl DependencyGraph {
    pub fn from_symbols(symbols: &[Symbol], file_imports: &HashMap<String, Vec<String>>) -> Self {
        let mut nodes = Vec::new();
        let mut edges = Vec::new();
        let mut seen_nodes = HashSet::new();

        for sym in symbols {
            if seen_nodes.insert(sym.path.clone()) {
                nodes.push(GraphNode {
                    id: sym.path.clone(),
                    path: sym.path.clone(),
                    kind: "file".into(),
                });
            }
            nodes.push(GraphNode {
                id: sym.id.clone(),
                path: sym.path.clone(),
                kind: sym.kind.label().into(),
            });
        }

        for (file, imports) in file_imports {
            for import in imports {
                let target = resolve_import(file, import);
                edges.push(GraphEdge {
                    from: file.clone(),
                    to: target,
                    kind: "import".into(),
                });
            }
        }

        for sym in symbols {
            for dep in &sym.dependencies {
                edges.push(GraphEdge {
                    from: sym.id.clone(),
                    to: dep.clone(),
                    kind: "reference".into(),
                });
            }
        }

        Self { nodes, edges }
    }

    pub fn neighbors(&self, node_id: &str) -> Vec<&GraphNode> {
        let mut ids = HashSet::new();
        for edge in &self.edges {
            if edge.from == node_id {
                ids.insert(edge.to.as_str());
            }
            if edge.to == node_id {
                ids.insert(edge.from.as_str());
            }
        }
        self.nodes
            .iter()
            .filter(|n| ids.contains(n.id.as_str()))
            .collect()
    }

    pub fn expand(&self, seeds: &[String], depth: usize) -> Vec<String> {
        let mut visited = HashSet::new();
        let mut queue: VecDeque<(String, usize)> = seeds
            .iter()
            .map(|s| (s.clone(), 0))
            .collect();

        while let Some((id, d)) = queue.pop_front() {
            if !visited.insert(id.clone()) {
                continue;
            }
            if d >= depth {
                continue;
            }
            for edge in &self.edges {
                if edge.from == id {
                    queue.push_back((edge.to.clone(), d + 1));
                }
                if edge.to == id {
                    queue.push_back((edge.from.clone(), d + 1));
                }
            }
        }
        visited.into_iter().collect()
    }
}

fn resolve_import(from_file: &str, import_text: &str) -> String {
    let parent = Path::new(from_file)
        .parent()
        .map(|p| p.to_string_lossy().to_string())
        .unwrap_or_default();

    let cleaned = import_text
        .trim()
        .trim_start_matches("use ")
        .trim_start_matches("import ")
        .trim_start_matches("from ")
        .trim_end_matches(';')
        .split_whitespace()
        .last()
        .unwrap_or(import_text)
        .trim_matches(|c| c == '"' || c == '\'' || c == '{' || c == '}')
        .to_string();

    if cleaned.starts_with('.') || cleaned.starts_with('/') {
        format!("{parent}/{cleaned}")
    } else {
        cleaned
    }
}
