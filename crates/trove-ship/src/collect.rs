//! Gather raw facts about a project: the engine index in `.trove/`, the project
//! trove in `.claude/trove/`, and well-known files (README, build manifests,
//! agent rules, ADRs). Nothing here is redacted yet; that happens on the
//! composed documents.

use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use anyhow::Result;
use ignore::WalkBuilder;
use regex::Regex;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::patch::PatchRecord;
use trove_core::store::{TroveManifest, TROVE_DIR};
use trove_index::graph::DependencyGraph;
use trove_index::symbol::{Symbol, SymbolKind};

use crate::redact::TroveIgnore;

const MAX_FILE_BYTES: u64 = 512 * 1024;
pub const MAX_INCLUDED_CHARS: usize = 4000;
const MAX_MARKERS: usize = 60;

#[derive(Debug, Clone, Default)]
pub struct Facts {
    pub name: String,
    pub readme: Option<Readme>,
    pub languages: Vec<(String, usize)>,
    pub top_level: Vec<TopEntry>,
    pub index: Option<IndexFacts>,
    pub build: Vec<BuildSystem>,
    pub env_vars: Vec<(String, Vec<String>)>,
    pub ci_commands: Vec<String>,
    pub rule_files: Vec<TextFile>,
    pub tooling: Vec<(String, String)>,
    pub adrs: Vec<TextFile>,
    pub trove: Vec<TroveEntry>,
    pub patches: Vec<PatchRecord>,
    pub markers: Vec<Marker>,
    pub test_layout: Vec<String>,
    pub files_scanned: usize,
    pub files_excluded: usize,
}

#[derive(Debug, Clone, Default)]
pub struct Readme {
    pub path: String,
    pub intro: String,
    pub sections: Vec<Section>,
}

#[derive(Debug, Clone)]
pub struct Section {
    pub level: usize,
    pub heading: String,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct TopEntry {
    pub name: String,
    pub is_dir: bool,
    pub files: usize,
}

#[derive(Debug, Clone, Default)]
pub struct IndexFacts {
    pub manifest: TroveManifest,
    pub subsystems: Vec<SubsystemFact>,
    pub modules: Vec<ModuleFact>,
    pub entry_points: Vec<String>,
    pub top_imports: Vec<(String, usize)>,
    pub graph_nodes: usize,
    pub graph_edges: usize,
}

#[derive(Debug, Clone)]
pub struct SubsystemFact {
    pub name: String,
    pub files: usize,
    pub symbols: usize,
    pub kinds: Vec<(String, usize)>,
}

#[derive(Debug, Clone)]
pub struct ModuleFact {
    pub path: String,
    pub exports: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct BuildSystem {
    pub name: String,
    pub file: String,
    pub prerequisite: Option<String>,
    pub commands: Vec<(String, String)>,
}

#[derive(Debug, Clone)]
pub struct TextFile {
    pub path: String,
    pub title: String,
    pub content: String,
}

#[derive(Debug, Clone, Default)]
pub struct TroveEntry {
    pub file: String,
    pub title: String,
    pub kind: String,
    pub created: String,
    pub tags: Vec<String>,
    pub body: String,
}

#[derive(Debug, Clone)]
pub struct Marker {
    pub path: String,
    pub line: usize,
    pub tag: String,
    pub text: String,
}

#[derive(Debug, Clone, Default)]
pub struct CollectOptions {
    /// Directories never walked (the output directory, for example).
    pub exclude_dirs: Vec<PathBuf>,
}

pub fn collect(repo: &Path, ignore: &TroveIgnore, opts: &CollectOptions) -> Result<Facts> {
    let repo = repo.canonicalize()?;
    let mut facts = Facts {
        name: repo
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "project".into()),
        ..Default::default()
    };

    let mut languages: BTreeMap<String, usize> = BTreeMap::new();
    let mut top: BTreeMap<String, (bool, usize)> = BTreeMap::new();
    let mut tests: BTreeSet<String> = BTreeSet::new();
    let mut cargo_files: Vec<(String, String)> = Vec::new();
    let exclude: Vec<PathBuf> = opts
        .exclude_dirs
        .iter()
        .filter_map(|d| d.canonicalize().ok())
        .collect();

