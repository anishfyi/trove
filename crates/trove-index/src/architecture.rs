use trove_core::compression::CompressedText;
use trove_core::memory::Confidence;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::content_hash;

use crate::graph::DependencyGraph;
use crate::symbol::Symbol;

pub fn build_architecture(
    repo_name: &str,
    symbols: &[Symbol],
    subsystems: &[MemoryObject],
    graph: &DependencyGraph,
) -> MemoryObject {
    let top_subsystems: Vec<_> = {
        let mut sorted = subsystems.to_vec();
        sorted.sort_by(|a, b| {
            b.symbols
                .len()
                .cmp(&a.symbols.len())
                .then_with(|| a.id.cmp(&b.id))
        });
        sorted.into_iter().take(12).collect()
    };

    let raw = format!(
        "# Architecture: {repo_name}\n\n\
         ## Overview\n\
         Trove-indexed repository with {} symbols across {} subsystems.\n\
         Dependency graph: {} nodes, {} edges.\n\n\
         ## Subsystems\n{}\n\n\
         ## Data flow\n\
         Retrieval flows top-down: architecture -> subsystem -> module -> symbol -> source.\n\
         Dependency expansion follows graph neighbors only.\n\n\
         ## Patterns\n\
         - Progressive context: summaries before source\n\
         - Hash-tracked freshness on every object\n\
         - Symbol index as the source of truth for structure\n",
        symbols.len(),
        subsystems.len(),
        graph.nodes.len(),
        graph.edges.len(),
        top_subsystems
            .iter()
            .map(|s| {
                format!(
                    "- {} ({} symbols, {} modules)",
                    s.path.as_deref().unwrap_or("?"),
                    s.symbols.len(),
                    s.dependencies.len()
                )
            })
            .collect::<Vec<_>>()
            .join("\n"),
    );

    let hash = content_hash(raw.as_bytes());
    let mut obj = MemoryObject::new("architecture:root", MemoryKind::Architecture, hash);
    obj.path = Some(repo_name.to_string());
    obj.dependencies = top_subsystems.iter().map(|s| s.id.clone()).collect();
    obj.text = CompressedText::from_raw(raw);
    obj.text.compress_heuristic();
    obj.confidence = Some(Confidence::high(
        "synthesized from index",
        vec!["indexer".into(), "architecture".into()],
    ));
    obj.importance = 1.0;
    obj
}
