//! The read-only trove MCP handler: every method reuses the exact
//! production code paths of `trove ask` / `trove query` / `trove status` /
//! `trove tools search`, loaded fresh per call. See crates/trove-mcp/SPEC.md.

use std::path::{Path, PathBuf};

use serde_json::{json, Value};
use trove_core::store::TroveStore;
use trove_mcp::{
    Handler, PromptResult, PromptSpec, ResourceContents, ResourceSpec, ToolOutput, ToolSpec,
};
use trove_retrieve::budget::ContextBudget;
use trove_retrieve::pipeline::RetrievalPipeline;

use crate::ask::{AskOptions, Knowledge};
use crate::llm::Provider;

const SECTIONS: [&str; 3] = ["conventions", "memory", "context"];

pub struct TroveMcp {
    repo: PathBuf,
    home: PathBuf,
}

impl TroveMcp {
    pub fn new(repo: PathBuf, home: PathBuf) -> TroveMcp {
        TroveMcp { repo, home }
    }

    fn ask_text(&self, question: &str, limit: usize, budget: usize) -> Result<String, String> {
        let k = Knowledge::load(&self.repo, true);
        let opts = AskOptions {
            budget,
            limit,
            provider: Provider::None, // the MCP server is permanently offline
            model: None,
            no_redact_nonsecret: false,
        };
        let r = crate::ask::ask(&k, question, &opts);
        let mut out = String::new();
        out.push_str("mode: offline retrieval\n");
        out.push_str(&format!(
            "{} of {} matches | ~{}/{} tokens\n",
            r.sources.len(),
            r.matches,
            r.tokens,
            r.budget
        ));
        for s in &r.sources {
            out.push_str(&format!("- {} ({}): {}\n", s.cite, s.kind.label(), s.title));
            // The same snippet the offline `trove ask` footer shows.
            if !s.snippet.is_empty() && s.snippet != s.title {
                out.push_str(&format!("   {}\n", s.snippet));
            }
        }
        Ok(out)
    }

    fn query_text(&self, question: &str, budget: usize) -> Result<String, String> {
        let store = TroveStore::open(&self.repo).map_err(|e| e.to_string())?;
        let pipeline = RetrievalPipeline::new(store);
        let ctx = pipeline
            .query(question, ContextBudget::for_total(budget))
            .map_err(|e| e.to_string())?;
        let mut out = format!("Query: {question}\n");
        out.push_str(&format!(
            "{} items | ~{} tokens\n",
            ctx.items.len(),
            ctx.estimated_tokens
        ));
        for i in &ctx.items {
            // Mirror the `trove query` per-item rendering; content length is
            // already governed by the pipeline budget.
            out.push_str(&format!(
                "## {} [{:?}] {}\n{}\n",
                i.level.label(),
                i.tier,
                i.object_id,
                i.content
            ));
        }
        Ok(out)
    }

    fn status_text(&self) -> Result<String, String> {
        let store = TroveStore::open(&self.repo).map_err(|e| e.to_string())?;
        let m = store.read_manifest().map_err(|e| e.to_string())?;
        let Some(m) = m else {
            return Ok("No index found. Run: trove index".into());
        };
        Ok(format!(
            "Trove v{}\nRepo: {}\nIndexed: {}\nCounts: {} symbols, {} modules, {} subsystems\n",
            m.version, m.repo_root, m.indexed_at, m.symbol_count, m.module_count, m.subsystem_count
        ))
    }

    fn tools_text(&self, query: &str) -> Result<String, String> {
        // Read-only: load the cached catalog if present; never refresh or
        // save (Catalog::open_at writes a cache file, which the server must
        // not do).
        let path = trove_tools::catalog::Catalog::default_path();
        let catalog =
            trove_tools::catalog::Catalog::load(&path).unwrap_or(trove_tools::catalog::Catalog {
                version: 1,
                refreshed_at: 0,
                dirs: vec![],
                entries: vec![],
            });
        let tools = catalog.tools();
        let hits = trove_tools::search::search(&tools, query);
        if hits.is_empty() {
            return Ok("no tools found\n".into());
        }
        let mut out = String::new();
        for h in hits {
            out.push_str(&format!(
                "- {} ({}) {}\n",
                h.tool.name,
                h.tool.source.label(),
                h.tool.summary.as_deref().unwrap_or("")
            ));
        }
        Ok(out)
    }

    fn index_bodies(&self) -> String {
        let mut out = String::new();
        for root in [
            self.repo.join(".claude/trove"),
            self.home.join(".claude/trove"),
        ] {
            if let Ok(body) = std::fs::read_to_string(root.join("INDEX.md")) {
                out.push_str(&body);
                if !body.ends_with('\n') {
                    out.push('\n');
                }
            }
        }
        out
    }

    fn section_body(&self, section: &str) -> String {
        let skills = self.repo.join(".claude/skills");
        if let Ok(entries) = std::fs::read_dir(&skills) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(prefix) = name.strip_suffix(&format!("-{section}")) {
                    if let Ok(body) = std::fs::read_to_string(e.path().join("SKILL.md")) {
                        let _ = prefix;
                        return body;
                    }
                }
            }
        }
        std::fs::read_to_string(
            self.repo
                .join(".claude/trove")
                .join(section)
                .join("INDEX.md"),
        )
        .unwrap_or_default()
    }

    fn skill_resource(&self, section: &str) -> ResourceSpec {
        ResourceSpec {
            uri: format!("trove://skills/{section}"),
            name: format!("{section} skill"),
            description: format!("The rendered {section} skill, or the section index as fallback"),
        }
    }
}

