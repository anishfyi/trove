//! The cached catalog of installed tools and its incremental refresh.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::dirs::{cache_dir, classify, DiscoveryEnv, ToolSource};
use crate::help::{parse_help_summary, probe_help};
use crate::man::ManIndex;

const CATALOG_VERSION: u32 = 1;
const CATALOG_FILE: &str = "tools.json";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SummaryOrigin {
    Man,
    Help,
}

/// One executable found on disk.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolEntry {
    pub name: String,
    pub path: String,
    pub dir: String,
    pub source: ToolSource,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub summary_from: Option<SummaryOrigin>,
    /// `--help` has been tried for this binary.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub probed: bool,
    pub mtime: u64,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirStamp {
    pub dir: String,
    pub mtime: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    /// Unix seconds of the last refresh that changed anything.
    pub refreshed_at: u64,
    pub dirs: Vec<DirStamp>,
    /// Every executable in lookup order, including shadowed duplicates.
    pub entries: Vec<ToolEntry>,
}

#[derive(Debug, Clone)]
pub struct RefreshOptions {
    /// Ignore the cache and rescan every directory.
    pub full: bool,
    /// Run `--help` (guarded, with a timeout) for tools lacking a man summary.
    pub probe: bool,
    pub probe_timeout: Duration,
    pub probe_threads: usize,
}

impl Default for RefreshOptions {
    fn default() -> Self {
        Self {
            full: false,
            probe: false,
            probe_timeout: Duration::from_millis(1500),
            probe_threads: std::thread::available_parallelism()
                .map(|n| (n.get() * 2).min(32))
                .unwrap_or(8),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize)]
pub struct RefreshStats {
    pub dirs: usize,
    pub dirs_rescanned: usize,
    pub tools: usize,
    pub new_entries: usize,
    pub removed_entries: usize,
    pub man_summaries: usize,
    pub help_probed: usize,
    pub changed: bool,
}

impl Catalog {
    pub fn empty() -> Self {
        Self {
            version: CATALOG_VERSION,
            refreshed_at: 0,
            dirs: Vec::new(),
            entries: Vec::new(),
        }
    }

    pub fn default_path() -> PathBuf {
        cache_dir().join(CATALOG_FILE)
    }

    pub fn load(path: &Path) -> Option<Self> {
        let data = std::fs::read_to_string(path).ok()?;
        let cat: Self = serde_json::from_str(&data).ok()?;
        (cat.version == CATALOG_VERSION).then_some(cat)
    }

    pub fn save(&self, path: &Path) -> Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create {}", parent.display()))?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec(self)?)?;
        std::fs::rename(&tmp, path)?;
        Ok(())
    }

    /// Load the cached catalog, refresh it incrementally, save if changed.
    pub fn open(env: &DiscoveryEnv, opts: &RefreshOptions) -> Result<(Self, RefreshStats)> {
        Self::open_at(&Self::default_path(), env, opts)
    }

    pub fn open_at(
        path: &Path,
        env: &DiscoveryEnv,
        opts: &RefreshOptions,
    ) -> Result<(Self, RefreshStats)> {
        let prev = Self::load(path);
        let (cat, stats) = Self::refresh(prev.as_ref(), env, opts);
        if stats.changed || prev.is_none() {
            cat.save(path)?;
        }
        Ok((cat, stats))
    }

    /// The tools a shell would run: first entry per name in lookup order,
    /// sorted by name.
    pub fn tools(&self) -> Vec<&ToolEntry> {
        let mut seen = HashSet::new();
        let mut out: Vec<&ToolEntry> = self
            .entries
            .iter()
            .filter(|e| seen.insert(e.name.as_str()))
            .collect();
        out.sort_by(|a, b| a.name.cmp(&b.name));
        out
    }

    pub fn find(&self, name: &str) -> Option<&ToolEntry> {
        self.entries.iter().find(|e| e.name == name)
    }