    let walker = WalkBuilder::new(&repo)
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .require_git(false)
        .sort_by_file_name(|a, b| a.cmp(b))
        .build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let rel = path
            .strip_prefix(&repo)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/");
        if rel.starts_with(".git/")
            || rel.starts_with(&format!("{TROVE_DIR}/"))
            || rel.starts_with("target/")
            || rel.contains("node_modules/")
            || exclude.iter().any(|d| path.starts_with(d))
        {
            continue;
        }
        if ignore.excludes_path(&rel, false) {
            facts.files_excluded += 1;
            continue;
        }
        if entry
            .metadata()
            .map(|m| m.len() > MAX_FILE_BYTES)
            .unwrap_or(true)
        {
            continue;
        }
        facts.files_scanned += 1;

        let first = rel.split('/').next().unwrap_or(&rel).to_string();
        let is_dir = rel.contains('/');
        let e = top.entry(first).or_insert((is_dir, 0));
        e.1 += 1;

        let file_name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        if let Some(lang) = language_of(&file_name) {
            *languages.entry(lang.to_string()).or_insert(0) += 1;
        }
        if let Some(t) = test_location(&rel, &file_name) {
            tests.insert(t);
        }

        // Test fixtures and vendored code describe someone else's project.
        if is_fixture_or_vendor(&rel) {
            continue;
        }

        // Only read the files we actually use. `.env` itself is never read.
        let wanted = is_code(&file_name)
            || is_rule_file(&rel)
            || is_adr(&rel)
            || is_env_example(&file_name)
            || rel.starts_with(".github/workflows/")
            || matches!(
                file_name.as_str(),
                "README.md"
                    | "Cargo.toml"
                    | "package.json"
                    | "Makefile"
                    | "pyproject.toml"
                    | "requirements.txt"
                    | "go.mod"
                    | "Dockerfile"
                    | "docker-compose.yml"
                    | "compose.yaml"
                    | "justfile"
            );
        if !wanted && tooling_label(&file_name).is_none() {
            continue;
        }
        if let Some(label) = tooling_label(&file_name) {
            facts.tooling.push((rel.clone(), label.to_string()));
        }
        let Ok(content) = fs::read_to_string(path) else {
            continue;
        };

        if is_code(&file_name) {
            collect_markers(&rel, &content, &mut facts.markers);
            if file_name.ends_with(".rs") && content.contains("#[cfg(test)]") {
                tests.insert("Rust unit tests inline in `#[cfg(test)]` modules".into());
            }
        }
        if rel == "README.md" || (facts.readme.is_none() && rel.eq_ignore_ascii_case("readme.md")) {
            facts.readme = Some(parse_readme(&rel, &content));
        }
        if is_rule_file(&rel) {
            facts.rule_files.push(TextFile {
                path: rel.clone(),
                title: rel.clone(),
                content: strip_frontmatter(&content).trim().to_string(),
            });
        }
        if is_adr(&rel) {
            facts.adrs.push(TextFile {
                path: rel.clone(),
                title: first_heading(&content).unwrap_or_else(|| file_name.clone()),
                content: content.trim().to_string(),
            });
        }
        if is_env_example(&file_name) {
            let vars = env_var_names(&content);
            if !vars.is_empty() {
                facts.env_vars.push((rel.clone(), vars));
            }
        }
        if rel.starts_with(".github/workflows/") {
            ci_commands(&content, &mut facts.ci_commands);
        }
        if file_name == "Cargo.toml" {
            cargo_files.push((rel.clone(), content.clone()));
        }
        if !rel.contains('/') {
            if let Some(b) = build_system(&repo, &file_name, &content) {
                facts.build.push(b);
            }
        }
    }

    if let Some(b) = cargo_build(&cargo_files) {
        facts.build.insert(0, b);
    }
    facts.languages = sort_counts(languages);
    facts.top_level = top
        .into_iter()
        .map(|(name, (is_dir, files))| TopEntry {
            name,
            is_dir,
            files,
        })
        .collect();
    facts.test_layout = tests.into_iter().collect();
    facts.markers.truncate(MAX_MARKERS);
    facts.ci_commands.truncate(20);
    facts.trove = read_project_trove(&repo, ignore)?;
    facts.index = read_index(&repo, ignore)?;
    facts.patches = read_patches(&repo);
    Ok(facts)
}

