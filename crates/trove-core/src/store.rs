use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

use crate::execution::ExecutionMemory;
use crate::object::{MemoryKind, MemoryObject};
use crate::patch::PatchRecord;

pub const TROVE_DIR: &str = ".trove";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TroveManifest {
    pub version: String,
    pub repo_root: String,
    pub indexed_at: String,
    pub symbol_count: usize,
    pub module_count: usize,
    pub subsystem_count: usize,
}

impl Default for TroveManifest {
    fn default() -> Self {
        Self {
            version: "0.2.0".into(),
            repo_root: String::new(),
            indexed_at: String::new(),
            symbol_count: 0,
            module_count: 0,
            subsystem_count: 0,
        }
    }
}

pub struct TroveStore {
    root: PathBuf,
}

impl TroveStore {
    pub fn open(repo_root: impl AsRef<Path>) -> Result<Self> {
        let root = repo_root.as_ref().join(TROVE_DIR);
        fs::create_dir_all(&root)
            .with_context(|| format!("create {}", root.display()))?;
        Ok(Self { root })
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    pub fn write_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let path = self.root.join(name);
        let data = serde_json::to_string_pretty(value)?;
        fs::write(path, data)?;
        Ok(())
    }

    pub fn read_json<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        let path = self.root.join(name);
        if !path.exists() {
            return Ok(None);
        }
        let data = fs::read_to_string(path)?;
        Ok(Some(serde_json::from_str(&data)?))
    }

    pub fn write_objects(&self, kind: MemoryKind, objects: &[MemoryObject]) -> Result<()> {
        let subdir = self.subdir_for(kind);
        fs::create_dir_all(&subdir)?;
        for obj in objects {
            let path = subdir.join(format!("{}.json", sanitize_id(&obj.id)));
            let data = serde_json::to_string_pretty(obj)?;
            fs::write(path, data)?;
        }
        Ok(())
    }

    pub fn read_objects(&self, kind: MemoryKind) -> Result<Vec<MemoryObject>> {
        let subdir = self.subdir_for(kind);
        if !subdir.exists() {
            return Ok(Vec::new());
        }
        let mut out = Vec::new();
        for entry in fs::read_dir(subdir)? {
            let entry = entry?;
            if entry.path().extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }
            let data = fs::read_to_string(entry.path())?;
            out.push(serde_json::from_str(&data)?);
        }
        Ok(out)
    }

    pub fn write_manifest(&self, manifest: &TroveManifest) -> Result<()> {
        self.write_json("manifest.json", manifest)
    }

    pub fn read_manifest(&self) -> Result<Option<TroveManifest>> {
        self.read_json("manifest.json")
    }

    pub fn write_execution(&self, memory: &ExecutionMemory) -> Result<()> {
        self.write_json("execution.json", memory)
    }

    pub fn read_execution(&self) -> Result<ExecutionMemory> {
        Ok(self.read_json("execution.json")?.unwrap_or_default())
    }

    pub fn append_patch(&self, patch: &PatchRecord) -> Result<()> {
        let mut patches: Vec<PatchRecord> = self.read_json("patches.json")?.unwrap_or_default();
        patches.push(patch.clone());
        self.write_json("patches.json", &patches)
    }

    fn subdir_for(&self, kind: MemoryKind) -> PathBuf {
        let name = match kind {
            MemoryKind::Symbol => "symbols",
            MemoryKind::Module => "modules",
            MemoryKind::Subsystem => "subsystems",
            MemoryKind::Architecture => "architecture",
            MemoryKind::Historical => "historical",
            MemoryKind::Repository => "repository",
            MemoryKind::Working => "working",
            MemoryKind::Patch => "patches",
            MemoryKind::Execution => "execution",
        };
        self.root.join(name)
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

pub fn content_hash(content: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(content);
    format!("{:x}", digest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn store_roundtrip() {
        let dir = std::env::temp_dir().join("trove-test-store");
        let _ = fs::remove_dir_all(&dir);
        let store = TroveStore::open(&dir).unwrap();
        let manifest = TroveManifest {
            repo_root: dir.to_string_lossy().into(),
            indexed_at: "now".into(),
            symbol_count: 1,
            module_count: 1,
            subsystem_count: 1,
            ..Default::default()
        };
        store.write_manifest(&manifest).unwrap();
        let loaded = store.read_manifest().unwrap().unwrap();
        assert_eq!(loaded.symbol_count, 1);
        let _ = fs::remove_dir_all(&dir);
    }
}
