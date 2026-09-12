use std::collections::HashSet;

use trove_core::compression::CompressionTier;
use trove_core::hierarchy::MemoryLevel;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::TroveStore;
use trove_index::graph::DependencyGraph;
use trove_index::symbol::Symbol;

use crate::budget::ContextBudget;

#[derive(Debug, Clone, serde::Serialize)]
pub struct RetrievedItem {
    pub level: MemoryLevel,
    pub object_id: String,
    pub tier: CompressionTier,
    pub content: String,
    pub confidence_reason: String,
    pub retrieval_path: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct AssembledContext {
    pub query: String,
    pub items: Vec<RetrievedItem>,
    pub estimated_tokens: usize,
    pub budget: ContextBudget,
    pub stopped_early: bool,
}

pub struct RetrievalPipeline {
    store: TroveStore,
}

impl RetrievalPipeline {
    pub fn new(store: TroveStore) -> Self {
        Self { store }
    }

    pub fn query(&self, query: &str, budget: ContextBudget) -> anyhow::Result<AssembledContext> {
        let terms = tokenize(query);
        let graph: DependencyGraph = self
            .store
            .read_json("graph.json")?
            .unwrap_or_default();
        let symbols: Vec<Symbol> = self.store.read_json("symbols.json")?.unwrap_or_default();

        let mut items = Vec::new();
        let mut used_tokens = 0usize;
        let mut stopped_early = false;

        // L4 Architecture
        if used_tokens < budget.architecture {
            if let Some(item) = self.best_match(MemoryKind::Architecture, &terms, budget.architecture)? {
                used_tokens += ContextBudget::estimate_tokens(&item.content);
                items.push(item);
            }
        }

        // L3 Subsystems
        let subsystem_budget = budget.active_files / 2;
        let subsystems = self.rank_objects(MemoryKind::Subsystem, &terms)?;
        for obj in subsystems.iter().take(3) {
            if used_tokens >= budget.total_tokens {
                stopped_early = true;
                break;
            }
            if let Some(item) = object_to_item(obj, MemoryLevel::L3Subsystem, CompressionTier::Summary) {
                let cost = ContextBudget::estimate_tokens(&item.content);
                if used_tokens + cost > budget.architecture + subsystem_budget {
                    stopped_early = true;
                    break;
                }
                used_tokens += cost;
                items.push(item);
            }
        }

        // L2 Modules
        let modules = self.rank_objects(MemoryKind::Module, &terms)?;
        for obj in modules.iter().take(8) {
            if used_tokens >= budget.total_tokens {
                stopped_early = true;
                break;
            }
            if let Some(item) = object_to_item(obj, MemoryLevel::L2Module, CompressionTier::Summary) {
                let cost = ContextBudget::estimate_tokens(&item.content);
                if used_tokens + cost > budget.architecture + budget.active_files + subsystem_budget {
                    stopped_early = true;
                    break;
                }
                used_tokens += cost;
                items.push(item);
            }
        }

        // L1 Symbols
        let ranked_symbols: Vec<_> = symbols
            .iter()
            .filter(|s| score_text(&format!("{} {}", s.name, s.path), &terms) > 0)
            .collect();

        let mut symbol_budget_used = 0usize;
        for sym in ranked_symbols.iter().take(20) {
            if symbol_budget_used >= budget.code {
                stopped_early = true;
                break;
            }
            let content = sym
                .signature
                .clone()
                .unwrap_or_else(|| format!("{} {}", sym.kind.label(), sym.name));
            let cost = ContextBudget::estimate_tokens(&content);
            if symbol_budget_used + cost > budget.code {
                stopped_early = true;
                break;
            }
            symbol_budget_used += cost;
            used_tokens += cost;
            items.push(RetrievedItem {
                level: MemoryLevel::L1ActiveSymbols,
                object_id: sym.id.clone(),
                tier: CompressionTier::Summary,
                content,
                confidence_reason: "symbol name/path match".into(),
                retrieval_path: vec![
                    "query".into(),
                    sym.path.clone(),
                    sym.name.clone(),
                ],
            });
        }

        // Dependency expansion from top module hits
        let seeds: Vec<String> = items
            .iter()
            .filter(|i| i.level == MemoryLevel::L2Module)
            .map(|i| i.object_id.clone())
            .collect();
        if !seeds.is_empty() {
            let expanded = graph.expand(&seeds, 1);
            let mut dep_budget = 0usize;
            for node_id in expanded.iter().take(10) {
                if dep_budget >= budget.dependency_expansion {
                    stopped_early = true;
                    break;
                }
                if let Some(sym) = symbols.iter().find(|s| s.id == *node_id || s.path == *node_id) {
                    let content = format!("neighbor: {} at {}", sym.name, sym.path);
                    let cost = ContextBudget::estimate_tokens(&content);
                    dep_budget += cost;
                    used_tokens += cost;
                    items.push(RetrievedItem {
                        level: MemoryLevel::L1ActiveSymbols,
                        object_id: sym.id.clone(),
                        tier: CompressionTier::UltraSummary,
                        content,
                        confidence_reason: "dependency graph neighbor".into(),
                        retrieval_path: vec!["graph".into(), node_id.clone()],
                    });
                }
            }
        }

        // L5 Historical: bridge to .claude/trove if present
        let historical = self.rank_objects(MemoryKind::Historical, &terms)?;
        for obj in historical.iter().take(3) {
            if used_tokens >= budget.total_tokens {
                stopped_early = true;
                break;
            }
            if let Some(item) = object_to_item(obj, MemoryLevel::L5Historical, CompressionTier::Summary) {
                used_tokens += ContextBudget::estimate_tokens(&item.content);
                items.push(item);
            }
        }

        Ok(AssembledContext {
            query: query.to_string(),
            items,
            estimated_tokens: used_tokens,
            budget,
            stopped_early,
        })
    }