fn sort_counts(map: BTreeMap<String, usize>) -> Vec<(String, usize)> {
    let mut v: Vec<_> = map.into_iter().collect();
    v.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

fn language_of(file_name: &str) -> Option<&'static str> {
    let ext = file_name.rsplit_once('.').map(|(_, e)| e)?;
    Some(match ext {
        "rs" => "Rust",
        "py" => "Python",
        "js" | "jsx" | "mjs" | "cjs" => "JavaScript",
        "ts" | "tsx" => "TypeScript",
        "go" => "Go",
        "sh" | "bash" => "Shell",
        "rb" => "Ruby",
        "java" => "Java",
        "kt" => "Kotlin",
        "swift" => "Swift",
        "c" | "h" => "C",
        "cc" | "cpp" | "hpp" => "C++",
        "cs" => "C#",
        "php" => "PHP",
        "html" => "HTML",
        "css" | "scss" => "CSS",
        "md" | "mdc" => "Markdown",
        "sql" => "SQL",
        _ => return None,
    })
}

fn is_code(file_name: &str) -> bool {
    matches!(
        language_of(file_name),
        Some(l) if !matches!(l, "Markdown" | "HTML" | "CSS")
    )
}

fn test_location(rel: &str, file_name: &str) -> Option<String> {
    for dir in ["tests", "test", "__tests__", "spec"] {
        if let Some(idx) = rel.split('/').position(|p| p == dir) {
            let parts: Vec<_> = rel.split('/').take(idx + 1).collect();
            return Some(format!("`{}/`", parts.join("/")));
        }
    }
    if file_name.ends_with("_test.go") {
        return Some("Go tests next to code (`*_test.go`)".into());
    }
    if file_name.starts_with("test_") && file_name.ends_with(".py") {
        return Some("Python tests (`test_*.py`)".into());
    }
    if [".test.ts", ".test.js", ".spec.ts", ".spec.js", ".test.tsx"]
        .iter()
        .any(|s| file_name.ends_with(s))
    {
        return Some("JS/TS tests next to code (`*.test.*`, `*.spec.*`)".into());
    }
    None
}

fn is_fixture_or_vendor(rel: &str) -> bool {
    rel.split('/').rev().skip(1).any(|d| {
        matches!(
            d,
            "fixtures" | "__fixtures__" | "testdata" | "vendor" | "third_party" | "third-party"
        )
    })
}

fn is_rule_file(rel: &str) -> bool {
    matches!(
        rel,
        "CLAUDE.md"
            | "AGENTS.md"
            | "CONTRIBUTING.md"
            | ".cursorrules"
            | ".windsurfrules"
            | ".github/copilot-instructions.md"
            | ".github/CONTRIBUTING.md"
    ) || (rel.starts_with(".cursor/rules/") && (rel.ends_with(".mdc") || rel.ends_with(".md")))
}

fn is_adr(rel: &str) -> bool {
    rel.ends_with(".md")
        && rel
            .split('/')
            .rev()
            .skip(1)
            .any(|d| matches!(d, "adr" | "adrs" | "decisions" | "decision-records"))
}

fn is_env_example(file_name: &str) -> bool {
    matches!(
        file_name,
        ".env.example" | ".env.sample" | ".env.template" | "env.example" | ".env.dist"
    )
}

fn tooling_label(file_name: &str) -> Option<&'static str> {
    Some(match file_name {
        "rustfmt.toml" | ".rustfmt.toml" => "rustfmt formatting rules",
        "clippy.toml" => "clippy lint settings",
        ".editorconfig" => "editor whitespace and indentation rules",
        ".prettierrc" | ".prettierrc.json" | ".prettierrc.js" | "prettier.config.js" => {
            "Prettier formatting"
        }
        ".eslintrc" | ".eslintrc.json" | ".eslintrc.js" | ".eslintrc.cjs" | "eslint.config.js"
        | "eslint.config.mjs" => "ESLint rules",
        "biome.json" => "Biome lint and format",
        "ruff.toml" | ".ruff.toml" => "Ruff lint rules",
        ".golangci.yml" | ".golangci.yaml" => "golangci-lint rules",
        ".pre-commit-config.yaml" => "pre-commit hooks",
        "tsconfig.json" => "TypeScript compiler settings",
        "rust-toolchain.toml" | "rust-toolchain" => "pinned Rust toolchain",
        ".nvmrc" | ".node-version" => "pinned Node version",
        ".python-version" => "pinned Python version",
        _ => return None,
    })
}

fn marker_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(TODO|FIXME|HACK|XXX)\b[:(]?\s*(.*)").expect("marker regex"))
}

