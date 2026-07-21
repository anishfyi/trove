use std::collections::HashMap;

use trove_core::compression::CompressedText;
use trove_core::memory::Confidence;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::content_hash;

use crate::symbol::{Symbol, SymbolKind};

#[derive(Debug, Clone)]
pub struct ModuleSummary {
    pub path: String,
    pub exports: Vec<String>,
    pub dependencies: Vec<String>,
    pub assumptions: Vec<String>,
    pub side_effects: Vec<String>,
}

pub fn build_module_memory(
    path: &str,
    content: &str,
    symbols: &[Symbol],
    imports: &[String],
) -> MemoryObject {
    let file_symbols: Vec<_> = symbols.iter().filter(|s| s.path == path).collect();
    let exports: Vec<String> = file_symbols
        .iter()
        .filter(|s| {
            matches!(
                s.kind,
                SymbolKind::Function
                    | SymbolKind::Struct
                    | SymbolKind::Class
                    | SymbolKind::Trait
                    | SymbolKind::Enum
            )
        })
        .map(|s| format!("{} ({})", s.name, s.kind.label()))
        .collect();

    let responsibilities = infer_responsibilities(path, &file_symbols);
    let assumptions = infer_assumptions(content);
    let side_effects = infer_side_effects(content);

    let summary = ModuleSummary {
        path: path.to_string(),
        exports: exports.clone(),
        dependencies: imports.to_vec(),
        assumptions: assumptions.clone(),
        side_effects: side_effects.clone(),
    };

    let raw = format_module_text(&summary, &responsibilities);
    let hash = content_hash(raw.as_bytes());
    let mut obj = MemoryObject::new(format!("module:{path}"), MemoryKind::Module, hash);
    obj.path = Some(path.to_string());
    obj.dependencies = imports.to_vec();
    obj.symbols = file_symbols.iter().map(|s| s.id.clone()).collect();
    obj.text = CompressedText::from_raw(raw);
    obj.text.compress_heuristic();
    obj.confidence = Some(Confidence::high(
        "derived from symbol index",
        vec!["indexer".into(), path.into()],
    ));
    obj.importance = (file_symbols.len() as f32 / 10.0).min(1.0);
    obj
}

fn format_module_text(summary: &ModuleSummary, responsibilities: &[String]) -> String {
    let mut out = format!("# Module: {}\n\n", summary.path);
    out.push_str("## Responsibilities\n");
    for r in responsibilities {
        out.push_str(&format!("- {r}\n"));
    }
    out.push_str("\n## Exports\n");
    for e in &summary.exports {
        out.push_str(&format!("- {e}\n"));
    }
    out.push_str("\n## Dependencies\n");
    for d in &summary.dependencies {
        out.push_str(&format!("- {d}\n"));
    }
    if !summary.assumptions.is_empty() {
        out.push_str("\n## Assumptions\n");
        for a in &summary.assumptions {
            out.push_str(&format!("- {a}\n"));
        }
    }
    if !summary.side_effects.is_empty() {
        out.push_str("\n## Side effects\n");
        for s in &summary.side_effects {
            out.push_str(&format!("- {s}\n"));
        }
    }
    out
}

fn infer_responsibilities(path: &str, symbols: &[&Symbol]) -> Vec<String> {
    let mut out = Vec::new();
    let stem = path.split('/').next().unwrap_or(path);
    out.push(format!("Package area: {stem}"));

    let kinds: HashMap<_, usize> = symbols.iter().fold(HashMap::new(), |mut acc, s| {
        *acc.entry(s.kind.label()).or_insert(0) += 1;
        acc
    });
    for (kind, count) in kinds {
        out.push(format!("{count} {kind}(s)"));
    }
    if out.len() == 1 {
        out.push("Supporting module".into());
    }
    out
}

fn infer_assumptions(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    if content.contains("unwrap()") || content.contains("expect(") {
        out.push("Uses infallible unwrap/expect in places".into());
    }
    if content.contains("TODO") || content.contains("FIXME") {
        out.push("Contains TODO/FIXME markers".into());
    }
    if content.contains("unsafe ") {
        out.push("Contains unsafe blocks".into());
    }
    out
}

fn infer_side_effects(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    if content.contains("fs::") || content.contains("open(") || content.contains("write(") {
        out.push("Filesystem I/O".into());
    }
    if content.contains("sql") || content.contains("query") || content.contains("db.") {
        out.push("Database access".into());
    }
    if content.contains("fetch(") || content.contains("http") || content.contains("reqwest") {
        out.push("Network I/O".into());
    }
    out
}
