//! Loaders that turn on-disk knowledge into `ask` documents.

use std::path::{Path, PathBuf};

use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::store::{TroveStore, TROVE_DIR};
use trove_index::symbol::{Symbol, SymbolKind};

use crate::ask::{Doc, SourceKind};

const MAX_DOC_BYTES: u64 = 256 * 1024;
const MAX_DOC_DEPTH: usize = 3;

/// Entries of a Claude trove (`~/.claude/trove` or `./.claude/trove`):
/// `entries/*.md` and `entries/*.json`, one fact per file.
pub fn trove_entries(
    dir: &Path,
    kind: SourceKind,
    display_root: &dyn Fn(&Path) -> String,
) -> Vec<Doc> {
    let Ok(rd) = std::fs::read_dir(dir.join("entries")) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            matches!(
                p.extension().and_then(|e| e.to_str()),
                Some("md") | Some("json")
            )
        })
        .collect();
    paths.sort();
    paths
        .into_iter()
        .filter_map(|path| {
            let text = read_small(&path)?;
            let slug = path.file_stem()?.to_string_lossy().into_owned();
            let title = entry_title(&text).unwrap_or_else(|| slug.replace('-', " "));
            Some(
                Doc::new(kind, display_root(&path), title)
                    .with_text(text)
                    .at_line(1, true),
            )
        })
        .collect()
}

/// Title from YAML frontmatter, a JSON `title` field, or the first heading.
pub fn entry_title(text: &str) -> Option<String> {
    let trimmed = text.trim_start();
    if trimmed.starts_with('{') {
        let v: serde_json::Value = serde_json::from_str(trimmed).ok()?;
        return v.get("title")?.as_str().map(str::to_string);
    }
    if let Some(rest) = trimmed.strip_prefix("---") {
        for line in rest.lines().skip(1) {
            if line.trim() == "---" {
                break;
            }
            if let Some(t) = line.strip_prefix("title:") {
                let t = t.trim().trim_matches('"').trim_matches('\'');
                if !t.is_empty() {
                    return Some(t.to_string());
                }
            }
        }
    }
    text.lines()
        .find_map(|l| l.strip_prefix("# "))
        .map(|t| t.trim().to_string())
}

/// Markdown docs in the repo: top-level `*.md` plus `docs/`, split into one
/// document per heading so citations land on a section.
pub fn repo_docs(repo: &Path) -> Vec<Doc> {
    let mut files = Vec::new();
    collect_md(repo, 0, 0, &mut files);
    collect_md(&repo.join("docs"), 0, MAX_DOC_DEPTH, &mut files);
    files.sort();
    files.dedup();
    let mut docs = Vec::new();
    for file in files {
        let Some(text) = read_small(&file) else {
            continue;
        };
        let rel = file
            .strip_prefix(repo)
            .unwrap_or(&file)
            .to_string_lossy()
            .into_owned();
        docs.extend(markdown_sections(&rel, &text));
    }
    docs
}

fn collect_md(dir: &Path, depth: usize, max_depth: usize, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name.to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(ft) = entry.file_type() else { continue };
        if ft.is_dir() && depth < max_depth {
            collect_md(&path, depth + 1, max_depth, out);
        } else if ft.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            out.push(path);
        }
    }
}

/// Split markdown at headings (outside code fences). Each section keeps its
/// starting line so snippets cite `file:line`.
pub fn markdown_sections(path: &str, text: &str) -> Vec<Doc> {
    let file_title = Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let mut docs = Vec::new();
    let mut title = file_title.clone();
    let mut start = 1usize;
    let mut buf: Vec<&str> = Vec::new();
    let mut in_fence = false;

    let mut flush = |title: &str, start: usize, buf: &mut Vec<&str>| {
        let body = buf.join("\n");
        if body.chars().any(|c| c.is_alphanumeric()) {
            docs.push(
                Doc::new(SourceKind::Doc, path, title)
                    .with_text(body)
                    .at_line(start, true),
            );
        }
        buf.clear();
    };

    for (i, line) in text.lines().enumerate() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
        }
        let heading = (!in_fence)
            .then(|| line.strip_prefix('#'))
            .flatten()
            .map(|h| h.trim_start_matches('#'))
            .filter(|h| h.starts_with(' '));
        if let Some(h) = heading {
            flush(&title, start, &mut buf);
            title = h.trim().to_string();
            start = i + 1;
        }
        buf.push(line);
    }
    flush(&title, start, &mut buf);
    docs
}

