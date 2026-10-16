//! Compose handoff documents from collected facts. Output is plain Markdown and
//! fully deterministic: the same inputs always produce the same bytes.

use crate::collect::{split_sections, truncate_paragraphs, Facts, TroveEntry, MAX_INCLUDED_CHARS};

#[derive(Debug, Clone)]
pub struct Doc {
    /// File name inside the bundle, such as `setup.md`.
    pub file: &'static str,
    pub title: &'static str,
    pub summary: &'static str,
    pub body: String,
}

pub const SECTIONS: &[(&str, &str, &str)] = &[
    (
        "overview.md",
        "Overview",
        "What the project is, its shape and where things live.",
    ),
    (
        "architecture.md",
        "Architecture",
        "Subsystems, modules, entry points and dependencies.",
    ),
    (
        "setup.md",
        "Setup",
        "Prerequisites, build and run commands, environment.",
    ),
    (
        "conventions.md",
        "Conventions",
        "House rules, tooling and code layout.",
    ),
    (
        "decisions.md",
        "Decisions",
        "Why things are the way they are.",
    ),
    ("gotchas.md", "Gotchas", "Traps and open TODOs."),
];

pub fn build(facts: &Facts) -> Vec<Doc> {
    SECTIONS
        .iter()
        .map(|&(file, title, summary)| {
            let body = match file {
                "overview.md" => overview(facts),
                "architecture.md" => architecture(facts),
                "setup.md" => setup(facts),
                "conventions.md" => conventions(facts),
                "decisions.md" => decisions(facts),
                _ => gotchas(facts),
            };
            Doc {
                file,
                title,
                summary,
                body,
            }
        })
        .collect()
}

fn header(out: &mut String, facts: &Facts, title: &str) {
    out.push_str(&format!("# {}: {title}\n\n", facts.name));
}

fn none(out: &mut String, what: &str) {
    out.push_str(&format!("_No {what} found._\n\n"));
}

fn overview(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "overview");
    match f.readme.as_ref().filter(|r| !r.intro.is_empty()) {
        Some(r) => {
            out.push_str(&r.intro);
            out.push_str(&format!("\n\n_From `{}`._\n\n", r.path));
        }
        None => out.push_str("_No README introduction found._\n\n"),
    }

    out.push_str("## At a glance\n\n");
    if !f.languages.is_empty() {
        let langs: Vec<String> = f
            .languages
            .iter()
            .take(6)
            .map(|(l, n)| format!("{l} ({})", plural(*n, "file")))
            .collect();
        out.push_str(&format!("- **Languages**: {}\n", langs.join(", ")));
    }
    out.push_str(&format!("- **Files scanned**: {}\n", f.files_scanned));
    if let Some(ix) = &f.index {
        out.push_str(&format!(
            "- **Index**: {}, {}, {}\n",
            plural(ix.manifest.symbol_count, "symbol"),
            plural(ix.manifest.module_count, "module"),
            plural(ix.manifest.subsystem_count, "subsystem")
        ));
    }
    if !f.build.is_empty() {
        let names: Vec<_> = f.build.iter().map(|b| b.name.as_str()).collect();
        out.push_str(&format!("- **Build**: {}\n", names.join(", ")));
    }
    if !f.trove.is_empty() {
        let count = |k: &str| f.trove.iter().filter(|e| e.kind == k).count();
        out.push_str(&format!(
            "- **Project trove**: {} ({}, {}, {})\n",
            plural(f.trove.len(), "entry"),
            plural(count("decision"), "decision"),
            plural(count("gotcha"), "gotcha"),
            plural(count("preference"), "convention")
        ));
    }
    out.push('\n');

    out.push_str("## Layout\n\n");
    if f.top_level.is_empty() {
        none(&mut out, "files");
    } else {
        for e in &f.top_level {
            if e.is_dir {
                out.push_str(&format!("- `{}/`: {}\n", e.name, plural(e.files, "file")));
            } else {
                out.push_str(&format!("- `{}`\n", e.name));
            }
        }
        out.push('\n');
    }

    let notes: Vec<&TroveEntry> = f
        .trove
        .iter()
        .filter(|e| matches!(e.kind.as_str(), "reference" | "project" | "snippet"))
        .collect();
    if !notes.is_empty() {
        out.push_str("## Notes from the project trove\n\n");
        entries(&mut out, &notes);
    }

    out.push_str("## Read next\n\n");
    for &(file, title, summary) in &SECTIONS[1..] {
        out.push_str(&format!("- [{title}]({file}): {summary}\n"));
    }
    out
}

