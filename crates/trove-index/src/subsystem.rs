use std::collections::HashMap;

use trove_core::compression::CompressedText;
use trove_core::memory::Confidence;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::content_hash;

use crate::symbol::Symbol;

pub fn build_subsystems(symbols: &[Symbol], modules: &[MemoryObject]) -> Vec<MemoryObject> {
    let mut by_package: HashMap<String, Vec<&Symbol>> = HashMap::new();

    for sym in symbols {
        let pkg = package_of(&sym.path);
        by_package.entry(pkg).or_default().push(sym);
    }

    let mut out = Vec::new();
    for (pkg, syms) in by_package {
        let module_paths: Vec<String> = modules
            .iter()
            .filter(|m| m.path.as_deref().map(package_of) == Some(pkg.clone()))
            .filter_map(|m| m.path.clone())
            .collect();

        let kinds: HashMap<_, usize> = syms.iter().fold(HashMap::new(), |mut acc, s| {
            *acc.entry(s.kind.label()).or_insert(0) += 1;
            acc
        });

        let raw = format!(
            "# Subsystem: {pkg}\n\nFiles: {}\nSymbols: {}\n\n## Composition\n{}\n\n## Modules\n{}",
            module_paths.len(),
            syms.len(),
            kinds
                .iter()
                .map(|(k, v)| format!("- {v} {k}"))
                .collect::<Vec<_>>()
                .join("\n"),
            module_paths
                .iter()
                .map(|p| format!("- {p}"))
                .collect::<Vec<_>>()
                .join("\n"),
        );

        let hash = content_hash(raw.as_bytes());
        let mut obj = MemoryObject::new(format!("subsystem:{pkg}"), MemoryKind::Subsystem, hash);
        obj.path = Some(pkg.clone());
        obj.symbols = syms.iter().map(|s| s.id.clone()).collect();
        obj.dependencies = module_paths;
        obj.text = CompressedText::from_raw(raw);
        obj.text.compress_heuristic();
        obj.confidence = Some(Confidence::medium(
            "clustered by top-level package directory",
            vec!["indexer".into(), pkg],
        ));
        obj.importance = (syms.len() as f32 / 50.0).min(1.0);
        out.push(obj);
    }

    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

fn package_of(path: &str) -> String {
    path.split('/').next().unwrap_or(path).to_string()
}