fn collect_markers(rel: &str, content: &str, out: &mut Vec<Marker>) {
    for (idx, line) in content.lines().enumerate() {
        let Some(comment) = comment_start(line) else {
            continue;
        };
        if let Some(c) = marker_regex().captures(&line[comment..]) {
            let text = c[2].trim().trim_end_matches("*/").trim();
            if text.is_empty() {
                continue;
            }
            let text: String = text.chars().take(200).collect();
            out.push(Marker {
                path: rel.to_string(),
                line: idx + 1,
                tag: c[1].to_string(),
                text,
            });
        }
    }
}

fn unescaped_quotes(s: &str) -> usize {
    let mut n = 0;
    let mut escaped = false;
    for c in s.chars() {
        if escaped {
            escaped = false;
            continue;
        }
        match c {
            '\\' => escaped = true,
            '"' => n += 1,
            _ => {}
        }
    }
    n
}

/// Byte offset where a line comment starts, ignoring comment tokens inside
/// string literals (an odd number of double quotes before the token).
fn comment_start(line: &str) -> Option<usize> {
    let trimmed = line.trim_start();
    let indent = line.len() - trimmed.len();
    if trimmed.starts_with("* ") || trimmed.starts_with("--") || trimmed.starts_with(';') {
        return Some(indent);
    }
    for token in ["//", "/*", "# "] {
        let mut from = 0;
        while let Some(pos) = line[from..].find(token) {
            let at = from + pos;
            if unescaped_quotes(&line[..at]).is_multiple_of(2) {
                return Some(at);
            }
            from = at + token.len();
        }
    }
    trimmed
        .starts_with('#')
        .then_some(indent)
        .filter(|_| !trimmed.starts_with("#["))
}

pub fn parse_readme(path: &str, content: &str) -> Readme {
    let sections = split_sections(content);
    let mut intro = String::new();
    for line in content.lines() {
        let t = line.trim();
        if t.starts_with("## ") || (t.starts_with('#') && !intro.is_empty() && t.starts_with("# "))
        {
            if !intro.trim().is_empty() {
                break;
            }
            continue;
        }
        if t.starts_with("# ")
            || t.starts_with("<p")
            || t.starts_with("</p")
            || t.starts_with("![")
            || t.starts_with("[![")
            || t.starts_with("<img")
            || t == "---"
        {
            continue;
        }
        intro.push_str(line);
        intro.push('\n');
    }
    Readme {
        path: path.to_string(),
        intro: truncate_paragraphs(intro.trim(), 1500),
        sections,
    }
}

/// Flat list of headed sections, ignoring headings inside code fences.
pub fn split_sections(content: &str) -> Vec<Section> {
    let mut out: Vec<Section> = Vec::new();
    let mut in_fence = false;
    for line in content.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        if !in_fence {
            if let Some((level, heading)) = heading_of(line) {
                out.push(Section {
                    level,
                    heading,
                    body: String::new(),
                });
                continue;
            }
        }
        if let Some(s) = out.last_mut() {
            s.body.push_str(line);
            s.body.push('\n');
        }
    }
    out
}

fn heading_of(line: &str) -> Option<(usize, String)> {
    let hashes = line.chars().take_while(|&c| c == '#').count();
    if (1..=6).contains(&hashes) && line[hashes..].starts_with(' ') {
        Some((hashes, line[hashes..].trim().to_string()))
    } else {
        None
    }
}

fn first_heading(content: &str) -> Option<String> {
    content.lines().find_map(|l| heading_of(l).map(|(_, h)| h))
}

pub fn strip_frontmatter(content: &str) -> &str {
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            let after = &rest[end + 4..];
            return after.strip_prefix('\n').unwrap_or(after);
        }
    }
    content
}

fn frontmatter(content: &str) -> BTreeMap<String, String> {
    let mut out = BTreeMap::new();
    if let Some(rest) = content.strip_prefix("---\n") {
        if let Some(end) = rest.find("\n---") {
            for line in rest[..end].lines() {
                if let Some((k, v)) = line.split_once(':') {
                    out.insert(k.trim().to_string(), v.trim().trim_matches('"').to_string());
                }
            }
        }
    }
    out
}

