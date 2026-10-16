//! Where installed command-line tools live on macOS and Linux.

use std::collections::HashSet;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Where a tool was installed from, inferred from its directory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolSource {
    System,
    Homebrew,
    Cargo,
    LocalBin,
    Npm,
    Pnpm,
    Bun,
    Go,
}

impl ToolSource {
    pub fn label(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Homebrew => "brew",
            Self::Cargo => "cargo",
            Self::LocalBin => "local",
            Self::Npm => "npm",
            Self::Pnpm => "pnpm",
            Self::Bun => "bun",
            Self::Go => "go",
        }
    }
}

/// The environment discovery reads. Built from the process env in normal use,
/// constructed by hand in tests so results do not depend on the host.
#[derive(Debug, Clone, Default)]
pub struct DiscoveryEnv {
    pub path: Option<OsString>,
    pub home: Option<PathBuf>,
    pub homebrew_prefix: Option<PathBuf>,
    pub cargo_home: Option<PathBuf>,
    pub npm_prefix: Option<PathBuf>,
    pub pnpm_home: Option<PathBuf>,
    pub gopath: Option<PathBuf>,
    pub manpath: Option<OsString>,
    /// Include well-known absolute dirs such as /opt/homebrew/bin.
    pub include_system_defaults: bool,
}

impl DiscoveryEnv {
    pub fn from_process() -> Self {
        let var = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty());
        Self {
            path: var("PATH"),
            home: var("HOME").map(PathBuf::from),
            homebrew_prefix: var("HOMEBREW_PREFIX").map(PathBuf::from),
            cargo_home: var("CARGO_HOME").map(PathBuf::from),
            npm_prefix: var("NPM_CONFIG_PREFIX")
                .or_else(|| var("npm_config_prefix"))
                .map(PathBuf::from),
            pnpm_home: var("PNPM_HOME").map(PathBuf::from),
            gopath: var("GOPATH").map(PathBuf::from),
            manpath: var("MANPATH"),
            include_system_defaults: true,
        }
    }

    /// Every candidate bin directory in lookup order: `$PATH` first (so the
    /// first match shadows later ones, like the shell), then package manager
    /// locations that are often missing from non-login shells.
    pub fn candidate_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = Vec::new();
        if let Some(path) = &self.path {
            dirs.extend(std::env::split_paths(path).filter(|p| p.is_absolute()));
        }

        if let Some(prefix) = &self.homebrew_prefix {
            dirs.push(prefix.join("bin"));
            dirs.push(prefix.join("sbin"));
        }
        if self.include_system_defaults {
            for d in [
                "/opt/homebrew/bin",
                "/opt/homebrew/sbin",
                "/home/linuxbrew/.linuxbrew/bin",
                "/usr/local/bin",
                "/usr/local/sbin",
            ] {
                dirs.push(PathBuf::from(d));
            }
        }

        match (&self.cargo_home, &self.home) {
            (Some(c), _) => dirs.push(c.join("bin")),
            (None, Some(h)) => dirs.push(h.join(".cargo/bin")),
            _ => {}
        }
        if let Some(prefix) = &self.npm_prefix {
            dirs.push(prefix.join("bin"));
        }
        if let Some(p) = &self.pnpm_home {
            dirs.push(p.clone());
        }
        if let Some(g) = &self.gopath {
            dirs.push(g.join("bin"));
        }
        if let Some(home) = &self.home {
            for rel in [
                ".local/bin",
                ".npm-global/bin",
                ".npm-packages/bin",
                ".volta/bin",
                "Library/pnpm",
                ".local/share/pnpm",
                ".bun/bin",
                "go/bin",
            ] {
                dirs.push(home.join(rel));
            }
        }
        dirs
    }

    /// Candidate dirs that exist, deduplicated by canonical path.
    pub fn search_dirs(&self) -> Vec<PathBuf> {
        let mut seen = HashSet::new();
        self.candidate_dirs()
            .into_iter()
            .filter(|d| d.is_dir())
            .filter(|d| seen.insert(d.canonicalize().unwrap_or_else(|_| d.clone())))
            .collect()
    }

    /// Man page roots: `$MANPATH`, then `share/man` next to each bin dir,
    /// then the usual system locations.
    pub fn man_roots(&self, bin_dirs: &[PathBuf]) -> Vec<PathBuf> {
        let mut roots: Vec<PathBuf> = Vec::new();
        if let Some(mp) = &self.manpath {
            roots.extend(std::env::split_paths(mp).filter(|p| p.is_absolute()));
        }
        for dir in bin_dirs {
            if let Some(parent) = dir.parent() {
                roots.push(parent.join("share/man"));
                roots.push(parent.join("man"));
            }
        }
        if self.include_system_defaults {
            for d in [
                "/usr/share/man",
                "/usr/local/share/man",
                "/opt/homebrew/share/man",
                "/home/linuxbrew/.linuxbrew/share/man",
                "/Library/Developer/CommandLineTools/usr/share/man",
            ] {
                roots.push(PathBuf::from(d));
            }
        }
        let mut seen = HashSet::new();
        roots
            .into_iter()
            .filter(|d| d.is_dir())
            .filter(|d| seen.insert(d.canonicalize().unwrap_or_else(|_| d.clone())))
            .collect()
    }
}

