//! `trove remember`: the deterministic Rust write path into trove memory.
//!
//! Mirrors the remember skill: route the entry type to a section, slugify the
//! title, write one entry file with the shared front matter, guard against
//! secrets, and add a newest-first line to the section's INDEX.md.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use trove_redact::detectors::builtin;
use trove_redact::Category;

/// Canonical entry types the remember skill writes.
pub const ENTRY_TYPES: [&str; 8] = [
    "convention",
    "decision",
    "gotcha",
    "preference",
    "reference",
    "project",
    "snippet",
    "context",
];

/// Trove sections, in canonical order.
pub const SECTIONS: [&str; 3] = ["conventions", "memory", "context"];

/// Input to one remember invocation.
#[derive(Debug, Clone)]
pub struct RememberOptions {
    /// Human readable title (already joined if it came as several words).
    pub title: String,
    /// Entry body prose.
    pub body: String,
    /// One of [`ENTRY_TYPES`]; drives section routing.
    pub entry_type: String,
    /// Optional tags for the front matter.
    pub tags: Vec<String>,
    /// Explicit section override (conventions | memory | context).
    pub section: Option<String>,
    /// Replace an existing entry with the same slug instead of refusing.
    pub update: bool,
}

/// What one remember invocation produced.
#[derive(Debug, Clone)]
pub struct Remembered {
    /// Section the entry landed in (empty for a flat trove).
    pub section: String,
    /// Slug of the written entry.
    pub slug: String,
    /// Absolute path of the written entry file.
    pub path: PathBuf,
    /// Whether an existing entry was replaced.
    pub updated: bool,
}

/// Remember one entry into the project trove at `<repo>/.claude/trove` (if it
/// has an INDEX.md) or the flat user trove at `~/.claude/trove`.
///
/// Deterministic semantics: the slug of an existing entry is never silently
/// duplicated — without `update` the call refuses, with it the entry is
/// replaced (keeping its original `created` date). Secrets (the detector
/// classes `trove.sh guard` refuses) abort before anything is written.
pub fn remember(repo: &Path, home: &Path, opts: &RememberOptions) -> Result<Remembered> {
    if !ENTRY_TYPES.contains(&opts.entry_type.as_str()) {
        bail!(
            "unknown type '{}'; expected one of {}",
            opts.entry_type,
            ENTRY_TYPES.join(", ")
        );
    }

    // Resolve the trove: project trove if it has an INDEX.md, else the user trove.
    let project = repo.join(".claude/trove");
    let user = home.join(".claude/trove");
    let (trove, sectioned) = if project.join("INDEX.md").is_file() {
        (project, true)
    } else if user.join("INDEX.md").is_file() {
        (user, false)
    } else {
        bail!(
            "no trove found at {} or {}; create one first (trove.sh init or an INDEX.md)",
            project.display(),
            user.display()
        );
    };

    let section = match (&opts.section, sectioned) {
        (Some(s), true) => {
            if !SECTIONS.contains(&s.as_str()) {
                bail!(
                    "unknown section '{}'; expected one of {}",
                    s,
                    SECTIONS.join(", ")
                );
            }
            s.clone()
        }
        (Some(_), false) => {
            bail!(
                "--section needs a sectioned project trove; the user trove is flat (entries/ only)"
            )
        }
        (None, true) => route_section(&opts.entry_type).to_string(),
        (None, false) => String::new(),
    };

    let slug = slugify(&opts.title);
    if slug.is_empty() {
        bail!("title {:?} yields an empty slug", opts.title);
    }
    let entries_dir = if sectioned {
        trove.join(&section).join("entries")
    } else {
        trove.join("entries")
    };
    let entry_path = entries_dir.join(format!("{slug}.md"));

    let existed = entry_path.exists() || entries_dir.join(format!("{slug}.json")).exists();
    if existed && !opts.update {
        bail!(
            "entry '{slug}' already exists in {}; rerun with --update to replace it",
            entries_dir.display()
        );
    }

    let created = if existed {
        existing_created(&entry_path).unwrap_or_else(|| {
            chrono::Utc::now()
                .date_naive()
                .format("%Y-%m-%d")
                .to_string()
        })
    } else {
        chrono::Utc::now()
            .date_naive()
            .format("%Y-%m-%d")
            .to_string()
    };

    let tags = if opts.tags.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", opts.tags.join(", "))
    };
    let content = format!(
        "---\ntitle: {}\nslug: {slug}\ntype: {}\ncreated: {created}\ntags: {tags}\n---\n\n{}\n",
        opts.title,
        opts.entry_type,
        opts.body.trim_end()
    );

    // Guard before touching the filesystem: a refusal must leave no entry
    // file and no index change. Never echo the matched value.
    guard_secrets(&content)?;

    std::fs::create_dir_all(&entries_dir).context("creating entries dir")?;
    atomic_write(&entry_path, content.as_bytes())
        .with_context(|| format!("writing entry {}", entry_path.display()))?;

    let index_path = if sectioned {
        trove.join(&section).join("INDEX.md")
    } else {
        trove.join("INDEX.md")
    };
    let hook = hook_of(&opts.body);
    let line = format!("- [{}](entries/{slug}.md) - {hook}", opts.title);
    let index_result = update_index(&index_path, &section, &slug, &line);
    if let Err(e) = index_result {
        // Roll the entry back so a failed index write leaves no partial state.
        if !existed {
            let _ = std::fs::remove_file(&entry_path);
        }
        return Err(e.context("updating INDEX.md (entry written then rolled back)"));
    }

    // An update may replace an entry previously stored as JSON.
    let json_sibling = entries_dir.join(format!("{slug}.json"));
    if existed {
        let _ = std::fs::remove_file(&json_sibling);
    }

    Ok(Remembered {
        section,
        slug,
        path: entry_path,
        updated: existed,
    })
}