    fn best_match(
        &self,
        kind: MemoryKind,
        terms: &[String],
        token_budget: usize,
    ) -> anyhow::Result<Option<RetrievedItem>> {
        let objects = self.store.read_objects(kind)?;
        let best = objects
            .iter()
            .max_by_key(|o| score_object(o, terms));
        Ok(best.and_then(|obj| {
            let item = object_to_item(obj, obj.kind.level(), CompressionTier::Summary)?;
            if ContextBudget::estimate_tokens(&item.content) <= token_budget {
                Some(item)
            } else {
                object_to_item(obj, obj.kind.level(), CompressionTier::UltraSummary)
            }
        }))
    }

    fn rank_objects(&self, kind: MemoryKind, terms: &[String]) -> anyhow::Result<Vec<MemoryObject>> {
        let mut objects = self.store.read_objects(kind)?;
        objects.sort_by_key(|o| std::cmp::Reverse(score_object(o, terms)));
        Ok(objects)
    }
}

fn object_to_item(
    obj: &MemoryObject,
    level: MemoryLevel,
    tier: CompressionTier,
) -> Option<RetrievedItem> {
    let content = obj
        .text
        .at_tier(tier)
        .map(str::to_string)
        .or_else(|| {
            if tier == CompressionTier::Keywords && !obj.text.keywords.is_empty() {
                Some(obj.text.keywords_line())
            } else {
                None
            }
        })?;

    Some(RetrievedItem {
        level,
        object_id: obj.id.clone(),
        tier,
        content,
        confidence_reason: obj
            .confidence
            .as_ref()
            .map(|c| c.reason.clone())
            .unwrap_or_else(|| "indexed".into()),
        retrieval_path: obj
            .confidence
            .as_ref()
            .map(|c| c.retrieval_path.clone())
            .unwrap_or_default(),
    })
}

fn tokenize(query: &str) -> Vec<String> {
    query
        .split(|c: char| !c.is_alphanumeric())
        .filter(|t| t.len() > 2)
        .map(|t| t.to_lowercase())
        .collect()
}

fn score_object(obj: &MemoryObject, terms: &[String]) -> i32 {
    let mut score = 0i32;
    let haystacks = [
        obj.id.as_str(),
        obj.path.as_deref().unwrap_or(""),
        obj.text.summary.as_deref().unwrap_or(""),
        obj.text.raw.as_deref().unwrap_or(""),
    ];
    for term in terms {
        for h in &haystacks {
            if h.to_lowercase().contains(term) {
                score += 3;
            }
        }
        for kw in &obj.text.keywords {
            if kw.contains(term) {
                score += 2;
            }
        }
    }
    score
}

fn score_text(text: &str, terms: &[String]) -> i32 {
    let lower = text.to_lowercase();
    terms.iter().filter(|t| lower.contains(t.as_str())).count() as i32
}

pub fn import_historical(store: &TroveStore, trove_dir: &std::path::Path) -> anyhow::Result<usize> {
    let index_path = trove_dir.join("INDEX.md");
    if !index_path.exists() {
        return Ok(0);
    }
    let index = std::fs::read_to_string(index_path)?;
    let entries_dir = trove_dir.join("entries");
    let mut imported = 0usize;
    let mut seen = HashSet::new();

    if entries_dir.exists() {
        for entry in std::fs::read_dir(entries_dir)? {
            let entry = entry?;
            let path = entry.path();
            let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
            if ext != "md" && ext != "json" {
                continue;
            }
            let content = std::fs::read_to_string(&path)?;
            let slug = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("entry")
                .to_string();
            if !seen.insert(slug.clone()) {
                continue;
            }
            let hash = trove_core::store::content_hash(content.as_bytes());
            let mut obj = MemoryObject::new(format!("historical:{slug}"), MemoryKind::Historical, hash);
            obj.path = Some(path.to_string_lossy().into());
            obj.text = trove_core::compression::CompressedText::from_raw(content);
            obj.text.compress_heuristic();
            obj.confidence = Some(trove_core::memory::Confidence::high(
                "imported from Claude trove entries",
                vec!["import".into(), slug],
            ));
            store.upsert_objects(std::slice::from_ref(&obj))?;
            imported += 1;
        }
    }

    // Also index the INDEX.md itself as L6 repository memory
    let hash = trove_core::store::content_hash(index.as_bytes());
    let mut repo_obj = MemoryObject::new("repository:index", MemoryKind::Repository, hash);
    repo_obj.text = trove_core::compression::CompressedText::from_raw(index);
    repo_obj.text.compress_heuristic();
    store.upsert_objects(std::slice::from_ref(&repo_obj))?;

    Ok(imported)
}
