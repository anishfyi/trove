use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::thread;

use chrono::{DateTime, Utc};
use trove_core::compression::CompressedText;
use trove_core::execution::{ExecutionEvent, ExecutionMemory};
use trove_core::memory::Confidence;
use trove_core::object::{MemoryKind, MemoryObject};
use trove_core::patch::PatchRecord;
use trove_core::reflection::{EventSubmission, NewRecord, Provenance, RecordKind, RecordUpdate};
use trove_core::store::{StoreError, TroveManifest, TroveStore};

fn tmpdir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!(
        "trove-sqlite-{name}-{}-{nanos}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = fs::remove_dir_all(&dir);
    dir
}

fn provenance(agent: &str) -> Provenance {
    Provenance {
        project: "trove-test".into(),
        agent: agent.into(),
        run_id: "run-1".into(),
        source_refs: vec!["src/main.rs:1".into()],
        source_hash: Some("deadbeef".into()),
    }
}

fn submission(id: &str, kind: RecordKind, body: &str) -> EventSubmission {
    EventSubmission {
        submission_id: id.into(),
        kind,
        body: body.into(),
        supersedes: None,
        provenance: provenance("agent"),
    }
}

fn new_record(id: &str, body: &str) -> NewRecord {
    NewRecord {
        record_id: id.into(),
        kind: RecordKind::Observation,
        body: body.into(),
        provenance: provenance("agent"),
    }
}

fn update(body: &str) -> RecordUpdate {
    RecordUpdate {
        kind: RecordKind::Hypothesis,
        body: body.into(),
        provenance: provenance("agent"),
    }
}