/// The engine index in `.trove/`: symbols, modules, subsystems and the
/// architecture map. Empty when the repo has not been indexed.
pub fn engine_index(repo: &Path) -> Vec<Doc> {
    let trove = repo.join(TROVE_DIR);
    if !trove.join("manifest.json").exists() {
        return Vec::new();
    }
    let Ok(store) = TroveStore::open(repo) else {
        return Vec::new();
    };
    let mut docs = Vec::new();

    let symbols: Vec<Symbol> = store
        .read_json("symbols.json")
        .ok()
        .flatten()
        .unwrap_or_default();
    for s in symbols {
        let name = s.name.trim_end_matches(';').trim();
        if name.is_empty() || matches!(s.kind, SymbolKind::Import | SymbolKind::Variable) {
            continue;
        }
        // `mod x;` declarations and tests are rarely what a question is after.
        let weight = match s.kind {
            SymbolKind::Module => 0.4,
            SymbolKind::Test => 0.6,
            _ => 1.0,
        };
        let stem = Path::new(&s.path)
            .file_stem()
            .map(|x| x.to_string_lossy().into_owned())
            .unwrap_or_default();
        let title = match &s.owner {
            Some(o) => format!("{o}::{name}"),
            None => name.to_string(),
        };
        let text = match s.signature.as_deref().map(str::trim) {
            Some(sig) if !sig.is_empty() => sig.to_string(),
            _ => format!("{} {name}", s.kind.label()),
        };
        docs.push(
            Doc::new(SourceKind::Symbol, s.path.clone(), title)
                .with_text(text)
                .with_keywords(format!("{} {name} {stem}", s.kind.label()))
                .at_line(s.start_line, false)
                .weighted(weight),
        );
    }

    for (kind, source) in [
        (MemoryKind::Module, SourceKind::Module),
        (MemoryKind::Subsystem, SourceKind::Subsystem),
        (MemoryKind::Architecture, SourceKind::Architecture),
    ] {
        for obj in store.read_objects(kind).unwrap_or_default() {
            docs.push(object_doc(&obj, source));
        }
    }
    docs
}

fn object_doc(obj: &MemoryObject, kind: SourceKind) -> Doc {
    let path = obj.path.clone().unwrap_or_else(|| obj.id.clone());
    let title = obj
        .id
        .split_once(':')
        .map(|(_, rest)| rest.to_string())
        .unwrap_or_else(|| obj.id.clone());
    let text = obj
        .text
        .raw
        .as_deref()
        .or(obj.text.summary.as_deref())
        .unwrap_or_default();
    // Generated summaries open with a "# Module: path" heading that only
    // repeats the title.
    let body = match text.split_once('\n') {
        Some((first, rest)) if first.starts_with("# ") => rest,
        _ => text,
    };
    Doc::new(kind, path, title).with_text(body.trim_start())
}

fn read_small(path: &Path) -> Option<String> {
    let meta = std::fs::metadata(path).ok()?;
    if meta.len() > MAX_DOC_BYTES {
        return None;
    }
    std::fs::read_to_string(path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_from_frontmatter_json_and_heading() {
        let md = "---\ntitle: Use pgx not an ORM\nslug: use-pgx\ntype: decision\n---\n\nBody\n";
        assert_eq!(entry_title(md).as_deref(), Some("Use pgx not an ORM"));
        let json = r#"{"title": "Service ports", "ports": {"api": 8080}}"#;
        assert_eq!(entry_title(json).as_deref(), Some("Service ports"));
        assert_eq!(entry_title("# Heading\ntext").as_deref(), Some("Heading"));
        assert_eq!(entry_title("plain"), None);
    }

    #[test]
    fn markdown_splits_on_headings_outside_fences() {
        let md = "# Title\nintro text\n\n## Install\nrun it\n```sh\n# not a heading\n```\n## Use\nquery things\n";
        let docs = markdown_sections("README.md", md);
        let summary: Vec<(&str, Option<usize>)> =
            docs.iter().map(|d| (d.title.as_str(), d.line)).collect();
        assert_eq!(
            summary,
            [("Title", Some(1)), ("Install", Some(4)), ("Use", Some(9))]
        );
        assert!(docs[1].text.contains("# not a heading"));
    }

    #[test]
    fn loads_trove_entries() {
        let dir = std::env::temp_dir().join(format!("trove-ask-entries-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("entries")).unwrap();
        std::fs::write(
            dir.join("entries/use-pgx.md"),
            "---\ntitle: Use pgx\n---\nWe use pgx.\n",
        )
        .unwrap();
        std::fs::write(dir.join("entries/ports.json"), r#"{"title":"Ports"}"#).unwrap();
        std::fs::write(dir.join("entries/notes.txt"), "skip").unwrap();
        let docs = trove_entries(&dir, SourceKind::Personal, &|p| {
            p.file_name().unwrap().to_string_lossy().into_owned()
        });
        let titles: Vec<&str> = docs.iter().map(|d| d.title.as_str()).collect();
        assert_eq!(titles, ["Ports", "Use pgx"]);
        assert_eq!(docs[1].path, "use-pgx.md");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