pub fn truncate_paragraphs(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_string();
    }
    let mut out = String::new();
    for para in text.split("\n\n") {
        if out.len() + para.len() + 2 > max {
            break;
        }
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        out.push_str(para);
    }
    if out.is_empty() {
        out = text.chars().take(max).collect();
    }
    out
}

fn env_var_names(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in content.lines() {
        let line = line.trim().trim_start_matches("export ");
        if line.starts_with('#') {
            continue;
        }
        if let Some((k, _)) = line.split_once('=') {
            let k = k.trim();
            if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
                out.push(k.to_string());
            }
        }
    }
    out
}

fn ci_commands(content: &str, out: &mut Vec<String>) {
    for line in content.lines() {
        let t = line.trim().trim_start_matches("- ");
        if let Some(cmd) = t.strip_prefix("run:") {
            let cmd = cmd.trim();
            if !cmd.is_empty() && cmd != "|" && cmd != ">" && !out.iter().any(|c| c == cmd) {
                out.push(cmd.to_string());
            }
        }
    }
}

fn cargo_build(files: &[(String, String)]) -> Option<BuildSystem> {
    let root = files.iter().find(|(p, _)| p == "Cargo.toml")?;
    let bin_re = Regex::new(r#"(?s)\[\[bin\]\][^\[]*?name\s*=\s*"([^"]+)""#).ok()?;
    let mut commands = vec![
        ("cargo build".to_string(), "build everything".to_string()),
        ("cargo test".to_string(), "run the test suite".to_string()),
    ];
    let mut bins: Vec<String> = files
        .iter()
        .flat_map(|(_, c)| {
            bin_re
                .captures_iter(c)
                .map(|m| m[1].to_string())
                .collect::<Vec<_>>()
        })
        .collect();
    bins.sort();
    bins.dedup();
    for b in bins {
        commands.push((
            format!("cargo run --bin {b} -- --help"),
            format!("see the `{b}` CLI"),
        ));
    }
    if files.len() > 1 || root.1.contains("[workspace]") {
        commands.push(("cargo fmt --all".into(), "format".into()));
        commands.push(("cargo clippy --all-targets".into(), "lint".into()));
    }
    Some(BuildSystem {
        name: "Rust (Cargo)".into(),
        file: "Cargo.toml".into(),
        prerequisite: Some("Rust toolchain via rustup".into()),
        commands,
    })
}

fn build_system(repo: &Path, file_name: &str, content: &str) -> Option<BuildSystem> {
    match file_name {
        "package.json" => {
            let v: serde_json::Value = serde_json::from_str(content).ok()?;
            let pm = if repo.join("pnpm-lock.yaml").exists() {
                "pnpm"
            } else if repo.join("yarn.lock").exists() {
                "yarn"
            } else if repo.join("bun.lockb").exists() || repo.join("bun.lock").exists() {
                "bun"
            } else {
                "npm"
            };
            let mut commands = vec![(format!("{pm} install"), "install dependencies".to_string())];
            if let Some(scripts) = v.get("scripts").and_then(|s| s.as_object()) {
                for (name, cmd) in scripts {
                    commands.push((
                        format!("{pm} run {name}"),
                        format!("runs `{}`", cmd.as_str().unwrap_or("")),
                    ));
                }
            }
            let node = v
                .pointer("/engines/node")
                .and_then(|n| n.as_str())
                .map(|n| format!("Node {n}"))
                .unwrap_or_else(|| "Node.js".into());
            Some(BuildSystem {
                name: format!("JavaScript ({pm})"),
                file: file_name.into(),
                prerequisite: Some(node),
                commands,
            })
        }
        "Makefile" => {
            let re = Regex::new(r"(?m)^([A-Za-z0-9][A-Za-z0-9_.-]*):(?:[^=\n]*?)(?:##\s*(.*))?$")
                .ok()?;
            let commands = re
                .captures_iter(content)
                .filter(|c| &c[1] != ".PHONY")
                .map(|c| {
                    (
                        format!("make {}", &c[1]),
                        c.get(2)
                            .map(|m| m.as_str().trim().to_string())
                            .unwrap_or_default(),
                    )
                })
                .take(20)
                .collect();
            Some(BuildSystem {
                name: "Make".into(),
                file: file_name.into(),
                prerequisite: Some("GNU make".into()),
                commands,
            })
        }
        "justfile" => {
            let re = Regex::new(r"(?m)^([A-Za-z0-9][A-Za-z0-9_-]*)(?:\s+[^:\n]*)?:\s*$").ok()?;
            let commands = re
                .captures_iter(content)
                .map(|c| (format!("just {}", &c[1]), String::new()))
                .take(20)
                .collect();
            Some(BuildSystem {
                name: "just".into(),
                file: file_name.into(),
                prerequisite: Some("just".into()),
                commands,
            })
        }
        "pyproject.toml" => {
            let (pm, install) = if content.contains("[tool.poetry]") {
                ("Poetry", "poetry install")
            } else if repo.join("uv.lock").exists() {
                ("uv", "uv sync")
            } else {
                ("pip", "pip install -e .")
            };
            let mut commands = vec![(install.to_string(), "install the package".to_string())];
            if content.contains("pytest") {
                commands.push(("pytest".into(), "run the test suite".into()));
            }
            Some(BuildSystem {
                name: format!("Python ({pm})"),
                file: file_name.into(),
                prerequisite: Some("Python 3".into()),
                commands,
            })
        }
        "requirements.txt" => Some(BuildSystem {
            name: "Python (pip)".into(),
            file: file_name.into(),
            prerequisite: Some("Python 3".into()),
            commands: vec![(
                "pip install -r requirements.txt".into(),
                "install dependencies".into(),
            )],
        }),
        "go.mod" => {
            let version = content
                .lines()
                .find_map(|l| l.strip_prefix("go "))
                .map(|v| format!("Go {}", v.trim()))
                .unwrap_or_else(|| "Go".into());
            Some(BuildSystem {
                name: "Go modules".into(),
                file: file_name.into(),
                prerequisite: Some(version),
                commands: vec![
                    ("go build ./...".into(), "build".into()),
                    ("go test ./...".into(), "run the test suite".into()),
                ],
            })
        }
        "Dockerfile" => Some(BuildSystem {
            name: "Docker".into(),
            file: file_name.into(),
            prerequisite: Some("Docker".into()),
            commands: vec![("docker build -t app .".into(), "build the image".into())],
        }),
        "docker-compose.yml" | "compose.yaml" => Some(BuildSystem {
            name: "Docker Compose".into(),
            file: file_name.into(),
            prerequisite: Some("Docker with Compose".into()),
            commands: vec![("docker compose up".into(), "start the local stack".into())],
        }),
        _ => None,
    }
}

/// Read entries from the project trove at `.claude/trove/entries/`. The user
/// trove in `~/.claude/trove` is personal and never shipped.
fn read_project_trove(repo: &Path, ignore: &TroveIgnore) -> Result<Vec<TroveEntry>> {
    let dir = repo.join(".claude/trove/entries");
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("md" | "json")))
        .collect();
    paths.sort();
    let mut out = Vec::new();
    for path in paths {
        let rel = path
            .strip_prefix(repo)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if ignore.excludes_path(&rel, false) {
            continue;
        }
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };
        let slug = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_default();
        let entry = if rel.ends_with(".json") {
            parse_json_entry(&rel, &slug, &content)
        } else {
            parse_md_entry(&rel, &slug, &content)
        };
        if let Some(e) = entry {
            out.push(e);
        }
    }
    out.sort_by(|a, b| b.created.cmp(&a.created).then(a.title.cmp(&b.title)));
    Ok(out)
}

