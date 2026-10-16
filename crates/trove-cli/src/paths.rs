use std::path::{Path, PathBuf};

pub fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Repo-relative inside the repo, `~/...` inside home, else absolute.
pub fn display_path(path: &Path, repo: &Path, home: &Path) -> String {
    if let Ok(rel) = path.strip_prefix(repo) {
        return rel.to_string_lossy().into_owned();
    }
    if let Ok(rel) = path.strip_prefix(home) {
        return format!("~/{}", rel.to_string_lossy());
    }
    path.to_string_lossy().into_owned()
}

/// `~/...` for paths inside home.
pub fn tilde(path: &Path) -> String {
    match path.strip_prefix(home_dir()) {
        Ok(rel) => format!("~/{}", rel.to_string_lossy()),
        Err(_) => path.to_string_lossy().into_owned(),
    }
}

/// Nearest ancestor holding `.trove` or `.git`, else `start` itself.
pub fn find_repo_root(start: &Path) -> PathBuf {
    let start = start.canonicalize().unwrap_or_else(|_| start.to_path_buf());
    start
        .ancestors()
        .find(|d| d.join(".trove").is_dir() || d.join(".git").exists())
        .map(Path::to_path_buf)
        .unwrap_or(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_prefers_repo_then_home() {
        let repo = Path::new("/home/u/code/app");
        let home = Path::new("/home/u");
        assert_eq!(
            display_path(Path::new("/home/u/code/app/src/main.rs"), repo, home),
            "src/main.rs"
        );
        assert_eq!(
            display_path(Path::new("/home/u/.claude/trove/entries/a.md"), repo, home),
            "~/.claude/trove/entries/a.md"
        );
        assert_eq!(
            display_path(Path::new("/usr/bin/git"), repo, home),
            "/usr/bin/git"
        );
    }
}