/// Section an entry type routes to, mirroring `trove.sh route`.
fn route_section(entry_type: &str) -> &'static str {
    match entry_type {
        "convention" | "preference" => "conventions",
        "context" => "context",
        _ => "memory",
    }
}

/// Refuse obvious secrets: every detector category `trove.sh guard` also
/// refuses. Names the line and the detector, never the value.
fn guard_secrets(content: &str) -> Result<()> {
    let mut hits = Vec::new();
    for detector in builtin() {
        detector.run(content, &mut hits);
    }
    let mut secret_hits: Vec<&trove_redact::Detection> = hits
        .iter()
        .filter(|d| d.category == Category::Secret)
        .collect();
    secret_hits.sort_by_key(|d| d.start);
    if let Some(first) = secret_hits.first() {
        let line = content[..first.start].matches('\n').count() + 1;
        bail!(
            "refusing: possible secret at line {line} ({}); secrets do not belong in a trove, rewrite the entry without it",
            first.reason
        );
    }
    Ok(())
}

/// `created:` value from an existing entry's front matter, if present.
fn existing_created(path: &Path) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    content
        .lines()
        .find_map(|l| l.strip_prefix("created: "))
        .map(|d| d.trim().to_string())
        .filter(|d| !d.is_empty())
}

/// One-line index hook: the first non-empty body line, whitespace collapsed,
/// cut to 100 characters on a word boundary.
fn hook_of(body: &str) -> String {
    let first = body
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let collapsed = first.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.chars().count() <= 100 {
        return collapsed;
    }
    let mut cut: String = collapsed.chars().take(100).collect();
    if let Some(sp) = cut.rfind(' ') {
        cut.truncate(sp);
    }
    format!("{cut}...")
}