pub fn parse_md_entry(rel: &str, slug: &str, content: &str) -> Option<TroveEntry> {
    let fm = frontmatter(content);
    let tags = fm
        .get("tags")
        .map(|t| {
            t.trim_matches(['[', ']'])
                .split(',')
                .map(|s| s.trim().trim_matches('"').to_string())
                .filter(|s| !s.is_empty())
                .collect()
        })
        .unwrap_or_default();
    Some(TroveEntry {
        file: rel.to_string(),
        title: fm.get("title").cloned().unwrap_or_else(|| slug.to_string()),
        kind: fm
            .get("type")
            .cloned()
            .unwrap_or_else(|| "reference".into())
            .to_ascii_lowercase(),
        created: fm.get("created").cloned().unwrap_or_default(),
        tags,
        body: strip_frontmatter(content).trim().to_string(),
    })
}

fn parse_json_entry(rel: &str, slug: &str, content: &str) -> Option<TroveEntry> {
    let v: serde_json::Value = serde_json::from_str(content).ok()?;
    let s = |k: &str| v.get(k).and_then(|x| x.as_str()).map(str::to_string);
    let mut body = s("summary").unwrap_or_default();
    if let Some(data) = v.get("data") {
        body.push_str("\n\n```json\n");
        body.push_str(&serde_json::to_string_pretty(data).unwrap_or_default());
        body.push_str("\n```");
    }
    Some(TroveEntry {
        file: rel.to_string(),
        title: s("title").unwrap_or_else(|| slug.to_string()),
        kind: s("type")
            .unwrap_or_else(|| "reference".into())
            .to_ascii_lowercase(),
        created: s("created").unwrap_or_default(),
        tags: v
            .get("tags")
            .and_then(|t| t.as_array())
            .map(|a| {
                a.iter()
                    .filter_map(|x| x.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default(),
        body: body.trim().to_string(),
    })
}

fn read_json<T: for<'de> serde::Deserialize<'de>>(path: &Path) -> Option<T> {
    serde_json::from_str(&fs::read_to_string(path).ok()?).ok()
}

fn read_objects(dir: &Path) -> Vec<MemoryObject> {
    let Ok(rd) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<MemoryObject> = rd
        .flatten()
        .filter(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
        .filter_map(|e| read_json(&e.path()))
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    out
}

/// Read the engine index from `.trove/` without creating anything.
fn read_index(repo: &Path, ignore: &TroveIgnore) -> Result<Option<IndexFacts>> {
    let root = repo.join(TROVE_DIR);
    let Some(manifest) = read_json::<TroveManifest>(&root.join("manifest.json")) else {
        return Ok(None);
    };
    let symbols: Vec<Symbol> = read_json::<Vec<Symbol>>(&root.join("symbols.json"))
        .unwrap_or_default()
        .into_iter()
        .filter(|s| !ignore.excludes_path(&s.path, false))
        .collect();
    let graph: DependencyGraph = read_json(&root.join("graph.json")).unwrap_or_default();
    let subsystem_objs = read_objects(&root.join(subdir(MemoryKind::Subsystem)));
    let module_objs = read_objects(&root.join(subdir(MemoryKind::Module)));

    let mut subsystems: Vec<SubsystemFact> = subsystem_objs
        .iter()
        .filter_map(|o| {
            let name = o.path.clone()?;
            if ignore.excludes_path(&name, true) {
                return None;
            }
            let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
            for s in symbols
                .iter()
                .filter(|s| s.path.split('/').next() == Some(name.as_str()))
            {
                *kinds.entry(s.kind.label().to_string()).or_insert(0) += 1;
            }
            Some(SubsystemFact {
                files: o.dependencies.len(),
                symbols: o.symbols.len(),
                kinds: sort_counts(kinds).into_iter().take(4).collect(),
                name,
            })
        })
        .collect();
    subsystems.sort_by(|a, b| b.symbols.cmp(&a.symbols).then(a.name.cmp(&b.name)));

    let mut modules: Vec<ModuleFact> = module_objs
        .iter()
        .filter_map(|o| {
            let path = o.path.clone()?;
            if ignore.excludes_path(&path, false) || path.starts_with(".claude/") {
                return None;
            }
            let exports: Vec<String> = symbols
                .iter()
                .filter(|s| s.path == path && !s.name.trim().is_empty())
                .filter(|s| {
                    matches!(
                        s.kind,
                        SymbolKind::Function
                            | SymbolKind::Struct
                            | SymbolKind::Class
                            | SymbolKind::Trait
                            | SymbolKind::Enum
                            | SymbolKind::Interface
                    )
                })
                .map(|s| format!("{} ({})", s.name, s.kind.label()))
                .collect();
            Some(ModuleFact { exports, path })
        })
        .collect();
    modules.sort_by(|a, b| a.path.cmp(&b.path));

    let mut entry_points: Vec<String> = symbols
        .iter()
        .filter(|s| s.name == "main" && matches!(s.kind, SymbolKind::Function))
        .map(|s| format!("{}:{}", s.path, s.start_line))
        .collect();
    for m in &modules {
        let f = m.path.rsplit('/').next().unwrap_or(&m.path);
        if matches!(
            f,
            "main.rs" | "index.js" | "index.ts" | "main.py" | "__main__.py" | "main.go" | "cli.py"
        ) && !entry_points.iter().any(|e| e.starts_with(&m.path))
        {
            entry_points.push(m.path.clone());
        }
    }
    entry_points.sort();
    entry_points.dedup();

    let mut imports: BTreeMap<String, usize> = BTreeMap::new();
    for e in graph.edges.iter().filter(|e| e.kind == "import") {
        if ignore.excludes_path(&e.from, false) {
            continue;
        }
        // Rust import text is a path; brace fragments without `::` are noise.
        if e.from.ends_with(".rs") && !e.to.contains("::") {
            continue;
        }
        let target = e.to.split("::").next().unwrap_or(&e.to).trim();
        if !target.is_empty() && target != "self" && target != "super" && target != "crate" {
            *imports.entry(target.to_string()).or_insert(0) += 1;
        }
    }

    Ok(Some(IndexFacts {
        manifest,
        subsystems,
        modules,
        entry_points,
        top_imports: sort_counts(imports).into_iter().take(12).collect(),
        graph_nodes: graph.nodes.len(),
        graph_edges: graph.edges.len(),
    }))
}

fn subdir(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Subsystem => "subsystems",
        MemoryKind::Module => "modules",
        _ => "architecture",
    }
}

fn read_patches(repo: &Path) -> Vec<PatchRecord> {
    let mut v: Vec<PatchRecord> =
        read_json(&repo.join(TROVE_DIR).join("patches.json")).unwrap_or_default();
    v.sort_by_key(|p| std::cmp::Reverse(p.date));
    v.truncate(30);
    v
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn readme_intro_skips_badges_and_title() {
        let r = parse_readme(
            "README.md",
            "<p align=\"center\"><img src=\"x\"></p>\n\n# App\n\n[![ci](x)](y)\n\nApp does things.\n\nMore.\n\n## Install\n\nrun it\n",
        );
        assert_eq!(r.intro, "App does things.\n\nMore.");
        assert!(r
            .sections
            .iter()
            .any(|s| s.heading == "Install" && s.body.contains("run it")));
    }

    #[test]
    fn sections_ignore_fenced_headings() {
        let s = split_sections("## A\n```\n# not a heading\n```\n## B\nb\n");
        assert_eq!(s.len(), 2);
        assert!(s[0].body.contains("# not a heading"));
    }

    #[test]
    fn markdown_entry_frontmatter() {
        let e = parse_md_entry(
            ".claude/trove/entries/x.md",
            "x",
            "---\ntitle: Use WAL\ntype: decision\ncreated: 2026-01-02\ntags: [db, sqlite]\n---\n\nBody here.\n",
        )
        .unwrap();
        assert_eq!(e.title, "Use WAL");
        assert_eq!(e.kind, "decision");
        assert_eq!(e.tags, vec!["db", "sqlite"]);
        assert_eq!(e.body, "Body here.");
    }

    #[test]
    fn json_entry_renders_data() {
        let e = parse_json_entry(
            "e.json",
            "e",
            r#"{"title":"Ports","type":"reference","summary":"local ports","data":{"api":8080}}"#,
        )
        .unwrap();
        assert!(e.body.starts_with("local ports"));
        assert!(e.body.contains("\"api\": 8080"));
    }

    #[test]
    fn env_names_only() {
        assert_eq!(
            env_var_names("# c\nDATABASE_URL=postgres://x\nexport API_KEY=abc\nbad line\n"),
            vec!["DATABASE_URL", "API_KEY"]
        );
    }

    #[test]
    fn markers_need_a_comment() {
        let mut m = Vec::new();
        collect_markers(
            "a.rs",
            "// TODO: fix this\nlet todo = 1;\n# FIXME later\nlet s = \"// TODO: in a string\";\nx(); // HACK: tail\nf(\"a \\\"// TODO: escaped\\\" b\");\n",
            &mut m,
        );
        assert_eq!(m.len(), 3);
        assert_eq!(m[0].text, "fix this");
        assert_eq!(m[1].line, 3);
        assert_eq!(m[2].text, "tail");
    }

    #[test]
    fn adr_and_rule_detection() {
        assert!(is_adr("docs/adr/0001-use-postgres.md"));
        assert!(!is_adr("docs/adr.md"));
        assert!(is_rule_file(".cursor/rules/style.mdc"));
        assert!(is_rule_file("CLAUDE.md"));
        assert!(!is_rule_file("docs/CLAUDE.md"));
        assert!(is_fixture_or_vendor("tests/fixtures/app/README.md"));
        assert!(!is_fixture_or_vendor("src/fixtures.rs"));
    }
}
