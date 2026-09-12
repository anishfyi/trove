use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};
use chrono::{DateTime, Utc};
use rusqlite::{
    params, Connection, ErrorCode, OptionalExtension, Transaction, TransactionBehavior,
};
use serde::{Deserialize, Serialize};

use crate::execution::{ExecutionEvent, ExecutionMemory};
use crate::object::{MemoryKind, MemoryObject};
use crate::patch::PatchRecord;
use crate::reflection::{
    EventSubmission, MemoryRecord, NewRecord, Provenance, RecordKind, RecordUpdate, ReflectionEvent,
};

pub const TROVE_DIR: &str = ".trove";
const DB_FILE: &str = "store.db";
const SCHEMA_VERSION: i32 = 1;
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_BUSY_RETRIES: usize = 8;
const EXECUTION_EVENT_LIMIT: i64 = 500;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("record {record_id} revision conflict: expected {expected}, found {actual}")]
    RevisionConflict {
        record_id: String,
        expected: u64,
        actual: u64,
    },
    #[error("record not found: {0}")]
    RecordNotFound(String),
    #[error("record already exists: {0}")]
    RecordExists(String),
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("corrupt stored data: {0}")]
    CorruptData(String),
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

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

/// Counts from `migrate_from_json`; `errors` lists legacy files that could not
/// be parsed and were skipped (the files themselves are left untouched).
#[derive(Debug, Clone, Default)]
pub struct MigrationReport {
    pub objects: usize,
    pub kv_entries: usize,
    pub patches: usize,
    pub execution_events: usize,
    pub errors: Vec<String>,
}

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS meta (
    key TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS kv (
    name TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS objects (
    kind TEXT NOT NULL,
    id TEXT NOT NULL,
    data TEXT NOT NULL,
    PRIMARY KEY (kind, id)
);
CREATE TABLE IF NOT EXISTS events (
    event_id TEXT PRIMARY KEY,
    submission_id TEXT NOT NULL UNIQUE,
    kind TEXT NOT NULL CHECK (kind IN ('observation', 'hypothesis', 'decision')),
    body TEXT NOT NULL,
    supersedes TEXT REFERENCES events(event_id),
    project TEXT NOT NULL,
    agent TEXT NOT NULL,
    run_id TEXT NOT NULL,
    source_refs TEXT NOT NULL DEFAULT '[]',
    source_hash TEXT,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS records (
    record_id TEXT PRIMARY KEY,
    revision INTEGER NOT NULL CHECK (revision > 0),
    kind TEXT NOT NULL CHECK (kind IN ('observation', 'hypothesis', 'decision')),
    body TEXT NOT NULL,
    project TEXT NOT NULL,
    agent TEXT NOT NULL,
    run_id TEXT NOT NULL,
    source_refs TEXT NOT NULL DEFAULT '[]',
    source_hash TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS patches (
    id TEXT PRIMARY KEY,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS execution_events (
    seq INTEGER PRIMARY KEY AUTOINCREMENT,
    data TEXT NOT NULL,
    created_at TEXT NOT NULL
);
";

/// Durable store in `.trove/store.db` (SQLite, WAL mode). `TroveStore` is a
/// lightweight handle: every operation opens its own connection, so clones and
/// copies across threads (or separate processes) get true concurrent readers
/// with a single writer, and writers serialize on bounded busy-retries.
#[derive(Clone)]
pub struct TroveStore {
    root: PathBuf,
    db_path: PathBuf,
}

impl TroveStore {
    pub fn open(repo_root: impl AsRef<Path>) -> Result<Self> {
        let root = repo_root.as_ref().join(TROVE_DIR);
        fs::create_dir_all(&root).with_context(|| format!("create {}", root.display()))?;
        let db_path = root.join(DB_FILE);
        {
            let conn = Connection::open(&db_path)
                .with_context(|| format!("open {}", db_path.display()))?;
            init_db(&conn)?;
        }
        let store = Self { root, db_path };
        store.migrate_legacy_json()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.root
    }

    fn connect(&self) -> Result<Connection, StoreError> {
        let conn = Connection::open(&self.db_path)?;
        conn.pragma_update(None, "synchronous", "FULL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.busy_timeout(BUSY_TIMEOUT)?;
        Ok(conn)
    }

    /// Single-statement reads and writes with bounded retry on busy/locked.
    fn conn_op<R>(
        &self,
        f: impl Fn(&Connection) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let mut attempts = 0usize;
        loop {
            let conn = self.connect()?;
            match f(&conn) {
                Err(e) if is_busy(&e) && attempts < MAX_BUSY_RETRIES => {
                    attempts += 1;
                    thread::sleep(Duration::from_millis(25 * attempts as u64));
                }
                other => return other,
            }
        }
    }

    /// One short immediate transaction with bounded retry. `f` may rerun, so
    /// it must be written against a clean transaction state.
    fn write_tx<R>(
        &self,
        f: impl Fn(&Transaction) -> Result<R, StoreError>,
    ) -> Result<R, StoreError> {
        let mut attempts = 0usize;
        loop {
            let mut conn = self.connect()?;
            let result = (|| {
                let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
                let out = f(&tx)?;
                tx.commit()?;
                Ok(out)
            })();
            match result {
                Err(e) if is_busy(&e) && attempts < MAX_BUSY_RETRIES => {
                    attempts += 1;
                    thread::sleep(Duration::from_millis(25 * attempts as u64));
                }
                other => return other,
            }
        }
    }

    pub fn write_json<T: Serialize>(&self, name: &str, value: &T) -> Result<()> {
        let data = serde_json::to_string_pretty(value)?;
        self.conn_op(|conn| {
            conn.execute(
                "INSERT INTO kv (name, value) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET value = excluded.value",
                params![name, data],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn read_json<T: for<'de> Deserialize<'de>>(&self, name: &str) -> Result<Option<T>> {
        self.conn_op(|conn| {
            let data: Option<String> = conn
                .query_row("SELECT value FROM kv WHERE name = ?1", params![name], |r| {
                    r.get(0)
                })
                .optional()?;
            data.map(|d| serde_json::from_str(&d).map_err(StoreError::from))
                .transpose()
        })
        .map_err(anyhow::Error::from)
    }

    /// Replace all objects of `kind` with `objects` in one transaction.
    pub fn write_objects(&self, kind: MemoryKind, objects: &[MemoryObject]) -> Result<()> {
        let rows = objects
            .iter()
            .map(|o| Ok((o.id.clone(), serde_json::to_string(o)?)))
            .collect::<Result<Vec<_>>>()?;
        self.write_tx(|tx| {
            tx.execute(
                "DELETE FROM objects WHERE kind = ?1",
                params![kind.as_str()],
            )?;
            for (id, data) in &rows {
                tx.execute(
                    "INSERT INTO objects (kind, id, data) VALUES (?1, ?2, ?3)",
                    params![kind.as_str(), id, data],
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Merge `objects` into the store keyed by each object's own kind and id,
    /// without deleting other objects of the same kind.
    pub fn upsert_objects(&self, objects: &[MemoryObject]) -> Result<()> {
        let rows = objects
            .iter()
            .map(|o| Ok((o.kind.as_str(), o.id.clone(), serde_json::to_string(o)?)))
            .collect::<Result<Vec<_>>>()?;
        self.write_tx(|tx| {
            for (kind, id, data) in &rows {
                tx.execute(
                    "INSERT INTO objects (kind, id, data) VALUES (?1, ?2, ?3)
                     ON CONFLICT(kind, id) DO UPDATE SET data = excluded.data",
                    params![kind, id, data],
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    pub fn read_objects(&self, kind: MemoryKind) -> Result<Vec<MemoryObject>> {
        self.conn_op(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM objects WHERE kind = ?1 ORDER BY id")?;
            let rows = stmt.query_map(params![kind.as_str()], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(serde_json::from_str(&row?)?);
            }
            Ok(out)
        })
        .map_err(anyhow::Error::from)
    }

    pub fn write_manifest(&self, manifest: &TroveManifest) -> Result<()> {
        self.write_json("manifest.json", manifest)
    }

    pub fn read_manifest(&self) -> Result<Option<TroveManifest>> {
        self.read_json("manifest.json")
    }

    /// Replace the execution event log with `memory`'s events.
    pub fn write_execution(&self, memory: &ExecutionMemory) -> Result<()> {
        let rows = memory
            .events
            .iter()
            .map(serde_json::to_string)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        self.write_tx(|tx| {
            tx.execute("DELETE FROM execution_events", [])?;
            for data in &rows {
                tx.execute(
                    "INSERT INTO execution_events (data, created_at) VALUES (?1, ?2)",
                    params![data, Utc::now()],
                )?;
            }
            Ok(())
        })?;
        Ok(())
    }

    /// Append one execution event without losing concurrently appended events.
    /// The log is trimmed to the most recent `EXECUTION_EVENT_LIMIT` entries,
    /// matching `ExecutionMemory::record`.
    pub fn record_execution(&self, event: &ExecutionEvent) -> Result<()> {
        let data = serde_json::to_string(event)?;
        self.write_tx(|tx| {
            tx.execute(
                "INSERT INTO execution_events (data, created_at) VALUES (?1, ?2)",
                params![data, Utc::now()],
            )?;
            tx.execute(
                "DELETE FROM execution_events
                 WHERE seq <= (SELECT MAX(seq) FROM execution_events) - ?1",
                params![EXECUTION_EVENT_LIMIT],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn read_execution(&self) -> Result<ExecutionMemory> {
        self.conn_op(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM execution_events ORDER BY seq")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut events = Vec::new();
            for row in rows {
                events.push(serde_json::from_str(&row?)?);
            }
            Ok(ExecutionMemory { events })
        })
        .map_err(anyhow::Error::from)
    }

    pub fn append_patch(&self, patch: &PatchRecord) -> Result<()> {
        let data = serde_json::to_string(patch)?;
        self.conn_op(|conn| {
            conn.execute(
                "INSERT INTO patches (id, data, created_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(id) DO UPDATE SET data = excluded.data",
                params![patch.id, data, Utc::now()],
            )?;
            Ok(())
        })?;
        Ok(())
    }

    pub fn read_patches(&self) -> Result<Vec<PatchRecord>> {
        self.conn_op(|conn| {
            let mut stmt = conn.prepare("SELECT data FROM patches ORDER BY created_at, id")?;
            let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(serde_json::from_str(&row?)?);
            }
            Ok(out)
        })
        .map_err(anyhow::Error::from)
    }

    /// Append an immutable reflection event. Retried submissions return the
    /// already-stored event: `submission_id` is a uniqueness constraint, so an
    /// agent that retries can never double-write.
    pub fn submit_event(
        &self,
        submission: &EventSubmission,
    ) -> Result<ReflectionEvent, StoreError> {
        if submission.submission_id.is_empty() {
            return Err(StoreError::InvalidInput(
                "submission_id must not be empty".into(),
            ));
        }
        let event_id = format!("evt:{}", submission.submission_id);
        let source_refs = serde_json::to_string(&submission.provenance.source_refs)?;
        self.conn_op(|conn| {
            conn.execute(
                "INSERT INTO events (event_id, submission_id, kind, body, supersedes,
                                     project, agent, run_id, source_refs, source_hash, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
                 ON CONFLICT(submission_id) DO NOTHING",
                params![
                    event_id,
                    submission.submission_id,
                    submission.kind.as_str(),
                    submission.body,
                    submission.supersedes,
                    submission.provenance.project,
                    submission.provenance.agent,
                    submission.provenance.run_id,
                    source_refs,
                    submission.provenance.source_hash,
                    Utc::now()
                ],
            )?;
            event_by_submission(conn, &submission.submission_id)?
                .ok_or_else(|| StoreError::CorruptData("event vanished after insert".into()))
        })
    }

    /// All reflection events in insertion order, oldest first.
    pub fn read_events(&self) -> Result<Vec<ReflectionEvent>> {
        self.conn_op(|conn| {
            let mut stmt = conn.prepare(&format!("{EVENT_SELECT} ORDER BY rowid"))?;
            let rows = stmt.query_map([], EventRow::from_row)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?.into_event()?);
            }
            Ok(out)
        })
        .map_err(anyhow::Error::from)
    }

    /// Create a mutable record at revision 1. Fails with
    /// `StoreError::RecordExists` if `record_id` is taken.
    pub fn create_record(&self, record: &NewRecord) -> Result<MemoryRecord, StoreError> {
        if record.record_id.is_empty() {
            return Err(StoreError::InvalidInput(
                "record_id must not be empty".into(),
            ));
        }
        let source_refs = serde_json::to_string(&record.provenance.source_refs)?;
        let now = Utc::now();
        self.conn_op(|conn| {
            let result = conn.execute(
                "INSERT INTO records (record_id, revision, kind, body, project, agent,
                                      run_id, source_refs, source_hash, created_at, updated_at)
                 VALUES (?1, 1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?9)",
                params![
                    record.record_id,
                    record.kind.as_str(),
                    record.body,
                    record.provenance.project,
                    record.provenance.agent,
                    record.provenance.run_id,
                    source_refs,
                    record.provenance.source_hash,
                    now
                ],
            );
            match result {
                Ok(_) => Ok(MemoryRecord {
                    record_id: record.record_id.clone(),
                    revision: 1,
                    kind: record.kind,
                    body: record.body.clone(),
                    provenance: record.provenance.clone(),
                    created_at: now,
                    updated_at: now,
                }),
                Err(e) if e.sqlite_error_code() == Some(ErrorCode::ConstraintViolation) => {
                    Err(StoreError::RecordExists(record.record_id.clone()))
                }
                Err(e) => Err(e.into()),
            }
        })
    }

    /// Update a record only if its current revision is `expected_revision`;
    /// bumps the revision by one. Never last-writer-wins: a mismatch is
    /// rejected with `StoreError::RevisionConflict`.
    pub fn update_record(
        &self,
        record_id: &str,
        expected_revision: u64,
        update: &RecordUpdate,
    ) -> Result<MemoryRecord, StoreError> {
        if record_id.is_empty() {
            return Err(StoreError::InvalidInput(
                "record_id must not be empty".into(),
            ));
        }
        let source_refs = serde_json::to_string(&update.provenance.source_refs)?;
        let now = Utc::now();
        self.write_tx(|tx| {
            let changed = tx.execute(
                "UPDATE records SET revision = revision + 1, kind = ?2, body = ?3,
                                     project = ?4, agent = ?5, run_id = ?6,
                                     source_refs = ?7, source_hash = ?8, updated_at = ?9
                 WHERE record_id = ?1 AND revision = ?10",
                params![
                    record_id,
                    update.kind.as_str(),
                    update.body,
                    update.provenance.project,
                    update.provenance.agent,
                    update.provenance.run_id,
                    source_refs,
                    update.provenance.source_hash,
                    now,
                    expected_revision as i64
                ],
            )?;
            if changed == 1 {
                return record_by_id(tx, record_id)?
                    .ok_or_else(|| StoreError::CorruptData("record vanished after update".into()));
            }
            match record_by_id(tx, record_id)? {
                Some(existing) => Err(StoreError::RevisionConflict {
                    record_id: record_id.into(),
                    expected: expected_revision,
                    actual: existing.revision,
                }),
                None => Err(StoreError::RecordNotFound(record_id.into())),
            }
        })
    }

    pub fn read_record(&self, record_id: &str) -> Result<Option<MemoryRecord>> {
        self.conn_op(|conn| record_by_id(conn, record_id))
            .map_err(anyhow::Error::from)
    }

    pub fn read_records(&self) -> Result<Vec<MemoryRecord>> {
        self.conn_op(|conn| {
            let mut stmt = conn.prepare(&format!("{RECORD_SELECT} ORDER BY record_id"))?;
            let rows = stmt.query_map([], RecordRow::from_row)?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?.into_record()?);
            }
            Ok(out)
        })
        .map_err(anyhow::Error::from)
    }

    /// One-way import of a legacy `.trove/` JSON store (one file per object,
    /// `manifest.json`, `patches.json`, `execution.json`). Runs once; a `meta`
    /// flag prevents re-imports, and everything lands in a single transaction
    /// so a failure leaves no partial state. Legacy files are left in place.
    pub fn migrate_from_json(&self) -> Result<MigrationReport> {
        let mut report = MigrationReport::default();
        if self.meta_get("json_migrated")?.is_some() {
            return Ok(report);
        }

        let mut blobs = Vec::new();
        for name in ["manifest.json", "graph.json", "symbols.json"] {
            let path = self.root.join(name);
            if path.is_file() {
                let data = fs::read_to_string(&path)
                    .with_context(|| format!("read {}", path.display()))?;
                blobs.push((name, data));
            }
        }

        let mut objects = Vec::new();
        for kind in MemoryKind::ALL {
            let dir = self.root.join(legacy_dir_for(kind));
            if !dir.is_dir() {
                continue;
            }
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                if path.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let parsed = fs::read_to_string(&path)
                    .map_err(|e| e.to_string())
                    .and_then(|d| {
                        serde_json::from_str::<MemoryObject>(&d).map_err(|e| e.to_string())
                    });
                match parsed {
                    Ok(obj) => objects.push(obj),
                    Err(e) => report.errors.push(format!("{}: {e}", path.display())),
                }
            }
        }

        let mut patches: Vec<PatchRecord> = Vec::new();
        let patches_path = self.root.join("patches.json");
        if patches_path.is_file() {
            match parse_json_file::<Vec<PatchRecord>>(&patches_path) {
                Ok(p) => patches = p,
                Err(e) => report
                    .errors
                    .push(format!("{}: {e}", patches_path.display())),
            }
        }

        let mut exec_events: Vec<String> = Vec::new();
        let exec_path = self.root.join("execution.json");
        if exec_path.is_file() {
            match parse_json_file::<ExecutionMemory>(&exec_path) {
                Ok(memory) => {
                    for event in &memory.events {
                        exec_events.push(serde_json::to_string(event)?);
                    }
                }
                Err(e) => report.errors.push(format!("{}: {e}", exec_path.display())),
            }
        }

        let object_rows = objects
            .iter()
            .map(|o| Ok((o.kind.as_str(), o.id.clone(), serde_json::to_string(o)?)))
            .collect::<Result<Vec<_>>>()?;
        let patch_rows = patches
            .iter()
            .map(|p| Ok((p.id.clone(), serde_json::to_string(p)?)))
            .collect::<Result<Vec<_>>>()?;
        let migrated_at = Utc::now();

        self.write_tx(|tx| {
            for (name, value) in &blobs {
                tx.execute(
                    "INSERT INTO kv (name, value) VALUES (?1, ?2)
                     ON CONFLICT(name) DO UPDATE SET value = excluded.value",
                    params![name, value],
                )?;
            }
            for (kind, id, data) in &object_rows {
                tx.execute(
                    "INSERT INTO objects (kind, id, data) VALUES (?1, ?2, ?3)
                     ON CONFLICT(kind, id) DO UPDATE SET data = excluded.data",
                    params![kind, id, data],
                )?;
            }
            for (id, data) in &patch_rows {
                tx.execute(
                    "INSERT INTO patches (id, data, created_at) VALUES (?1, ?2, ?3)
                     ON CONFLICT(id) DO NOTHING",
                    params![id, data, migrated_at],
                )?;
            }
            for data in &exec_events {
                tx.execute(
                    "INSERT INTO execution_events (data, created_at) VALUES (?1, ?2)",
                    params![data, migrated_at],
                )?;
            }
            tx.execute(
                "INSERT INTO meta (key, value) VALUES ('json_migrated', ?1)
                 ON CONFLICT(key) DO UPDATE SET value = excluded.value",
                params![migrated_at.to_rfc3339()],
            )?;
            Ok(())
        })?;

        report.kv_entries = blobs.len();
        report.objects = object_rows.len();
        report.patches = patch_rows.len();
        report.execution_events = exec_events.len();
        Ok(report)
    }

    fn migrate_legacy_json(&self) -> Result<()> {
        if !legacy_json_present(&self.root) {
            return Ok(());
        }
        let report = self.migrate_from_json()?;
        for err in &report.errors {
            eprintln!("trove: skipped during store migration: {err}");
        }
        Ok(())
    }

    fn meta_get(&self, key: &str) -> Result<Option<String>, StoreError> {
        self.conn_op(|conn| {
            Ok(conn
                .query_row("SELECT value FROM meta WHERE key = ?1", params![key], |r| {
                    r.get(0)
                })
                .optional()?)
        })
    }
}

const EVENT_SELECT: &str = "SELECT event_id, submission_id, kind, body, supersedes, project, agent,
            run_id, source_refs, source_hash, created_at FROM events";

const RECORD_SELECT: &str = "SELECT record_id, revision, kind, body, project, agent, run_id,
            source_refs, source_hash, created_at, updated_at FROM records";

struct EventRow {
    event_id: String,
    submission_id: String,
    kind: String,
    body: String,
    supersedes: Option<String>,
    project: String,
    agent: String,
    run_id: String,
    source_refs: String,
    source_hash: Option<String>,
    created_at: DateTime<Utc>,
}

impl EventRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            event_id: row.get(0)?,
            submission_id: row.get(1)?,
            kind: row.get(2)?,
            body: row.get(3)?,
            supersedes: row.get(4)?,
            project: row.get(5)?,
            agent: row.get(6)?,
            run_id: row.get(7)?,
            source_refs: row.get(8)?,
            source_hash: row.get(9)?,
            created_at: row.get(10)?,
        })
    }

    fn into_event(self) -> Result<ReflectionEvent, StoreError> {
        Ok(ReflectionEvent {
            event_id: self.event_id,
            submission_id: self.submission_id,
            kind: parse_kind(&self.kind)?,
            body: self.body,
            supersedes: self.supersedes,
            provenance: Provenance {
                project: self.project,
                agent: self.agent,
                run_id: self.run_id,
                source_refs: serde_json::from_str(&self.source_refs)?,
                source_hash: self.source_hash,
            },
            created_at: self.created_at,
        })
    }
}

struct RecordRow {
    record_id: String,
    revision: u64,
    kind: String,
    body: String,
    project: String,
    agent: String,
    run_id: String,
    source_refs: String,
    source_hash: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl RecordRow {
    fn from_row(row: &rusqlite::Row) -> rusqlite::Result<Self> {
        Ok(Self {
            record_id: row.get(0)?,
            revision: row.get::<_, i64>(1)? as u64,
            kind: row.get(2)?,
            body: row.get(3)?,
            project: row.get(4)?,
            agent: row.get(5)?,
            run_id: row.get(6)?,
            source_refs: row.get(7)?,
            source_hash: row.get(8)?,
            created_at: row.get(9)?,
            updated_at: row.get(10)?,
        })
    }

    fn into_record(self) -> Result<MemoryRecord, StoreError> {
        Ok(MemoryRecord {
            record_id: self.record_id,
            revision: self.revision,
            kind: parse_kind(&self.kind)?,
            body: self.body,
            provenance: Provenance {
                project: self.project,
                agent: self.agent,
                run_id: self.run_id,
                source_refs: serde_json::from_str(&self.source_refs)?,
                source_hash: self.source_hash,
            },
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn event_by_submission(
    conn: &Connection,
    submission_id: &str,
) -> Result<Option<ReflectionEvent>, StoreError> {
    conn.query_row(
        &format!("{EVENT_SELECT} WHERE submission_id = ?1"),
        params![submission_id],
        EventRow::from_row,
    )
    .optional()?
    .map(EventRow::into_event)
    .transpose()
}

fn record_by_id(conn: &Connection, record_id: &str) -> Result<Option<MemoryRecord>, StoreError> {
    conn.query_row(
        &format!("{RECORD_SELECT} WHERE record_id = ?1"),
        params![record_id],
        RecordRow::from_row,
    )
    .optional()?
    .map(RecordRow::into_record)
    .transpose()
}

fn parse_kind(s: &str) -> Result<RecordKind, StoreError> {
    RecordKind::from_str(s)
        .map_err(|_| StoreError::CorruptData(format!("unknown record kind {s:?}")))
}

fn parse_json_file<T: for<'de> Deserialize<'de>>(path: &Path) -> Result<T, String> {
    fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|d| serde_json::from_str(&d).map_err(|e| e.to_string()))
}

/// One-time database setup: WAL mode, durable sync, schema. Retried on busy so
/// many agents opening the same fresh store at once still converge.
fn init_db(conn: &Connection) -> Result<(), rusqlite::Error> {
    let mut attempts = 0usize;
    loop {
        let result = (|| {
            conn.pragma_update(None, "journal_mode", "WAL")?;
            conn.pragma_update(None, "synchronous", "FULL")?;
            conn.pragma_update(None, "foreign_keys", "ON")?;
            conn.busy_timeout(BUSY_TIMEOUT)?;
            conn.execute_batch(SCHEMA)?;
            conn.pragma_update(None, "user_version", SCHEMA_VERSION)
        })();
        match result {
            Err(e) if is_busy_sqlite(&e) && attempts < MAX_BUSY_RETRIES => {
                attempts += 1;
                thread::sleep(Duration::from_millis(25 * attempts as u64));
            }
            other => return other,
        }
    }
}

fn is_busy_sqlite(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(e, _)
            if matches!(
                e.code,
                ErrorCode::DatabaseBusy | ErrorCode::DatabaseLocked
            )
    )
}

fn is_busy(err: &StoreError) -> bool {
    matches!(err, StoreError::Sqlite(e) if is_busy_sqlite(e))
}

fn legacy_dir_for(kind: MemoryKind) -> &'static str {
    match kind {
        MemoryKind::Symbol => "symbols",
        MemoryKind::Module => "modules",
        MemoryKind::Subsystem => "subsystems",
        MemoryKind::Architecture => "architecture",
        MemoryKind::Historical => "historical",
        MemoryKind::Repository => "repository",
        MemoryKind::Working => "working",
        MemoryKind::Patch => "patches",
        MemoryKind::Execution => "execution",
    }
}

fn legacy_json_present(root: &Path) -> bool {
    const BLOBS: [&str; 5] = [
        "manifest.json",
        "graph.json",
        "symbols.json",
        "execution.json",
        "patches.json",
    ];
    if BLOBS.iter().any(|name| root.join(name).is_file()) {
        return true;
    }
    MemoryKind::ALL.iter().any(|kind| {
        let dir = root.join(legacy_dir_for(*kind));
        dir.is_dir()
            && fs::read_dir(&dir)
                .map(|entries| {
                    entries
                        .flatten()
                        .any(|e| e.path().extension().and_then(|x| x.to_str()) == Some("json"))
                })
                .unwrap_or(false)
    })
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
        let dir = std::env::temp_dir().join(format!("trove-test-store-{}", std::process::id()));
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