impl Handler for TroveMcp {
    fn server_info(&self) -> (String, String) {
        ("trove".into(), env!("CARGO_PKG_VERSION").into())
    }

    fn tools(&self) -> Vec<ToolSpec> {
        vec![
            ToolSpec {
                name: "trove_ask".into(),
                description: "Ask trove memory a question; returns cited sources within a token budget. Offline retrieval, no LLM.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "question": {"type": "string"},
                        "limit": {"type": "integer", "minimum": 1, "maximum": 50},
                        "budget": {"type": "integer", "minimum": 100, "maximum": 100000}
                    },
                    "required": ["question"]
                }),
            },
            ToolSpec {
                name: "trove_query".into(),
                description: "Assemble progressive context for a query within a token budget (level-major items).".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "question": {"type": "string"},
                        "budget": {"type": "integer", "minimum": 100, "maximum": 1000000}
                    },
                    "required": ["question"]
                }),
            },
            ToolSpec {
                name: "trove_status".into(),
                description: "Index freshness and counts for the resolved repository.".into(),
                input_schema: json!({"type": "object", "properties": {}, "required": []}),
            },
            ToolSpec {
                name: "trove_tools_search".into(),
                description: "Search the installed trove tool catalog.".into(),
                input_schema: json!({
                    "type": "object",
                    "properties": {"query": {"type": "string"}},
                    "required": ["query"]
                }),
            },
        ]
    }

    fn call_tool(&self, name: &str, args: &Value) -> Result<ToolOutput, String> {
        match name {
            "trove_ask" => {
                let Some(q) = args.get("question").and_then(Value::as_str) else {
                    return Err("trove_ask requires a string question".into());
                };
                let limit = args
                    .get("limit")
                    .and_then(Value::as_u64)
                    .map(|v| v.clamp(1, 50) as usize)
                    .unwrap_or(12);
                let budget = args
                    .get("budget")
                    .and_then(Value::as_u64)
                    .map(|v| v.clamp(100, 100_000) as usize)
                    .unwrap_or(crate::ask::DEFAULT_BUDGET);
                Ok(ToolOutput {
                    text: self.ask_text(q, limit, budget)?,
                })
            }
            "trove_query" => {
                let Some(q) = args.get("question").and_then(Value::as_str) else {
                    return Err("trove_query requires a string question".into());
                };
                let budget = args
                    .get("budget")
                    .and_then(Value::as_u64)
                    .map(|v| v.clamp(100, 1_000_000) as usize)
                    .unwrap_or(200_000);
                Ok(ToolOutput {
                    text: self.query_text(q, budget)?,
                })
            }
            "trove_status" => Ok(ToolOutput {
                text: self.status_text()?,
            }),
            "trove_tools_search" => {
                let Some(q) = args.get("query").and_then(Value::as_str) else {
                    return Err("trove_tools_search requires a string query".into());
                };
                Ok(ToolOutput {
                    text: self.tools_text(q)?,
                })
            }
            _ => Err(format!("unknown tool: {name}")),
        }
    }

    fn resources(&self) -> Vec<ResourceSpec> {
        let mut out = vec![ResourceSpec {
            uri: "trove://index".into(),
            name: "trove index".into(),
            description: "The index bodies the session loader would show (project and personal)"
                .into(),
        }];
        for s in SECTIONS {
            out.push(self.skill_resource(s));
        }
        out
    }

    fn read_resource(&self, uri: &str) -> Result<ResourceContents, String> {
        let text = match uri {
            "trove://index" => self.index_bodies(),
            "trove://skills/conventions" => self.section_body("conventions"),
            "trove://skills/memory" => self.section_body("memory"),
            "trove://skills/context" => self.section_body("context"),
            _ => return Err(format!("unknown resource: {uri}")),
        };
        Ok(ResourceContents {
            uri: uri.into(),
            text,
        })
    }

    fn prompts(&self) -> Vec<PromptSpec> {
        vec![
            PromptSpec {
                name: "remember".into(),
                description: "How to capture one durable fact into the trove".into(),
            },
            PromptSpec {
                name: "reflect".into(),
                description: "How to reflect a session into at most three candidate entries".into(),
            },
        ]
    }

    fn get_prompt(&self, name: &str) -> Result<PromptResult, String> {
        match name {
            "remember" => Ok(PromptResult {
                text: "Capture one durable fact per entry: pick the section (conventions, memory, context), \
write a kebab-case slug file under entries/ with front matter (title, slug, type, created, tags) \
and a one-line hook, then add the newest-first index line under `<!-- entries below -->`. \
Run the secret guard first: never store keys, tokens or passphrases; \
`trove remember <title> --type <t>` does the routing, guard and index line for you."
                    .into(),
            }),
            "reflect" => Ok(PromptResult {
                text: "At session end, summarize what changed and propose at most three trove entries \
(decision, gotcha or convention) a future session needs. Quote no secrets and no raw logs; \
prefer one-line hooks a reader can cite. Store them with the remember flow, then render."
                    .into(),
            }),
            _ => Err(format!("unknown prompt: {name}")),
        }
    }
}

/// Serve read-only trove memory over stdio MCP. Only protocol frames are
/// written to stdout; this process never invokes an LLM provider.
pub fn serve(repo: &Path, home: &Path) -> std::io::Result<()> {
    let handler = TroveMcp::new(repo.to_path_buf(), home.to_path_buf());
    let stdin = std::io::stdin();
    let stdout = std::io::stdout();
    let mut lock_out = stdout.lock();
    let res = trove_mcp::run(&mut stdin.lock(), &mut lock_out, &handler);
    use std::io::Write as _;
    let _ = lock_out.flush();
    res
}