fn architecture(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "architecture");
    let Some(ix) = &f.index else {
        out.push_str("_No Trove index found. Run `trove index` (or `trove-ship --reindex`) for subsystem and module detail._\n");
        return out;
    };
    out.push_str(&format!(
        "Built from the Trove engine index: {} in {} across {}. \
         The dependency graph has {} and {}.\n\n",
        plural(ix.manifest.symbol_count, "symbol"),
        plural(ix.manifest.module_count, "module"),
        plural(ix.manifest.subsystem_count, "subsystem"),
        plural(ix.graph_nodes, "node"),
        plural(ix.graph_edges, "edge")
    ));

    out.push_str("## Subsystems\n\n");
    if ix.subsystems.is_empty() {
        none(&mut out, "subsystems");
    } else {
        out.push_str("| Subsystem | Files | Symbols | Mostly |\n|---|---|---|---|\n");
        for s in ix.subsystems.iter().take(20) {
            let kinds: Vec<String> = s.kinds.iter().map(|(k, n)| format!("{n} {k}")).collect();
            out.push_str(&format!(
                "| `{}` | {} | {} | {} |\n",
                s.name,
                s.files,
                s.symbols,
                kinds.join(", ")
            ));
        }
        out.push('\n');
    }

    out.push_str("## Entry points\n\n");
    if ix.entry_points.is_empty() {
        none(&mut out, "entry points");
    } else {
        for e in &ix.entry_points {
            out.push_str(&format!("- `{e}`\n"));
        }
        out.push('\n');
    }

    if !ix.top_imports.is_empty() {
        out.push_str("## Most used dependencies\n\n");
        for (name, n) in &ix.top_imports {
            out.push_str(&format!("- `{name}`: imported {n} times\n"));
        }
        out.push('\n');
    }

    out.push_str("## Module map\n\n");
    let mut current = String::new();
    let mut shown = 0;
    for m in ix.modules.iter().filter(|m| !m.exports.is_empty()) {
        if shown >= 80 {
            out.push_str("\n_Module map truncated._\n");
            break;
        }
        let pkg = m.path.split('/').next().unwrap_or(&m.path).to_string();
        if pkg != current {
            out.push_str(&format!("\n### `{pkg}`\n\n"));
            current = pkg;
        }
        let exports: Vec<_> = m.exports.iter().take(6).cloned().collect();
        let more = m.exports.len().saturating_sub(6);
        let suffix = if more > 0 {
            format!(", and {more} more")
        } else {
            String::new()
        };
        out.push_str(&format!("- `{}`: {}{suffix}\n", m.path, exports.join(", ")));
        shown += 1;
    }
    if shown == 0 {
        none(&mut out, "modules with exported symbols");
    }
    out
}