/// Infer the install source from a bin dir and, for symlinks, the resolved
/// target (Homebrew links `/usr/local/bin/jq` into its Cellar).
pub fn classify(dir: &Path, target: Option<&Path>) -> ToolSource {
    let by_dir = classify_path(dir);
    if by_dir != ToolSource::System {
        return by_dir;
    }
    target.map(classify_path).unwrap_or(ToolSource::System)
}

fn classify_path(path: &Path) -> ToolSource {
    let s = path.to_string_lossy();
    if s.contains("/homebrew/") || s.contains("linuxbrew") || s.contains("/Cellar/") {
        ToolSource::Homebrew
    } else if s.contains("/.cargo/") || s.contains("/cargo/bin") {
        ToolSource::Cargo
    } else if s.contains("pnpm") {
        ToolSource::Pnpm
    } else if s.contains("/.bun/") {
        ToolSource::Bun
    } else if s.contains("npm")
        || s.contains("/.nvm/")
        || s.contains("/.volta/")
        || s.contains("/node_modules/")
        || s.contains("/fnm/")
    {
        ToolSource::Npm
    } else if s.ends_with("/go/bin") || s.contains("/go/bin/") {
        ToolSource::Go
    } else if s.contains("/.local/bin") {
        ToolSource::LocalBin
    } else {
        ToolSource::System
    }
}

/// Cache directory for the tool catalog. `TROVE_CACHE_DIR` overrides.
pub fn cache_dir() -> PathBuf {
    if let Some(d) = std::env::var_os("TROVE_CACHE_DIR").filter(|v| !v.is_empty()) {
        return PathBuf::from(d);
    }
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    if cfg!(target_os = "macos") {
        return home.join("Library/Caches/trove");
    }
    std::env::var_os("XDG_CACHE_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| home.join(".cache"))
        .join("trove")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_comes_first_then_package_managers() {
        let env = DiscoveryEnv {
            path: Some(OsString::from("/usr/bin:/bin:relative/skip")),
            home: Some(PathBuf::from("/home/u")),
            pnpm_home: Some(PathBuf::from("/home/u/.pnpm")),
            ..Default::default()
        };
        let dirs = env.candidate_dirs();
        assert_eq!(dirs[0], PathBuf::from("/usr/bin"));
        assert_eq!(dirs[1], PathBuf::from("/bin"));
        assert!(!dirs.iter().any(|d| d.ends_with("relative/skip")));
        assert!(dirs.contains(&PathBuf::from("/home/u/.cargo/bin")));
        assert!(dirs.contains(&PathBuf::from("/home/u/.local/bin")));
        assert!(dirs.contains(&PathBuf::from("/home/u/.npm-global/bin")));
        assert!(dirs.contains(&PathBuf::from("/home/u/.pnpm")));
        assert!(!dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
    }

    #[test]
    fn cargo_home_overrides_default() {
        let env = DiscoveryEnv {
            home: Some(PathBuf::from("/home/u")),
            cargo_home: Some(PathBuf::from("/opt/cargo")),
            ..Default::default()
        };
        let dirs = env.candidate_dirs();
        assert!(dirs.contains(&PathBuf::from("/opt/cargo/bin")));
        assert!(!dirs.contains(&PathBuf::from("/home/u/.cargo/bin")));
    }

    #[test]
    fn classifies_sources() {
        let c = |d: &str| classify(Path::new(d), None);
        assert_eq!(c("/opt/homebrew/bin"), ToolSource::Homebrew);
        assert_eq!(c("/home/linuxbrew/.linuxbrew/bin"), ToolSource::Homebrew);
        assert_eq!(c("/Users/a/.cargo/bin"), ToolSource::Cargo);
        assert_eq!(c("/home/a/.local/bin"), ToolSource::LocalBin);
        assert_eq!(c("/home/a/.npm-global/bin"), ToolSource::Npm);
        assert_eq!(c("/home/a/.nvm/versions/node/v22.1.0/bin"), ToolSource::Npm);
        assert_eq!(c("/Users/a/Library/pnpm"), ToolSource::Pnpm);
        assert_eq!(c("/home/a/.bun/bin"), ToolSource::Bun);
        assert_eq!(c("/home/a/go/bin"), ToolSource::Go);
        assert_eq!(c("/usr/bin"), ToolSource::System);
        assert_eq!(
            classify(
                Path::new("/usr/local/bin"),
                Some(Path::new("/usr/local/Cellar/jq/1.7/bin/jq"))
            ),
            ToolSource::Homebrew
        );
    }
}
