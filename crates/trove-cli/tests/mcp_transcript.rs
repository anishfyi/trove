//! Transcript tests against the real `trove mcp serve` binary over stdio:
//! lifecycle, all four tools, resources, prompts, and the error shapes of
//! the spec (crates/trove-mcp/SPEC.md). The repo is a temp copy of the
//! evals fixture, indexed once via the real `trove index`.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

struct ServerProc {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<ChildStdout>,
}

impl ServerProc {
    fn start(repo: &Path, home: &Path) -> ServerProc {
        let mut child = Command::new(env!("CARGO_BIN_EXE_trove"))
            .args(["--repo", repo.to_str().unwrap(), "mcp", "serve"])
            .env("HOME", home)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn trove mcp serve");
        let stdin = child.stdin.take().unwrap();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        ServerProc {
            child,
            stdin: Some(stdin),
            stdout,
        }
    }

    fn send(&mut self, value: &serde_json::Value) {
        self.stdin
            .as_mut()
            .unwrap()
            .write_all(format!("{}\n", serde_json::to_string(value).unwrap()).as_bytes())
            .unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    fn recv(&mut self) -> serde_json::Value {
        let mut line = String::new();
        let n = self
            .stdout
            .read_line(&mut line)
            .expect("read response line");
        assert!(n > 0, "server closed stdout before responding");
        serde_json::from_str(line.trim()).expect("response line is JSON")
    }

    fn raw(&mut self, line: &str) {
        self.stdin
            .as_mut()
            .unwrap()
            .write_all(format!("{line}\n").as_bytes())
            .unwrap();
        self.stdin.as_mut().unwrap().flush().unwrap();
    }

    fn request(&mut self, id: i64, method: &str, params: serde_json::Value) -> serde_json::Value {
        self.send(&serde_json::json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        self.recv()
    }
}

impl Drop for ServerProc {
    fn drop(&mut self) {
        drop(self.stdin.take());
        let _ = self.child.wait();
    }
}

fn copy_dir(src: &Path, dst: &Path) {
    std::fs::create_dir_all(dst).unwrap();
    for entry in std::fs::read_dir(src).unwrap().flatten() {
        let from = entry.path();
        let to = dst.join(entry.file_name());
        if from.is_dir() {
            copy_dir(&from, &to);
        } else {
            std::fs::copy(&from, &to).unwrap();
        }
    }
}

fn fixture(name: &str) -> (PathBuf, PathBuf) {
    let dir = std::env::temp_dir().join(format!(
        "trove-mcp-transcript-{name}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("empty-home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = dir.join("fixture");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/fixture"),
        &repo,
    );
    // Section index bodies so the skills resources have something to show.
    for (section, body) in [
        (
            "conventions",
            "# Conventions Index\n\n- [Token bucket](entries/token-bucket.md)\n",
        ),
        (
            "memory",
            "# Memory Index\n\n- [WAL gotcha: bulk inserts](entries/wal-bulk-inserts.md)\n",
        ),
        (
            "context",
            "# Context Index\n\n- [Service map](entries/service-map.md)\n",
        ),
    ] {
        std::fs::write(
            repo.join(".claude/trove").join(section).join("INDEX.md"),
            body,
        )
        .unwrap();
    }
    // Give status/query something to read: index with the real binary.
    let out = Command::new(env!("CARGO_BIN_EXE_trove"))
        .args(["--repo", repo.to_str().unwrap(), "index"])
        .env("HOME", &home)
        .output()
        .unwrap();
    assert!(out.status.success(), "fixture indexing failed: {:?}", out);
    (repo, home)
}

#[test]
fn transcript_lifecycle_and_tools() {
    let (repo, home) = fixture("lifecycle");
    let mut p = ServerProc::start(&repo, &home);

    // initialize
    let r = p.request(1, "initialize", serde_json::json!({}));
    assert_eq!(r["result"]["serverInfo"]["name"], "trove");
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05");

    // initialized notification: no response; the next request proves it.
    p.send(&serde_json::json!({"jsonrpc": "2.0", "method": "notifications/initialized"}));
    let r = p.request(2, "ping", serde_json::json!({}));
    assert_eq!(r["result"], serde_json::json!({}));

    // tools/list: exactly the four read-only tools of the spec
    let r = p.request(3, "tools/list", serde_json::json!({}));
    let names: Vec<&str> = r["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(
        names,
        [
            "trove_ask",
            "trove_query",
            "trove_status",
            "trove_tools_search"
        ]
    );

    // trove_ask: offline retrieval with citations
    let r = p.request(
        4,
        "tools/call",
        serde_json::json!({"name": "trove_ask", "arguments": {"question": "what trips up bulk inserts"}}),
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("wal-bulk-inserts.md"), "cited source: {text}");
    assert!(text.contains("WAL gotcha"), "title: {text}");
    assert!(
        text.contains("function runBulkInsert(client, rows)"),
        "source snippet content: {text}"
    );
    assert!(
        text.starts_with("mode: offline retrieval"),
        "always offline: {text}"
    );

    // trove_ask freshness: an entry written between calls is visible
    std::fs::write(
        repo.join(".claude/trove/memory/entries/fresh-fact.md"),
        "---\ntitle: Fresh fact\ntype: reference\n---\n\nThe freshness probe fact.\n",
    )
    .unwrap();
    let r = p.request(
        5,
        "tools/call",
        serde_json::json!({"name": "trove_ask", "arguments": {"question": "freshness probe fact"}}),
    );
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(
        text.contains("fresh-fact.md"),
        "retrieval is fresh per call: {text}"
    );

    // trove_query: level-major context items
    let r = p.request(
        6,
        "tools/call",
        serde_json::json!({"name": "trove_query", "arguments": {"question": "runBulkInsert"}}),
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Query: runBulkInsert"), "{text}");
    assert!(text.contains("L1"), "symbol items at level L1: {text}");
    assert!(text.contains("runBulkInsert"), "item text content: {text}");

    // trove_status: index freshness via the real manifest
    let r = p.request(
        7,
        "tools/call",
        serde_json::json!({"name": "trove_status", "arguments": {}}),
    );
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("Trove v"), "{text}");
    assert!(text.contains("Counts:"), "{text}");

    // trove_tools_search: honest empty catalog under a scratch HOME
    let r = p.request(
        8,
        "tools/call",
        serde_json::json!({"name": "trove_tools_search", "arguments": {"query": "git"}}),
    );
    assert_eq!(r["result"]["isError"], false, "{r}");
    let text = r["result"]["content"][0]["text"].as_str().unwrap();
    assert!(!text.is_empty(), "always a text body: {r}");

    // handler-level failure shape: bad question type -> isError, not protocol error
    let r = p.request(
        9,
        "tools/call",
        serde_json::json!({"name": "trove_ask", "arguments": {"question": 42}}),
    );
    assert_eq!(r["result"]["isError"], true, "{r}");
}

#[test]
fn transcript_resources_prompts_and_errors() {
    let (repo, home) = fixture("resources");
    let mut p = ServerProc::start(&repo, &home);

    let r = p.request(1, "resources/list", serde_json::json!({}));
    let uris: Vec<&str> = r["result"]["resources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["uri"].as_str().unwrap())
        .collect();
    assert_eq!(
        uris,
        [
            "trove://index",
            "trove://skills/conventions",
            "trove://skills/memory",
            "trove://skills/context"
        ]
    );

    let r = p.request(
        2,
        "resources/read",
        serde_json::json!({"uri": "trove://index"}),
    );
    let text = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("# Fixture trove"), "{text}");
    assert_eq!(r["result"]["contents"][0]["mimeType"], "text/markdown");

    let r = p.request(
        3,
        "resources/read",
        serde_json::json!({"uri": "trove://skills/memory"}),
    );
    let text = r["result"]["contents"][0]["text"].as_str().unwrap();
    assert!(text.contains("WAL gotcha"), "{text}");

    // Arbitrary URIs are refused: no file reads outside the allowlist.
    let r = p.request(
        4,
        "resources/read",
        serde_json::json!({"uri": "file:///etc/passwd"}),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");
    let r = p.request(
        5,
        "resources/read",
        serde_json::json!({"uri": "trove://../../etc/passwd"}),
    );
    assert_eq!(r["error"]["code"], -32602, "{r}");

    let r = p.request(6, "prompts/list", serde_json::json!({}));
    let names: Vec<&str> = r["result"]["prompts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|x| x["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["remember", "reflect"]);

    let r = p.request(7, "prompts/get", serde_json::json!({"name": "remember"}));
    let text = r["result"]["messages"][0]["content"]["text"]
        .as_str()
        .unwrap();
    assert!(
        text.to_lowercase().contains("never") && text.to_lowercase().contains("secret"),
        "{text}"
    );

    // Unknown method and malformed frames
    let r = p.request(8, "workspace/symbol", serde_json::json!({}));
    assert_eq!(r["error"]["code"], -32601);
    p.raw("not json at all");
    let r = p.recv();
    assert_eq!(r["id"], serde_json::Value::Null);
    assert_eq!(r["error"]["code"], -32700);

    // The server survives all of the above and still answers.
    let r = p.request(9, "ping", serde_json::json!({}));
    assert_eq!(r["result"], serde_json::json!({}));
}

#[test]
fn transcript_protocol_version_negotiation() {
    let (repo, home) = fixture("negotiate");
    let mut p = ServerProc::start(&repo, &home);
    let r = p.request(
        1,
        "initialize",
        serde_json::json!({"protocolVersion": "2025-03-26"}),
    );
    assert_eq!(r["result"]["protocolVersion"], "2025-03-26", "{r}");
    let r = p.request(
        2,
        "initialize",
        serde_json::json!({"protocolVersion": "2024-11-05"}),
    );
    assert_eq!(r["result"]["protocolVersion"], "2024-11-05", "{r}");
    let r = p.request(
        3,
        "initialize",
        serde_json::json!({"protocolVersion": "2099-01-01"}),
    );
    assert_eq!(
        r["result"]["protocolVersion"], "2024-11-05",
        "unknown falls back: {r}"
    );
}

#[test]
fn transcript_invalid_params_and_unknown_tool() {
    let (repo, home) = fixture("params");
    let mut p = ServerProc::start(&repo, &home);
    let r = p.request(
        1,
        "tools/call",
        serde_json::json!({"name": "trove_write", "arguments": {}}),
    );
    assert_eq!(
        r["error"]["code"], -32602,
        "write tool does not exist yet: {r}"
    );
    let r = p.request(
        2,
        "tools/call",
        serde_json::json!({"name": "trove_ask", "arguments": {}}),
    );
    assert_eq!(
        r["result"]["isError"], true,
        "missing question is a tool error: {r}"
    );
}