    /// Shadowed copies of a tool later in the lookup order.
    pub fn shadowed(&self, name: &str) -> Vec<&ToolEntry> {
        self.entries
            .iter()
            .filter(|e| e.name == name)
            .skip(1)
            .collect()
    }

    /// Record a summary found later (for example by `trove tools <name>`).
    pub fn set_summary(&mut self, name: &str, summary: Option<String>, origin: SummaryOrigin) {
        for e in self.entries.iter_mut().filter(|e| e.name == name) {
            if origin == SummaryOrigin::Help {
                e.probed = true;
            }
            if e.summary.is_none() && summary.is_some() {
                e.summary = summary.clone();
                e.summary_from = Some(origin);
            }
        }
    }

    /// Bring `prev` up to date with the filesystem. Directories whose mtime
    /// is unchanged are reused wholesale; in changed directories, binaries
    /// whose mtime and size are unchanged keep their cached summary.
    pub fn refresh(
        prev: Option<&Catalog>,
        env: &DiscoveryEnv,
        opts: &RefreshOptions,
    ) -> (Catalog, RefreshStats) {
        let dirs = env.search_dirs();
        let empty = Catalog::empty();
        let prev = if opts.full {
            &empty
        } else {
            prev.unwrap_or(&empty)
        };
        let prev_dir_mtime: HashMap<&str, u64> = prev
            .dirs
            .iter()
            .map(|d| (d.dir.as_str(), d.mtime))
            .collect();
        let prev_by_path: HashMap<&str, &ToolEntry> =
            prev.entries.iter().map(|e| (e.path.as_str(), e)).collect();

        let mut stats = RefreshStats {
            dirs: dirs.len(),
            ..Default::default()
        };
        let mut stamps = Vec::with_capacity(dirs.len());
        let mut entries: Vec<ToolEntry> = Vec::new();
        let mut fresh: Vec<usize> = Vec::new();

        for dir in &dirs {
            let dir_str = dir.to_string_lossy().into_owned();
            let mtime = std::fs::metadata(dir)
                .ok()
                .and_then(|m| m.modified().ok())
                .map(nanos)
                .unwrap_or(0);
            stamps.push(DirStamp {
                dir: dir_str.clone(),
                mtime,
            });

            if prev_dir_mtime.get(dir_str.as_str()) == Some(&mtime) {
                entries.extend(prev.entries.iter().filter(|e| e.dir == dir_str).cloned());
                continue;
            }
            stats.dirs_rescanned += 1;
            for found in scan_dir(dir) {
                let path = found.path.to_string_lossy().into_owned();
                match prev_by_path.get(path.as_str()) {
                    Some(old) if old.mtime == found.mtime && old.size == found.size => {
                        entries.push((*old).clone());
                    }
                    _ => {
                        fresh.push(entries.len());
                        entries.push(ToolEntry {
                            source: classify(dir, found.target.as_deref()),
                            name: found.name,
                            path,
                            dir: dir_str.clone(),
                            summary: None,
                            summary_from: None,
                            probed: false,
                            mtime: found.mtime,
                            size: found.size,
                        });
                    }
                }
            }
        }

        stats.new_entries = fresh.len();
        let kept: HashSet<&str> = entries.iter().map(|e| e.path.as_str()).collect();
        stats.removed_entries = prev
            .entries
            .iter()
            .filter(|e| !kept.contains(e.path.as_str()))
            .count();

        if !fresh.is_empty() {
            let man = ManIndex::build(&env.man_roots(&dirs));
            let mut memo: HashMap<String, Option<String>> = HashMap::new();
            for &i in &fresh {
                let name = entries[i].name.clone();
                let summary = memo
                    .entry(name.clone())
                    .or_insert_with(|| man.summary(&name))
                    .clone();
                if summary.is_some() {
                    stats.man_summaries += 1;
                    entries[i].summary = summary;
                    entries[i].summary_from = Some(SummaryOrigin::Man);
                }
            }
        }

        if opts.probe {
            stats.help_probed = probe_missing(&mut entries, opts);
        }

        let mut cat = Catalog {
            version: CATALOG_VERSION,
            refreshed_at: prev.refreshed_at,
            dirs: stamps,
            entries,
        };
        stats.tools = cat.tools().len();
        stats.changed = stats.new_entries > 0
            || stats.removed_entries > 0
            || stats.help_probed > 0
            || cat.dirs != prev.dirs;
        if stats.changed {
            cat.refreshed_at = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
        }
        (cat, stats)
    }
}

