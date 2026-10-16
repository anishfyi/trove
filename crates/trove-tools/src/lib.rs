//! Discover every command-line tool installed on macOS or Linux and keep a
//! cached, incrementally refreshed catalog with one-line summaries.

pub mod catalog;
pub mod dirs;
pub mod help;
pub mod man;
pub mod search;

pub use catalog::{Catalog, RefreshOptions, RefreshStats, SummaryOrigin, ToolEntry};
pub use dirs::{DiscoveryEnv, ToolSource};
pub use search::{search, ToolHit};

/// A fresh, unique scratch directory for a test.
#[cfg(test)]
pub(crate) fn test_dir(tag: &str) -> std::path::PathBuf {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static N: AtomicUsize = AtomicUsize::new(0);
    let dir = std::env::temp_dir().join(format!(
        "trove-tools-{tag}-{}-{}",
        std::process::id(),
        N.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}