fn setup(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "setup");

    out.push_str("## Prerequisites\n\n");
    let mut prereqs: Vec<String> = f
        .build
        .iter()
        .filter_map(|b| b.prerequisite.clone())
        .collect();
    prereqs.dedup();
    if prereqs.is_empty() {
        none(&mut out, "toolchain requirements");
    } else {
        for p in prereqs {
            out.push_str(&format!("- {p}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Build and run\n\n");
    if f.build.is_empty() {
        none(&mut out, "build system");
    }
    for b in &f.build {
        out.push_str(&format!("### {} (`{}`)\n\n", b.name, b.file));
        out.push_str("| Command | What it does |\n|---|---|\n");
        for (cmd, what) in &b.commands {
            out.push_str(&format!("| `{}` | {} |\n", cmd, escape_cell(what)));
        }
        out.push('\n');
    }

    if !f.env_vars.is_empty() {
        out.push_str("## Environment variables\n\n");
        out.push_str(
            "Names only. Values are never copied into these docs; ask the team for them.\n\n",
        );
        for (file, vars) in &f.env_vars {
            out.push_str(&format!("From `{file}`:\n\n"));
            for v in vars {
                out.push_str(&format!("- `{v}`\n"));
            }
            out.push('\n');
        }
    }

    if !f.ci_commands.is_empty() {
        out.push_str("## What CI runs\n\n");
        for c in &f.ci_commands {
            out.push_str(&format!("- `{c}`\n"));
        }
        out.push('\n');
    }

    if let Some(r) = &f.readme {
        let picked = setup_sections(&r.sections);
        if !picked.is_empty() {
            out.push_str(&format!("## From `{}`\n\n", r.path));
            for (heading, body) in picked {
                out.push_str(&format!("### {heading}\n\n"));
                out.push_str(&include(&body, 4));
                out.push_str("\n\n");
            }
        }
    }
    out
}

/// README sections that describe installing, building or running the project,
/// each with its nested subsections.
fn setup_sections(sections: &[crate::collect::Section]) -> Vec<(String, String)> {
    const KEYS: &[&str] = &[
        "install",
        "setup",
        "set up",
        "getting started",
        "quick start",
        "quickstart",
        "build",
        "develop",
        "running",
        "usage",
        "requirement",
        "prerequisite",
        "configur",
        "environment",
        "testing",
        "local",
    ];
    let mut out = Vec::new();
    let mut i = 0;
    while i < sections.len() && out.len() < 6 {
        let s = &sections[i];
        let h = s.heading.to_lowercase();
        if s.level >= 2 && KEYS.iter().any(|k| h.contains(k)) {
            let mut body = s.body.clone();
            let mut j = i + 1;
            while j < sections.len() && sections[j].level > s.level {
                body.push_str(&format!(
                    "{} {}\n",
                    "#".repeat(sections[j].level),
                    sections[j].heading
                ));
                body.push_str(&sections[j].body);
                j += 1;
            }
            out.push((s.heading.clone(), body));
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn conventions(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "conventions");

    out.push_str("## House rules\n\n");
    if f.rule_files.is_empty() {
        none(&mut out, "agent or contributor rule files");
    }
    for r in &f.rule_files {
        out.push_str(&format!("### `{}`\n\n", r.path));
        out.push_str(&include(&r.content, 4));
        out.push_str("\n\n");
    }

    let prefs: Vec<&TroveEntry> = f
        .trove
        .iter()
        .filter(|e| e.kind == "preference" || e.tags.iter().any(|t| t == "convention"))
        .collect();
    if !prefs.is_empty() {
        out.push_str("## From the project trove\n\n");
        entries(&mut out, &prefs);
    }

    out.push_str("## Tooling\n\n");
    if f.tooling.is_empty() {
        none(&mut out, "formatter or linter configuration");
    } else {
        for (path, label) in &f.tooling {
            out.push_str(&format!("- `{path}`: {label}\n"));
        }
        out.push('\n');
    }

    out.push_str("## Tests\n\n");
    if f.test_layout.is_empty() {
        none(&mut out, "test directories");
    } else {
        for t in &f.test_layout {
            out.push_str(&format!("- {t}\n"));
        }
        out.push('\n');
    }
    out
}

fn decisions(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "decisions");
    let decided: Vec<&TroveEntry> = f.trove.iter().filter(|e| e.kind == "decision").collect();

    out.push_str("## From the project trove\n\n");
    if decided.is_empty() {
        none(&mut out, "decision entries");
    } else {
        entries(&mut out, &decided);
    }

    out.push_str("## Architecture decision records\n\n");
    if f.adrs.is_empty() {
        none(&mut out, "ADRs");
    }
    for a in &f.adrs {
        out.push_str(&format!("### {} (`{}`)\n\n", a.title, a.path));
        let body: String = split_sections(&a.content)
            .into_iter()
            .skip_while(|s| s.level == 1 && s.body.trim().is_empty())
            .map(|s| {
                if s.level == 1 {
                    s.body
                } else {
                    format!("{} {}\n{}", "#".repeat(s.level), s.heading, s.body)
                }
            })
            .collect();
        let body = if body.trim().is_empty() {
            a.content.clone()
        } else {
            body
        };
        out.push_str(&include(&body, 4));
        out.push_str("\n\n");
    }

    if !f.patches.is_empty() {
        out.push_str("## Recorded patches\n\n");
        for p in &f.patches {
            out.push_str(&format!(
                "- `{}` ({}): {}\n",
                p.path,
                p.date.format("%Y-%m-%d"),
                p.reason
            ));
        }
        out.push('\n');
    }
    out
}

fn gotchas(f: &Facts) -> String {
    let mut out = String::new();
    header(&mut out, f, "gotchas");
    let traps: Vec<&TroveEntry> = f.trove.iter().filter(|e| e.kind == "gotcha").collect();

    out.push_str("## From the project trove\n\n");
    if traps.is_empty() {
        none(&mut out, "gotcha entries");
    } else {
        entries(&mut out, &traps);
    }

    out.push_str("## Open markers in code\n\n");
    if f.markers.is_empty() {
        none(&mut out, "TODO, FIXME, HACK or XXX markers");
    } else {
        for m in &f.markers {
            out.push_str(&format!(
                "- **{}** `{}:{}`: {}\n",
                m.tag, m.path, m.line, m.text
            ));
        }
        out.push('\n');
    }

    out
}

fn entries(out: &mut String, list: &[&TroveEntry]) {
    for e in list {
        out.push_str(&format!("### {}\n\n", e.title));
        let mut meta = Vec::new();
        if !e.created.is_empty() {
            meta.push(e.created.clone());
        }
        if !e.tags.is_empty() {
            meta.push(format!("tags: {}", e.tags.join(", ")));
        }
        meta.push(format!("`{}`", e.file));
        out.push_str(&format!("_{}_\n\n", meta.join(" | ")));
        out.push_str(&include(&e.body, 4));
        out.push_str("\n\n");
    }
}

/// Embed external Markdown under a heading level: headings are shifted so the
/// shallowest one becomes `level`, length is capped, and code fences stay
/// balanced.
pub fn include(md: &str, level: usize) -> String {
    let mut text = truncate_paragraphs(md.trim(), MAX_INCLUDED_CHARS);
    let truncated = text.len() < md.trim().len();
    let mut in_fence = false;
    let mut min_level = usize::MAX;
    for line in text.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        } else if !in_fence {
            let n = line.chars().take_while(|&c| c == '#').count();
            if (1..=6).contains(&n) && line[n..].starts_with(' ') {
                min_level = min_level.min(n);
            }
        }
    }
    if min_level != usize::MAX {
        let mut shifted = String::new();
        in_fence = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_fence = !in_fence;
            }
            let n = line.chars().take_while(|&c| c == '#').count();
            if !in_fence && (1..=6).contains(&n) && line[n..].starts_with(' ') {
                let new = (n + level - min_level).min(6);
                shifted.push_str(&"#".repeat(new));
                shifted.push_str(&line[n..]);
            } else {
                shifted.push_str(line);
            }
            shifted.push('\n');
        }
        text = shifted.trim_end().to_string();
    }
    if text
        .lines()
        .filter(|l| l.trim_start().starts_with("```"))
        .count()
        % 2
        == 1
    {
        text.push_str("\n```");
    }
    if truncated {
        text.push_str("\n\n_(truncated)_");
    }
    text
}

fn plural(n: usize, word: &str) -> String {
    match (n, word.strip_suffix('y')) {
        (1, _) => format!("1 {word}"),
        (_, Some(stem)) => format!("{n} {stem}ies"),
        _ => format!("{n} {word}s"),
    }
}

fn escape_cell(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn include_shifts_headings_and_balances_fences() {
        let out = include("# Top\ntext\n## Sub\n```\n# code\n", 4);
        assert!(out.starts_with("#### Top"));
        assert!(out.contains("##### Sub"));
        assert!(out.contains("# code"));
        assert!(out.ends_with("```"));
    }

    #[test]
    fn setup_sections_take_nested_children() {
        let secs = split_sections("## Install\na\n### Linux\nb\n## License\nc\n");
        let picked = setup_sections(&secs);
        assert_eq!(picked.len(), 1);
        assert!(picked[0].1.contains("### Linux"));
        assert!(!picked[0].1.contains("License"));
    }

    #[test]
    fn empty_facts_still_produce_every_doc() {
        let facts = Facts {
            name: "demo".into(),
            ..Default::default()
        };
        let docs = build(&facts);
        assert_eq!(docs.len(), SECTIONS.len());
        assert!(docs.iter().all(|d| d.body.starts_with("# demo: ")));
    }
}