/// Probe `--help` for each winning entry without a summary, in parallel.
fn probe_missing(entries: &mut [ToolEntry], opts: &RefreshOptions) -> usize {
    let mut seen = HashSet::new();
    let targets: Vec<(usize, PathBuf, String)> = entries
        .iter()
        .enumerate()
        .filter(|(_, e)| seen.insert(e.name.clone()))
        .filter(|(_, e)| e.summary.is_none() && !e.probed)
        .map(|(i, e)| (i, PathBuf::from(&e.path), e.name.clone()))
        .collect();
    if targets.is_empty() {
        return 0;
    }
    let threads = opts.probe_threads.max(1);
    let chunk = targets.len().div_ceil(threads);
    let results: Vec<(usize, Option<String>)> = std::thread::scope(|s| {
        let handles: Vec<_> = targets
            .chunks(chunk)
            .map(|batch| {
                s.spawn(move || {
                    batch
                        .iter()
                        .map(|(i, path, name)| {
                            let summary = probe_help(path, opts.probe_timeout)
                                .and_then(|h| parse_help_summary(&h, name));
                            (*i, summary)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap_or_default())
            .collect()
    });
    let probed = results.len();
    for (i, summary) in results {
        let e = &mut entries[i];
        e.probed = true;
        if summary.is_some() {
            e.summary = summary;
            e.summary_from = Some(SummaryOrigin::Help);
        }
    }
    probed
}

struct Found {
    name: String,
    path: PathBuf,
    target: Option<PathBuf>,
    mtime: u64,
    size: u64,
}

fn scan_dir(dir: &Path) -> Vec<Found> {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut out: Vec<Found> = rd
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let path = entry.path();
            // Follows symlinks: a dangling link is skipped.
            let meta = std::fs::metadata(&path).ok()?;
            if !meta.is_file() || !is_executable(&meta) {
                return None;
            }
            let target = entry
                .file_type()
                .ok()
                .filter(|t| t.is_symlink())
                .and_then(|_| std::fs::canonicalize(&path).ok());
            Some(Found {
                name,
                path,
                target,
                mtime: meta.modified().map(nanos).unwrap_or(0),
                size: meta.len(),
            })
        })
        .collect();
    out.sort_by(|a, b| a.name.cmp(&b.name));
    out
}

#[cfg(unix)]
fn is_executable(meta: &std::fs::Metadata) -> bool {
    use std::os::unix::fs::PermissionsExt;
    meta.permissions().mode() & 0o111 != 0
}

#[cfg(not(unix))]
fn is_executable(_meta: &std::fs::Metadata) -> bool {
    true
}

fn nanos(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(0)
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn make_exe(path: &Path, body: &str) {
        std::fs::write(path, body).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn fixture(tag: &str) -> (PathBuf, DiscoveryEnv) {
        let root = crate::test_dir(tag);
        let bin_a = root.join("a/bin");
        let bin_b = root.join("home/.cargo/bin");
        let man1 = root.join("a/share/man/man1");
        for d in [&bin_a, &bin_b, &man1] {
            std::fs::create_dir_all(d).unwrap();
        }
        make_exe(&bin_a.join("alpha"), "#!/bin/sh\n");
        make_exe(&bin_a.join("dup"), "#!/bin/sh\n");
        std::fs::write(bin_a.join("notexec"), "data").unwrap();
        make_exe(&bin_a.join(".hidden"), "#!/bin/sh\n");
        make_exe(
            &bin_b.join("beta"),
            "#!/bin/sh\necho 'Beta does beta things for tests.'\n",
        );
        make_exe(&bin_b.join("dup"), "#!/bin/sh\n");
        std::fs::write(
            man1.join("alpha.1"),
            ".SH NAME\nalpha \\- first letter tool\n",
        )
        .unwrap();
        let env = DiscoveryEnv {
            path: Some(bin_a.clone().into_os_string()),
            home: Some(root.join("home")),
            ..Default::default()
        };
        (root, env)
    }

    #[test]
    fn scans_executables_with_shadowing_and_man_summaries() {
        let (root, env) = fixture("catalog-scan");
        let (cat, stats) = Catalog::refresh(None, &env, &RefreshOptions::default());
        let names: Vec<&str> = cat.tools().iter().map(|t| t.name.as_str()).collect();
        assert_eq!(names, ["alpha", "beta", "dup"]);
        assert_eq!(stats.tools, 3);
        assert_eq!(stats.new_entries, 4);
        assert!(stats.changed);

        let alpha = cat.find("alpha").unwrap();
        assert_eq!(alpha.summary.as_deref(), Some("first letter tool"));
        assert_eq!(alpha.summary_from, Some(SummaryOrigin::Man));
        assert_eq!(cat.find("beta").unwrap().source, ToolSource::Cargo);
        assert!(cat.find("dup").unwrap().path.contains("/a/bin/"));
        assert_eq!(cat.shadowed("dup").len(), 1);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn incremental_refresh_reuses_and_detects_changes() {
        let (root, env) = fixture("catalog-incr");
        let opts = RefreshOptions::default();
        let (first, _) = Catalog::refresh(None, &env, &opts);

        let (second, stats) = Catalog::refresh(Some(&first), &env, &opts);
        assert!(!stats.changed);
        assert_eq!(stats.dirs_rescanned, 0);
        assert_eq!(second.entries, first.entries);

        // Mtime granularity can be coarse; make sure the dir stamp moves.
        std::thread::sleep(Duration::from_millis(20));
        make_exe(&root.join("a/bin/gamma"), "#!/bin/sh\n");
        std::fs::remove_file(root.join("a/bin/dup")).unwrap();
        let (third, stats) = Catalog::refresh(Some(&second), &env, &opts);
        if third.dirs == second.dirs {
            // Filesystem with second-level mtimes inside one second.
            let _ = std::fs::remove_dir_all(&root);
            return;
        }
        assert!(stats.changed);
        assert_eq!(stats.dirs_rescanned, 1);
        assert_eq!(stats.new_entries, 1);
        assert_eq!(stats.removed_entries, 1);
        assert!(third.find("gamma").is_some());
        // dup now resolves to the cargo copy.
        assert!(third.find("dup").unwrap().path.contains(".cargo"));
        // alpha kept its cached summary without a rescan of its man page.
        assert_eq!(
            third.find("alpha").unwrap().summary.as_deref(),
            Some("first letter tool")
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn probe_fills_missing_summaries_and_cache_roundtrips() {
        let (root, env) = fixture("catalog-probe");
        let opts = RefreshOptions {
            probe: true,
            probe_timeout: Duration::from_secs(5),
            ..Default::default()
        };
        let path = root.join("cache/tools.json");
        let (cat, stats) = Catalog::open_at(&path, &env, &opts).unwrap();
        assert!(stats.help_probed >= 1);
        let beta = cat.find("beta").unwrap();
        assert_eq!(
            beta.summary.as_deref(),
            Some("Beta does beta things for tests.")
        );
        assert_eq!(beta.summary_from, Some(SummaryOrigin::Help));

        let loaded = Catalog::load(&path).unwrap();
        assert_eq!(loaded.entries, cat.entries);
        // Already probed: nothing to do on the next open.
        let (_, stats) = Catalog::open_at(&path, &env, &opts).unwrap();
        assert_eq!(stats.help_probed, 0);
        assert!(!stats.changed);
        let _ = std::fs::remove_dir_all(&root);
    }
}
