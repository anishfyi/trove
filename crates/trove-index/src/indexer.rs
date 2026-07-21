use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::Result;
use chrono::Utc;
use ignore::WalkBuilder;
use trove_core::compression::CompressedText;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::{content_hash, TroveManifest, TroveStore};

use crate::architecture::build_architecture;
use crate::graph::DependencyGraph;
use crate::module::build_module_memory;
use crate::parser::parse_file;
use crate::subsystem::build_subsystems;
use crate::symbol::Symbol;

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub incremental: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self { incremental: true }
    }
}

#[derive(Debug, Clone)]
pub struct IndexReport {
    pub files_scanned: usize,
    pub symbols_indexed: usize,
    pub modules_built: usize,
    pub subsystems_built: usize,
    pub stale_rebuilt: usize,
}

pub struct Indexer {
    repo_root: PathBuf,
    store: TroveStore,
}

impl Indexer {
    pub fn open(repo_root: impl AsRef<Path>) -> Result<Self> {
        let repo_root = repo_root.as_ref().canonicalize()?;
        let store = TroveStore::open(&repo_root)?;
        Ok(Self { repo_root, store })
    }

    pub fn index(&self, options: IndexOptions) -> Result<IndexReport> {
        let mut files_scanned = 0usize;
        let mut stale_rebuilt = 0usize;

        let existing_symbols: Vec<Symbol> = if options.incremental {
            self.store.read_json("symbols.json")?.unwrap_or_default()
        } else {
            Vec::new()
        };
        let existing_modules: HashMap<String, MemoryObject> = if options.incremental {
            self.store
                .read_objects(MemoryKind::Module)?
                .into_iter()
                .filter_map(|m| m.path.clone().map(|p| (p, m)))
                .collect()
        } else {
            HashMap::new()
        };

        let mut all_symbols: Vec<Symbol> = Vec::new();
        let mut file_imports: HashMap<String, Vec<String>> = HashMap::new();
        let mut modules: Vec<MemoryObject> = Vec::new();
        let mut seen_paths = HashMap::new();

        let walker = WalkBuilder::new(&self.repo_root)
            .hidden(false)
            .git_ignore(true)
            .git_global(true)
            .git_exclude(true)
            .build();

        for entry in walker.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }
            if path.starts_with(self.repo_root.join(".trove")) {
                continue;
            }
            if path.starts_with(self.repo_root.join("target")) {
                continue;
            }
            if path.starts_with(self.repo_root.join(".git")) {
                continue;
            }

            let rel = path
                .strip_prefix(&self.repo_root)
                .unwrap_or(path)
                .to_string_lossy()
                .to_string();

            let content = match fs::read_to_string(path) {
                Ok(c) => c,
                Err(_) => continue,
            };

            files_scanned += 1;
            let hash = content_hash(content.as_bytes());
            seen_paths.insert(rel.clone(), hash.clone());

            if let Some(existing) = existing_modules.get(&rel) {
                if existing.hash == hash {
                    all_symbols.extend(
                        existing_symbols
                            .iter()
                            .filter(|s| s.path == rel)
                            .cloned(),
                    );
                    modules.push(existing.clone());
                    continue;
                }
                stale_rebuilt += 1;
            }

            if let Some(parsed) = parse_file(path, &rel, &content, &hash) {
                file_imports.insert(rel.clone(), parsed.imports);
                all_symbols.extend(parsed.symbols);
            }
            modules.push(build_module_memory(
                &rel,
                &content,
                &all_symbols,
                file_imports.get(&rel).cloned().unwrap_or_default().as_slice(),
            ));
        }

        // Drop modules for deleted files
        modules.retain(|m| m.path.as_ref().map(|p| seen_paths.contains_key(p)).unwrap_or(false));
        all_symbols.retain(|s| seen_paths.contains_key(&s.path));

        if all_symbols.is_empty() && modules.is_empty() {
            if let Ok(Some(manifest)) = self.store.read_manifest() {
                return Ok(IndexReport {
                    files_scanned: 0,
                    symbols_indexed: manifest.symbol_count,
                    modules_built: manifest.module_count,
                    subsystems_built: manifest.subsystem_count,
                    stale_rebuilt: 0,
                });
            }
        }

        let subsystems = build_subsystems(&all_symbols, &modules);
        let graph = DependencyGraph::from_symbols(&all_symbols, &file_imports);
        let repo_name = self
            .repo_root
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "repository".into());
        let architecture = build_architecture(&repo_name, &all_symbols, &subsystems, &graph);

        let symbol_objects: Vec<MemoryObject> = all_symbols.iter().map(symbol_to_memory).collect();

        self.store.write_objects(MemoryKind::Symbol, &symbol_objects)?;
        self.store.write_objects(MemoryKind::Module, &modules)?;
        self.store.write_objects(MemoryKind::Subsystem, &subsystems)?;
        self.store
            .write_objects(MemoryKind::Architecture, std::slice::from_ref(&architecture))?;
        self.store.write_json("graph.json", &graph)?;
        self.store.write_json("symbols.json", &all_symbols)?;

        let manifest = TroveManifest {
            version: "0.2.0".into(),
            repo_root: self.repo_root.to_string_lossy().into(),
            indexed_at: Utc::now().to_rfc3339(),
            symbol_count: all_symbols.len(),
            module_count: modules.len(),
            subsystem_count: subsystems.len(),
        };
        self.store.write_manifest(&manifest)?;

        Ok(IndexReport {
            files_scanned,
            symbols_indexed: all_symbols.len(),
            modules_built: modules.len(),
            subsystems_built: subsystems.len(),
            stale_rebuilt,
        })
    }

    pub fn store(&self) -> &TroveStore {
        &self.store
    }
}

fn symbol_to_memory(sym: &Symbol) -> MemoryObject {
    let mut obj = MemoryObject::new(sym.id.clone(), MemoryKind::Symbol, sym.hash.clone());
    obj.path = Some(sym.path.clone());
    obj.dependencies = sym.dependencies.clone();
    let raw = sym
        .signature
        .clone()
        .unwrap_or_else(|| format!("{} {}", sym.kind.label(), sym.name));
    obj.text = CompressedText::from_raw(raw);
    obj.text.compress_heuristic();
    obj.risk = (sym.complexity as f32 / 50.0).min(1.0);
    obj
}