/// Insert or refresh the entry line in an INDEX.md, newest first right below
/// the `<!-- entries below -->` marker.
fn update_index(index_path: &Path, section: &str, slug: &str, line: &str) -> Result<()> {
    let template = if section.is_empty() {
        "# Trove Index\n\n<!-- entries below -->\n".to_string()
    } else {
        format!(
            "# {} Index\n\n> One line per entry, newest first.\n\n<!-- entries below -->\n",
            section_title(section)
        )
    };
    let mut content = std::fs::read_to_string(index_path).unwrap_or(template);
    let mut lines: Vec<String> = content.lines().map(str::to_string).collect();

    // Refresh (not duplicate) any line already pointing at this slug.
    lines.retain(|l| {
        !(l.contains(&format!("(entries/{slug}.md)"))
            || l.contains(&format!("(entries/{slug}.json)")))
    });

    match lines
        .iter()
        .position(|l| l.trim() == "<!-- entries below -->")
    {
        Some(i) => lines.insert(i + 1, line.to_string()),
        None => {
            lines.push(String::new());
            lines.push("<!-- entries below -->".to_string());
            lines.push(line.to_string());
        }
    }
    content = lines.join("\n");
    if !content.ends_with('\n') {
        content.push('\n');
    }
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    atomic_write(index_path, content.as_bytes())
        .with_context(|| format!("writing {}", index_path.display()))
}

fn section_title(section: &str) -> String {
    let mut cs = section.chars();
    match cs.next() {
        Some(c) => c.to_uppercase().collect::<String>() + cs.as_str(),
        None => String::new(),
    }
}

/// Write via a sibling temp file + rename so a crash never leaves a partial file.
fn atomic_write(dest: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = dest.with_file_name(format!(
        ".{}.{}",
        dest.file_name().and_then(|n| n.to_str()).unwrap_or("file"),
        std::process::id()
    ));
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, dest)?;
    Ok(())
}

