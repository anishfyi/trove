//! Trove entry layout shared by every Rust reader.
//!
//! Troves are written in two shapes: flat (`<trove>/entries/`) and sectioned
//! (`<trove>/{conventions,memory,context}/entries/`, created by
//! `trove.sh ensure_sections`). Readers must see both; this module is the one
//! place that knows the shapes.

use std::path::{Path, PathBuf};

/// Sectioned trove sections, in canonical order.
pub const SECTIONS: [&str; 3] = ["conventions", "memory", "context"];

/// Entry files of a trove dir: flat `entries/*` plus each section's
/// `<section>/entries/*`, `.md`/`.json` only, sorted, without duplicates.
/// Flat troves yield exactly the flat `entries/` files.
pub fn entry_files(trove_dir: &Path) -> Vec<PathBuf> {
    let mut dirs = vec![trove_dir.join("entries")];
    dirs.extend(SECTIONS.iter().map(|s| trove_dir.join(s).join("entries")));
    let mut files = Vec::new();
    for dir in dirs {
        let Ok(rd) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in rd.flatten() {
            let path = entry.path();
            if !matches!(
                path.extension().and_then(|e| e.to_str()),
                Some("md") | Some("json")
            ) {
                continue;
            }
            files.push(path);
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_flat_and_sectioned_entries_only() {
        let dir = std::env::temp_dir().join(format!("trove-layout-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        for sub in ["entries", "memory/entries", "context/entries"] {
            std::fs::create_dir_all(dir.join(sub)).unwrap();
        }
        std::fs::write(dir.join("entries/flat.md"), "flat").unwrap();
        std::fs::write(dir.join("entries/notes.txt"), "skip").unwrap();
        std::fs::write(dir.join("memory/entries/gotcha.md"), "md").unwrap();
        std::fs::write(dir.join("memory/entries/data.json"), "{}").unwrap();
        std::fs::write(dir.join("context/entries/context.json"), "{}").unwrap();
        std::fs::write(dir.join("memory/README.md"), "skip").unwrap();
        let files = entry_files(&dir);
        let rel: Vec<String> = files
            .iter()
            .map(|p| p.strip_prefix(&dir).unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            rel,
            vec![
                "context/entries/context.json",
                "entries/flat.md",
                "memory/entries/data.json",
                "memory/entries/gotcha.md",
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn missing_trove_is_empty() {
        assert!(entry_files(Path::new("/nonexistent-trove")).is_empty());
    }
}