#[test]
fn store_is_wal_and_shared() {
    let dir = tmpdir("wal");
    let store = TroveStore::open(&dir).unwrap();
    assert!(store.path().join("store.db").is_file());

    let conn = rusqlite::Connection::open(store.path().join("store.db")).unwrap();
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "wal");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn events_are_idempotent_and_append_only() {
    let dir = tmpdir("events");
    let store = TroveStore::open(&dir).unwrap();

    let original = submission("s-1", RecordKind::Observation, "sky is green");
    let e1 = store.submit_event(&original).unwrap();

    // A retried submission returns the stored event instead of writing again.
    let retried = store.submit_event(&original).unwrap();
    assert_eq!(retried.event_id, e1.event_id);

    // Even with a different body, the submission id wins: no double-write.
    let mut changed = original.clone();
    changed.body = "rewritten".into();
    let again = store.submit_event(&changed).unwrap();
    assert_eq!(again.event_id, e1.event_id);
    assert_eq!(again.body, "sky is green");

    // A correction appends a superseding event; the original stays untouched.
    let correction = EventSubmission {
        supersedes: Some(e1.event_id.clone()),
        ..submission("s-2", RecordKind::Observation, "sky is blue")
    };
    let e2 = store.submit_event(&correction).unwrap();
    assert_eq!(e2.supersedes.as_deref(), Some(e1.event_id.as_str()));

    let events = store.read_events().unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].body, "sky is green");
    assert_eq!(events[1].body, "sky is blue");
    assert_eq!(events[0].provenance.agent, "agent");
    assert_eq!(events[0].provenance.project, "trove-test");
    assert_eq!(
        events[0].provenance.source_refs,
        vec!["src/main.rs:1".to_string()]
    );
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn record_revision_checks() {
    let dir = tmpdir("records");
    let store = TroveStore::open(&dir).unwrap();

    let rec = store.create_record(&new_record("r1", "v1")).unwrap();
    assert_eq!(rec.revision, 1);

    assert!(matches!(
        store.create_record(&new_record("r1", "dup")),
        Err(StoreError::RecordExists(_))
    ));

    let updated = store.update_record("r1", 1, &update("v2")).unwrap();
    assert_eq!(updated.revision, 2);
    assert_eq!(updated.body, "v2");
    assert_eq!(updated.kind, RecordKind::Hypothesis);

    match store.update_record("r1", 1, &update("v3")) {
        Err(StoreError::RevisionConflict {
            record_id,
            expected: 1,
            actual: 2,
        }) => assert_eq!(record_id, "r1"),
        other => panic!("expected RevisionConflict, got {other:?}"),
    }

    assert!(matches!(
        store.update_record("missing", 3, &update("x")),
        Err(StoreError::RecordNotFound(_))
    ));

    // Rejected writes changed nothing.
    let current = store.read_record("r1").unwrap().unwrap();
    assert_eq!(current.revision, 2);
    assert_eq!(current.body, "v2");
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn objects_replace_and_upsert() {
    let dir = tmpdir("objects");
    let store = TroveStore::open(&dir).unwrap();

    let a = MemoryObject::new("sym:a", MemoryKind::Symbol, "h1");
    let b = MemoryObject::new("sym:b", MemoryKind::Symbol, "h2");
    store.write_objects(MemoryKind::Symbol, &[a, b]).unwrap();
    assert_eq!(store.read_objects(MemoryKind::Symbol).unwrap().len(), 2);

    // write_objects replaces the kind's whole set.
    let c = MemoryObject::new("sym:c", MemoryKind::Symbol, "h3");
    store
        .write_objects(MemoryKind::Symbol, std::slice::from_ref(&c))
        .unwrap();
    let remaining = store.read_objects(MemoryKind::Symbol).unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, "sym:c");

    // upsert merges without deleting; existing ids are overwritten.
    let d = MemoryObject::new("sym:d", MemoryKind::Symbol, "h4");
    let mut c_new = c.clone();
    c_new.hash = "h3b".into();
    store.upsert_objects(&[c_new, d]).unwrap();
    let all = store.read_objects(MemoryKind::Symbol).unwrap();
    assert_eq!(all.len(), 2);
    let c_read = all.iter().find(|o| o.id == "sym:c").unwrap();
    assert_eq!(c_read.hash, "h3b");

    // Other kinds are untouched.
    assert!(store.read_objects(MemoryKind::Module).unwrap().is_empty());
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn execution_events_append_and_trim() {
    let dir = tmpdir("execution");
    let store = TroveStore::open(&dir).unwrap();
    let at = DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
        .unwrap()
        .with_timezone(&Utc);

    for i in 0..505usize {
        store
            .record_execution(&ExecutionEvent::Search {
                query: format!("q{i}"),
                at,
            })
            .unwrap();
    }
    let memory = store.read_execution().unwrap();
    assert_eq!(memory.events.len(), 500);
    match &memory.events[0] {
        ExecutionEvent::Search { query, .. } => assert_eq!(query, "q5"),
        other => panic!("unexpected event {other:?}"),
    }
    match &memory.events[499] {
        ExecutionEvent::Search { query, .. } => assert_eq!(query, "q504"),
        other => panic!("unexpected event {other:?}"),
    }

    // write_execution still replaces the whole log.
    let memory = ExecutionMemory {
        events: vec![ExecutionEvent::Command {
            cmd: "ls".into(),
            at,
        }],
    };
    store.write_execution(&memory).unwrap();
    assert_eq!(store.read_execution().unwrap().events.len(), 1);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn concurrent_writers_no_lost_writes_or_double_submits() {
    let dir = tmpdir("concurrent");
    let store = TroveStore::open(&dir).unwrap();
    store.create_record(&new_record("shared", "seed")).unwrap();
    store.create_record(&new_record("storm", "seed")).unwrap();

    const THREADS: usize = 8;
    const EVENTS_PER: usize = 15;
    const UPDATES_PER: usize = 8;

    let barrier = Arc::new(Barrier::new(THREADS));
    let mut handles = Vec::new();
    for t in 0..THREADS {
        let store = store.clone();
        let barrier = barrier.clone();
        handles.push(thread::spawn(move || {
            let agent = format!("agent-{t}");
            let mut ok = 0usize;
            let mut conflicts = 0usize;
            barrier.wait();

            for i in 0..EVENTS_PER {
                let sub = EventSubmission {
                    submission_id: format!("t{t}-sub{i}"),
                    kind: RecordKind::Observation,
                    body: format!("event {t}/{i}"),
                    supersedes: None,
                    provenance: provenance(&agent),
                };
                store.submit_event(&sub).unwrap();
                let retried = store.submit_event(&sub).unwrap();
                assert_eq!(retried.submission_id, sub.submission_id);
            }

            // Readers run while other threads are still writing.
            let _ = store.read_events().unwrap();
            let _ = store.read_objects(MemoryKind::Symbol).unwrap();

            for _ in 0..UPDATES_PER {
                let current = store.read_record("shared").unwrap().unwrap();
                match store.update_record(
                    "shared",
                    current.revision,
                    &update(&format!("{agent} rev {}", current.revision)),
                ) {
                    Ok(_) => ok += 1,
                    Err(StoreError::RevisionConflict { .. }) => conflicts += 1,
                    Err(e) => panic!("unexpected store error: {e}"),
                }
            }

            // Stale revision 0 always conflicts: the record exists at >= 1.
            match store.update_record("shared", 0, &update("stale")) {
                Err(StoreError::RevisionConflict { .. }) => conflicts += 1,
                other => panic!("expected RevisionConflict, got {other:?}"),
            }

            // Fixed expected revision: at most one thread can win this update.
            let storm_won = store.update_record("storm", 1, &update(&agent)).is_ok();
            (ok, conflicts, storm_won)
        }));
    }

    let mut total_ok = 0usize;
    let mut total_conflicts = 0usize;
    let mut storm_wins = 0usize;
    for h in handles {
        let (ok, conflicts, storm_won) = h.join().unwrap();
        total_ok += ok;
        total_conflicts += conflicts;
        storm_wins += storm_won as usize;
    }

    // Every submitted event landed exactly once, despite one retry each.
    let events = store.read_events().unwrap();
    assert_eq!(events.len(), THREADS * EVENTS_PER);
    let ids: HashSet<_> = events.iter().map(|e| e.submission_id.as_str()).collect();
    assert_eq!(ids.len(), THREADS * EVENTS_PER);
    assert!(events
        .iter()
        .all(|e| !e.provenance.agent.is_empty() && !e.provenance.run_id.is_empty()));

    // Revision bookkeeping: accepted writes bumped by exactly one each, and
    // every read-then-write attempt either succeeded or was rejected.
    let shared = store.read_record("shared").unwrap().unwrap();
    assert_eq!(shared.revision, 1 + total_ok as u64);
    assert_eq!(total_ok + total_conflicts, THREADS * (UPDATES_PER + 1));
    assert!(total_conflicts >= THREADS);

    // The fixed-revision race produced exactly one winner; everyone else was
    // rejected rather than silently overwriting.
    assert_eq!(storm_wins, 1);
    let storm = store.read_record("storm").unwrap().unwrap();
    assert_eq!(storm.revision, 2);
    let _ = fs::remove_dir_all(&dir);
}

fn fixture_object(id: &str, kind: MemoryKind) -> MemoryObject {
    let mut obj = MemoryObject::new(id, kind, format!("hash-{id}"));
    obj.path = Some(format!("src/{id}.rs"));
    obj.text = CompressedText {
        raw: Some(format!("raw text for {id}")),
        summary: Some(format!("summary {id}")),
        ultra_summary: Some(format!("ultra {id}")),
        keywords: vec!["alpha".into(), "beta".into()],
        embedding: Some(vec![0.1, 0.2, 0.3]),
    };
    obj.dependencies = vec!["dep:1".into(), "dep:2".into()];
    obj.symbols = vec!["sym:1".into()];
    obj.updated_at = DateTime::parse_from_rfc3339("2026-01-02T03:04:05Z")
        .unwrap()
        .with_timezone(&Utc);
    obj.importance = 0.75;
    obj.confidence = Some(Confidence::high("fixture", vec!["test".into()]));
    obj.risk = 0.4;
    obj.git_commit = Some("abc123".into());
    obj
}

fn write_json_file(path: &Path, value: &impl serde::Serialize) {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    fs::write(path, serde_json::to_string_pretty(value).unwrap()).unwrap();
}

#[test]
fn migrates_legacy_json_store_without_loss() {
    let dir = tmpdir("migrate");
    let root = dir.join(".trove");

    let objects: Vec<MemoryObject> = vec![
        fixture_object("sym:a", MemoryKind::Symbol),
        fixture_object("sym:b", MemoryKind::Symbol),
        fixture_object("mod:x", MemoryKind::Module),
        fixture_object("subsys:y", MemoryKind::Subsystem),
        fixture_object("arch:main", MemoryKind::Architecture),
        fixture_object("hist:1", MemoryKind::Historical),
        fixture_object("repo:index", MemoryKind::Repository),
        fixture_object("work:1", MemoryKind::Working),
        fixture_object("patch-kind:1", MemoryKind::Patch),
        fixture_object("exec-kind:1", MemoryKind::Execution),
    ];
    let dir_for = |kind: MemoryKind| match kind {
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
    for obj in &objects {
        let file = root
            .join(dir_for(obj.kind))
            .join(format!("{}.json", obj.id.replace(':', "_")));
        write_json_file(&file, obj);
    }

    let manifest = TroveManifest {
        version: "0.2.0".into(),
        repo_root: dir.to_string_lossy().into(),
        indexed_at: "2026-01-02T03:04:05Z".into(),
        symbol_count: 2,
        module_count: 1,
        subsystem_count: 1,
    };
    write_json_file(&root.join("manifest.json"), &manifest);

    let graph = serde_json::json!({"adjacency": {"a": ["b"]}, "edges": 1});
    write_json_file(&root.join("graph.json"), &graph);
    let symbols = serde_json::json!([{"id": "sym:a", "name": "alpha", "path": "src/a.rs"}]);
    write_json_file(&root.join("symbols.json"), &symbols);

    let execution = ExecutionMemory {
        events: vec![
            ExecutionEvent::Search {
                query: "q".into(),
                at: DateTime::parse_from_rfc3339("2026-01-01T00:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
            ExecutionEvent::OpenedFile {
                path: "src/a.rs".into(),
                at: DateTime::parse_from_rfc3339("2026-01-01T00:01:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
            },
        ],
    };
    write_json_file(&root.join("execution.json"), &execution);

    let patch = PatchRecord {
        id: "patch:src/a.rs:1".into(),
        path: "src/a.rs".into(),
        before_hash: "b".into(),
        after_hash: "a".into(),
        reason: "fix".into(),
        related_issue: Some("issue-1".into()),
        related_pr: None,
        author: Some("dev".into()),
        date: DateTime::parse_from_rfc3339("2026-01-01T02:00:00Z")
            .unwrap()
            .with_timezone(&Utc),
        future_impact: Some("none".into()),
    };
    write_json_file(&root.join("patches.json"), &vec![patch.clone()]);

    // Opening the store runs the one-way migration automatically.
    let store = TroveStore::open(&dir).unwrap();

    // Every legacy object round-trips field for field.
    for expected in &objects {
        let found = store
            .read_objects(expected.kind)
            .unwrap()
            .into_iter()
            .find(|o| o.id == expected.id)
            .unwrap_or_else(|| panic!("missing object {}", expected.id));
        assert_eq!(
            serde_json::to_value(&found).unwrap(),
            serde_json::to_value(expected).unwrap(),
            "object {} changed during migration",
            expected.id
        );
    }
    assert_eq!(
        store.read_objects(MemoryKind::Symbol).unwrap().len(),
        2,
        "both symbols migrated"
    );

    let loaded_manifest = store.read_manifest().unwrap().unwrap();
    assert_eq!(
        serde_json::to_value(&loaded_manifest).unwrap(),
        serde_json::to_value(&manifest).unwrap()
    );
    assert_eq!(
        store
            .read_json::<serde_json::Value>("graph.json")
            .unwrap()
            .unwrap(),
        graph
    );
    assert_eq!(
        store
            .read_json::<serde_json::Value>("symbols.json")
            .unwrap()
            .unwrap(),
        symbols
    );

    let events = store.read_execution().unwrap().events;
    assert_eq!(
        serde_json::to_value(&events).unwrap(),
        serde_json::to_value(&execution.events).unwrap()
    );
    let patches = store.read_patches().unwrap();
    assert_eq!(patches.len(), 1);
    assert_eq!(
        serde_json::to_value(&patches[0]).unwrap(),
        serde_json::to_value(&patch).unwrap()
    );

    // Migration is non-destructive: legacy files stay on disk.
    assert!(root.join("manifest.json").is_file());
    assert!(root.join("symbols/sym_a.json").is_file());
    assert!(root.join("execution.json").is_file());

    // A second open does not re-import.
    let store2 = TroveStore::open(&dir).unwrap();
    assert_eq!(store2.read_execution().unwrap().events.len(), 2);
    let report = store2.migrate_from_json().unwrap();
    assert_eq!(report.objects, 0);
    assert_eq!(report.execution_events, 0);
    let _ = fs::remove_dir_all(&dir);
}

#[test]
fn migrate_from_json_reports_counts() {
    let dir = tmpdir("migrate-report");
    let root = dir.join(".trove");
    fs::create_dir_all(&root).unwrap();

    // Start with an empty store so no auto-migration happens yet.
    let store = TroveStore::open(&dir).unwrap();

    let obj = fixture_object("mod:m", MemoryKind::Module);
    write_json_file(&root.join("modules/mod_m.json"), &obj);
    write_json_file(&root.join("manifest.json"), &TroveManifest::default());

    let report = store.migrate_from_json().unwrap();
    assert_eq!(report.objects, 1);
    assert_eq!(report.kv_entries, 1);
    assert_eq!(report.patches, 0);
    assert_eq!(report.execution_events, 0);
    assert!(report.errors.is_empty());
    assert_eq!(store.read_objects(MemoryKind::Module).unwrap().len(), 1);
    let _ = fs::remove_dir_all(&dir);
}