/// kebab-case slug from a title.
pub fn slugify(title: &str) -> String {
    let mut slug = String::new();
    let mut prev_hyphen = true;
    for c in title.chars() {
        if c.is_ascii_alphanumeric() {
            slug.push(c.to_ascii_lowercase());
            prev_hyphen = false;
        } else if !prev_hyphen {
            slug.push('-');
            prev_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

#[cfg(test)]
mod tests {
    use super::*;
    use trove_retrieve::ask::SourceKind;
    use trove_retrieve::sources::trove_entries;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("trove-remember-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    /// Project trove with sections and one pre-existing memory entry, so the
    /// newest-first INDEX assertion is meaningful.
    fn project_trove(repo: &Path) -> PathBuf {
        let trove = repo.join(".claude/trove");
        write(
            &trove.join("INDEX.md"),
            "# Trove Index\n\n<!-- entries below -->\n",
        );
        for section in SECTIONS {
            write(
                &trove.join(section).join("INDEX.md"),
                &format!("# {} Index\n\n> blurb\n\n<!-- entries below -->\n", section),
            );
            std::fs::create_dir_all(trove.join(section).join("entries")).unwrap();
        }
        write(
            &trove.join("memory/entries/old-decision.md"),
            "---\ntitle: Old decision\nslug: old-decision\ntype: decision\ncreated: 2025-12-01\ntags: []\n---\n\nOld body.\n",
        );
        write(
            &trove.join("memory/INDEX.md"),
            "# Memory Index\n\n> blurb\n\n<!-- entries below -->\n- [Old decision](entries/old-decision.md) - the old hook\n",
        );
        trove
    }

    fn opts(title: &str, entry_type: &str, body: &str) -> RememberOptions {
        RememberOptions {
            title: title.into(),
            body: body.into(),
            entry_type: entry_type.into(),
            tags: vec!["db".into()],
            section: None,
            update: false,
        }
    }

    #[test]
    fn writes_sectioned_entry_with_frontmatter_and_newest_first_index() {
        let repo = scratch("write");
        let trove = project_trove(&repo);
        let r = remember(
            &repo,
            &repo,
            &opts(
                "WAL gotcha: bulk inserts",
                "gotcha",
                "Bulk inserts must wrap in one transaction.\n",
            ),
        )
        .unwrap();
        assert_eq!(r.section, "memory");
        assert_eq!(r.slug, "wal-gotcha-bulk-inserts");
        let entry = &r.path;
        assert_eq!(
            entry,
            &trove.join("memory/entries/wal-gotcha-bulk-inserts.md")
        );
        let content = std::fs::read_to_string(entry).unwrap();
        assert!(content.starts_with("---\ntitle: WAL gotcha: bulk inserts\nslug: wal-gotcha-bulk-inserts\ntype: gotcha\ncreated: "));
        let created_line = content
            .lines()
            .find(|l| l.starts_with("created: "))
            .unwrap();
        let date = created_line.trim_start_matches("created: ");
        chrono::NaiveDate::parse_from_str(date, "%Y-%m-%d").expect("created is YYYY-MM-DD");
        assert!(content.contains("tags: [db]\n"));
        assert!(content.contains("---\n\nBulk inserts must wrap in one transaction.\n"));
        let index = std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap();
        let marker = index.find("<!-- entries below -->").unwrap();
        let old = index.find("Old decision").unwrap();
        let new = index.find("- [WAL gotcha: bulk inserts](entries/wal-gotcha-bulk-inserts.md) - Bulk inserts must wrap in one transaction.").unwrap();
        assert!(marker < new && new < old, "newest first below the marker");
    }

    #[test]
    fn refuses_duplicate_slug_and_leaves_files_unchanged() {
        let repo = scratch("dupe");
        let trove = project_trove(&repo);
        let o = opts("WAL gotcha: bulk inserts", "gotcha", "Body one.\n");
        remember(&repo, &repo, &o).unwrap();
        let entry = trove.join("memory/entries/wal-gotcha-bulk-inserts.md");
        let before = std::fs::read_to_string(&entry).unwrap();
        let index_before = std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap();
        let err = remember(
            &repo,
            &repo,
            &opts("WAL gotcha: bulk inserts", "decision", "Different body.\n"),
        )
        .unwrap_err();
        assert!(err.to_string().contains("wal-gotcha-bulk-inserts"), "{err}");
        assert!(err.to_string().contains("--update"), "{err}");
        assert_eq!(std::fs::read_to_string(&entry).unwrap(), before);
        assert_eq!(
            std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap(),
            index_before
        );
    }

    #[test]
    fn update_replaces_entry_and_refreshes_index_line() {
        let repo = scratch("update");
        let trove = project_trove(&repo);
        remember(
            &repo,
            &repo,
            &opts("WAL gotcha: bulk inserts", "gotcha", "Old body.\n"),
        )
        .unwrap();
        let mut o = opts("WAL gotcha: bulk inserts", "gotcha", "New body.\n");
        o.update = true;
        let r = remember(&repo, &repo, &o).unwrap();
        assert!(r.updated);
        let content = std::fs::read_to_string(&r.path).unwrap();
        assert!(content.contains("New body."));
        assert!(!content.contains("Old body."));
        // created from the first write is preserved, not reset to today only.
        assert_eq!(content.matches("created:").count(), 1);
        let index = std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap();
        assert_eq!(content.matches("slug: wal-gotcha-bulk-inserts").count(), 1);
        assert_eq!(
            index
                .lines()
                .filter(|l| l.contains("entries/wal-gotcha-bulk-inserts.md"))
                .count(),
            1,
            "index line refreshed, not duplicated"
        );
    }

    #[test]
    fn refuses_secrets_without_entry_or_index_change() {
        let repo = scratch("secret");
        let trove = project_trove(&repo);
        let index_before = std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap();
        // Assemble fake credentials at runtime so no scannable literal ships.
        let cat = |p: &[&str]| p.concat();
        let openai = cat(&["sk-", &"Ab12Cd34Ef56Gh78".repeat(3)]);
        let aws = cat(&["AKIA", "Z7Q3XK2M9P4L8W1N"]);
        let pem = cat(&[
            "-----BEGIN RSA PRIVATE KEY",
            "-----\nMIIEowI\n-----END RSA PRIVATE KEY-----",
        ]);
        for body in [
            format!("Key was {openai} when deployed.\n"),
            format!("AWS id {aws} in staging.\n"),
            pem.clone(),
        ] {
            let err = remember(
                &repo,
                &repo,
                &opts(
                    "Deploy keys",
                    "decision",
                    &format!("Deployment notes.\n{body}"),
                ),
            )
            .unwrap_err();
            assert!(err.to_string().to_lowercase().contains("secret"), "{err}");
            let msg = err.to_string();
            assert!(
                !msg.contains(&openai) && !msg.contains(&aws),
                "no secret echoed"
            );
        }
        assert!(!trove.join("memory/entries/deploy-keys.md").exists());
        assert_eq!(
            std::fs::read_to_string(trove.join("memory/INDEX.md")).unwrap(),
            index_before
        );
    }

    #[test]
    fn write_to_read_round_trip_via_trove_entries() {
        let repo = scratch("roundtrip");
        project_trove(&repo);
        remember(
            &repo,
            &repo,
            &opts("Use WAL mode", "decision", "Enable WAL for concurrency.\n"),
        )
        .unwrap();
        let docs = trove_entries(&repo.join(".claude/trove"), SourceKind::Project, &|p| {
            p.to_string_lossy().into_owned()
        });
        let doc = docs
            .iter()
            .find(|d| d.title == "Use WAL mode")
            .expect("reader sees the remembered entry");
        assert!(
            doc.path.ends_with("memory/entries/use-wal-mode.md"),
            "{}",
            doc.path
        );
    }

    #[test]
    fn routes_types_to_sections_and_honors_override() {
        let repo = scratch("route");
        let trove = project_trove(&repo);
        let r = remember(&repo, &repo, &opts("No ORM", "convention", "Use pgx.\n")).unwrap();
        assert_eq!(r.section, "conventions");
        let r = remember(
            &repo,
            &repo,
            &opts("Service map", "context", "The gateway owns auth.\n"),
        )
        .unwrap();
        assert_eq!(r.section, "context");
        let mut o = opts("Gotcha but filed", "gotcha", "Body.\n");
        o.section = Some("conventions".into());
        let r = remember(&repo, &repo, &o).unwrap();
        assert_eq!(r.section, "conventions");
        assert!(trove
            .join("conventions/entries/gotcha-but-filed.md")
            .exists());
        let err = remember(&repo, &repo, &opts("Bad type", "vibes", "Body.\n")).unwrap_err();
        assert!(err.to_string().contains("type"), "{err}");
    }

    #[test]
    fn writes_into_flat_user_trove() {
        let repo = scratch("flat");
        let home = repo.join("home");
        let trove = home.join(".claude/trove");
        write(
            &trove.join("INDEX.md"),
            "# Trove Index\n\n<!-- entries below -->\n",
        );
        std::fs::create_dir_all(trove.join("entries")).unwrap();
        let r = remember(
            &repo.join("code"),
            &home,
            &opts("Personal rule", "convention", "Always run tests.\n"),
        )
        .unwrap();
        assert_eq!(r.section, "");
        assert!(r.path.ends_with("entries/personal-rule.md"));
        let index = std::fs::read_to_string(trove.join("INDEX.md")).unwrap();
        assert!(index.contains("- [Personal rule](entries/personal-rule.md) - Always run tests."));
    }

    #[test]
    fn errors_when_no_trove_exists() {
        let repo = scratch("none");
        let err = remember(&repo, &repo, &opts("Nothing", "gotcha", "Body.\n")).unwrap_err();
        assert!(err.to_string().contains("no trove"), "{err}");
    }
}
