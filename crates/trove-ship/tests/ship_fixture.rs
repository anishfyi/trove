//! End to end: ship a fixture repo seeded with fake secrets and PII and check
//! that the docs contain none of them while the report lists each one.
//!
//! Token-shaped values are assembled here and substituted into
//! `tests/fixtures/seeded` at runtime, so the repository never contains a
//! literal credential that a secret scanner would flag.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use trove_ship::redact::{normalize, LocalIdentity};
use trove_ship::{ship, FailClosed, Format, Reindex, ShipOptions};

struct Seed {
    placeholder: Option<&'static str>,
    value: String,
    detector: &'static str,
}

fn seed(placeholder: &'static str, value: String, detector: &'static str) -> Seed {
    Seed {
        placeholder: Some(placeholder),
        value,
        detector,
    }
}

fn literal(value: &str, detector: &'static str) -> Seed {
    Seed {
        placeholder: None,
        value: value.to_string(),
        detector,
    }
}

fn seeds() -> Vec<Seed> {
    let c = |parts: &[&str]| parts.concat();
    vec![
        seed("@@AWS_ID@@", c(&["AKIA", "Q7XK2M9P4L8W1NZ3"]), "aws-access-key-id"),
        seed(
            "@@AWS_SECRET@@",
            c(&["wJalrXUtnFEMI/K7MDENG/", "bPxRfiCYzq8Wq2x9T3"]),
            "aws-secret-access-key",
        ),
        seed("@@DB_PASS@@", c(&["S3cr3tPr0d", "Pa55"]), "url-credentials"),
        seed("@@BEARER@@", c(&["AbC123dEf456", "GhI789jKl012"]), "bearer-token"),
        seed("@@STRIPE@@", c(&["sk_live_", "4eC39HqLyjWDarjtT1zdp7dc"]), "stripe-key"),
        seed("@@GOOGLE@@", c(&["AIza", "SyA1b2C3d4E5f6G7h8I9j0K1l2M3n4O5p6Q"]), "google-api-key"),
        seed(
            "@@JWT@@",
            c(&[
                "eyJ",
                "hbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9",
                ".eyJ",
                "zdWIiOiIxMjM0NTY3ODkwIn0",
                ".",
                "SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c",
            ]),
            "jwt",
        ),
        seed(
            "@@SLACK_HOOK@@",
            c(&["https://hooks.slack.com/services/", "T024BE7LD/B01ABCDEF12/xYz9AbC8dEf7GhI6jKl5MnO4"]),
            "slack-webhook",
        ),
        seed(
            "@@ANTHROPIC@@",
            c(&["sk-ant-", "api03-Zk9mQ2xW7vB4nR8tY1uE5oP3aS6dF0gH"]),
            "anthropic-api-key",
        ),
        seed("@@GH_TOKEN@@", c(&["ghp_", "aB3dE5fG7haB3dE5fG7haB3dE5fG7hJ9kL1m"]), "github-token"),
        seed(
            "@@PRIVATE_KEY@@",
            c(&[
                "-----BEGIN OPENSSH ",
                "PRIVATE KEY-----\nb3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZW\n",
                "QyNTUxOQAAACBmYWtla2V5ZmFrZWtleWZha2VrZXlmYWtla2V5ZmFrZWtleQ\n",
                "-----END OPENSSH ",
                "PRIVATE KEY-----",
            ]),
            "private-key",
        ),
        seed("@@SESSION@@", c(&["q8RzLk2Vx9", "Tn4Wm7Yp3s"]), "secret-assignment"),
        literal("maria.kowalski@acme-billing.com", "email"),
        literal("+1 (415) 555-0132", "phone"),
        literal("jenkins.corp.acme-billing.com", "internal-hostname"),
        literal("db1.prod.internal", "internal-hostname"),
        literal("status.billing-ops.net", "troveignore-internal-domain"),
        literal("10.42.7.19", "ip-address"),
        literal("fd12:3456:789a::7", "ipv6-address"),
        literal("/Users/mkowalski", "home-path"),
        literal("C:\\Users\\mkowalski", "windows-home-path"),
        literal("/srv/devboxes/mkowalski", "local-home"),
        literal("mkowalski", "local-username"),
        literal("Globex", "troveignore-customer"),
        literal("Initech, LLC", "company-suffix"),
        literal("Umbrella Health", "customer-keyword"),
        literal("Project Nightjar", "troveignore-deny"),
        literal("BILL-2041", "troveignore-deny-regex"),
    ]
}

fn fixture_src() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/seeded")
}

/// Copy the fixture into a fresh temp dir with every placeholder filled in.
fn materialize(name: &str, seeds: &[Seed]) -> PathBuf {
    let root = std::env::temp_dir().join(format!("trove-ship-it-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    let repo = root.join("billing-service");
    copy_dir(&fixture_src(), &repo, seeds);
    repo
}

fn copy_dir(src: &Path, dst: &Path, seeds: &[Seed]) {
    fs::create_dir_all(dst).unwrap();
    for entry in fs::read_dir(src).unwrap() {
        let entry = entry.unwrap();
        let to = dst.join(entry.file_name());
        if entry.path().is_dir() {
            copy_dir(&entry.path(), &to, seeds);
        } else {
            let mut text = fs::read_to_string(entry.path()).unwrap();
            for s in seeds {
                if let Some(p) = s.placeholder {
                    text = text.replace(p, &s.value);
                }
            }
            assert!(
                !text.contains("@@"),
                "unfilled placeholder in {}",
                entry.path().display()
            );
            fs::write(to, text).unwrap();
        }
    }
}

fn options(repo: &Path, format: Format) -> ShipOptions {
    let mut opts = ShipOptions::new(repo);
    opts.out = repo
        .parent()
        .unwrap()
        .join(format!("out-{}", format.label()));
    opts.format = format;
    opts.reindex = Reindex::Auto;
    opts.identity = Some(LocalIdentity {
        username: Some("mkowalski".into()),
        home: Some("/srv/devboxes/mkowalski".into()),
    });
    opts
}

fn read_all(dir: &Path) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).unwrap() {
        let p = entry.unwrap().path();
        if p.is_file() {
            out.push((
                p.file_name().unwrap().to_string_lossy().to_string(),
                fs::read_to_string(&p).unwrap(),
            ));
        }
    }
    out
}

fn assert_clean(files: &[(String, String)], seeds: &[Seed]) {
    for (name, content) in files {
        let normalized = normalize(content);
        for s in seeds {
            assert!(
                !content.contains(&s.value) && !normalized.contains(&s.value),
                "{name} still contains the value for {}",
                s.detector
            );
        }
        assert!(
            !content.contains("VENDOR-CONTRACT-SENTINEL"),
            "{name} has excluded content"
        );
    }
}

#[test]
fn folder_ship_strips_every_seed_and_reports_each_one() {
    let seeds = seeds();
    let repo = materialize("folder", &seeds);
    let opts = options(&repo, Format::Folder);
    let outcome = ship(&opts).expect("ship succeeds");

    let files = read_all(&opts.out);
    for expected in [
        "README.md",
        "overview.md",
        "architecture.md",
        "setup.md",
        "conventions.md",
        "decisions.md",
        "gotchas.md",
        "REDACTION_REPORT.md",
        "redaction-report.json",
    ] {
        assert!(
            files.iter().any(|(n, _)| n == expected),
            "missing {expected}"
        );
    }
    assert_clean(&files, &seeds);

    // Each seed reached the docs and is listed in the report under the
    // detector that should have caught it, with a matching length.
    let report = &outcome.report;
    for s in &seeds {
        let len = s.value.chars().count();
        assert!(
            report
                .items
                .iter()
                .any(|i| i.detector == s.detector && i.length == len),
            "report has no {} entry of length {len}; got {:?}",
            s.detector,
            report
                .items
                .iter()
                .map(|i| (&i.detector, i.length))
                .collect::<Vec<_>>()
        );
    }
    assert!(report.files_excluded >= 1);
    assert!(report.verification.starts_with("passed"));

    // The allowlisted public address survives; placeholders replaced the rest.
    let contributing = &files.iter().find(|(n, _)| n == "conventions.md").unwrap().1;
    assert!(contributing.contains("oss@acme-billing.com"));
    assert!(contributing.contains("[REDACTED:"));
    let overview = &files.iter().find(|(n, _)| n == "overview.md").unwrap().1;
    assert!(
        overview.contains("~/src/runbooks"),
        "home path should become ~"
    );

    // The report itself never carries a removed value.
    let report_md = &files
        .iter()
        .find(|(n, _)| n == "REDACTION_REPORT.md")
        .unwrap()
        .1;
    assert!(report_md.contains("| `[REDACTED:"));
    assert!(report_md.contains("troveignore-customer"));

    // Docs are built from the index and the project trove.
    let arch = &files
        .iter()
        .find(|(n, _)| n == "architecture.md")
        .unwrap()
        .1;
    assert!(
        arch.contains("invoiceTotal"),
        "module map comes from the index"
    );
    let decisions = &files.iter().find(|(n, _)| n == "decisions.md").unwrap().1;
    assert!(decisions.contains("Use Postgres for the ledger"));
    let setup = &files.iter().find(|(n, _)| n == "setup.md").unwrap().1;
    assert!(setup.contains("npm run dev") && setup.contains("`SESSION_SECRET`"));

    let _ = fs::remove_dir_all(repo.parent().unwrap());
}

#[test]
fn single_file_and_site_are_clean_too() {
    let seeds = seeds();
    let repo = materialize("formats", &seeds);

    let single = options(&repo, Format::Single);
    ship(&single).expect("single ship");
    let files = read_all(&single.out);
    assert!(files.iter().any(|(n, _)| n == "HANDOFF.md"));
    assert_clean(&files, &seeds);

    let site = options(&repo, Format::Site);
    ship(&site).expect("site ship");
    let files = read_all(&site.out);
    for expected in [
        "index.html",
        "setup.html",
        "gotchas.html",
        "redaction-report.html",
    ] {
        assert!(
            files.iter().any(|(n, _)| n == expected),
            "missing {expected}"
        );
    }
    assert_clean(&files, &seeds);
    let index = &files.iter().find(|(n, _)| n == "index.html").unwrap().1;
    assert!(index.contains("<a href=\"setup.html\">"));

    let _ = fs::remove_dir_all(repo.parent().unwrap());
}

#[test]
fn surviving_secret_fails_closed_and_writes_nothing() {
    let seeds = seeds();
    let repo = materialize("failclosed", &seeds);
    // A Markdown-escaped key slips past the detectors but renders as a real key.
    let escaped = ["sk\\_live\\_", "9xY8wV7uT6sR5qP4oN3mL2kJ"].concat();
    let claude = repo.join("CLAUDE.md");
    let mut text = fs::read_to_string(&claude).unwrap();
    text.push_str(&format!("- Old key, rotated: {escaped}\n"));
    fs::write(&claude, text).unwrap();

    let opts = options(&repo, Format::Folder);
    let err = ship(&opts).expect_err("must fail closed");
    let fc = err.downcast_ref::<FailClosed>().expect("FailClosed error");
    assert!(fc
        .survivors
        .iter()
        .any(|s| s.file == "conventions.md" && s.detector == "stripe-key"));
    assert!(!fc.to_string().contains("9xY8wV7uT6sR5qP4oN3mL2kJ"));
    assert!(!opts.out.exists(), "nothing may be written");

    let _ = fs::remove_dir_all(repo.parent().unwrap());
}

#[cfg(unix)]
#[test]
fn agent_review_catches_what_regexes_miss() {
    let seeds = seeds();
    let repo = materialize("agent", &seeds);
    let mut opts = options(&repo, Format::Folder);
    opts.agent = Some(trove_ship::redact::agent::AgentConfig {
        kind: None,
        command: Some(
            r#"cat >/dev/null; echo '[{"text":"Bluebird","category":"other","reason":"internal codename"}]'"#.into(),
        ),
        timeout: std::time::Duration::from_secs(30),
    });
    let outcome = ship(&opts).expect("ship with agent");
    let files = read_all(&opts.out);
    assert_clean(&files, &seeds);
    assert!(files.iter().all(|(_, c)| !c.contains("Bluebird")));
    assert!(outcome.report.agent_findings >= 1);
    assert!(outcome
        .report
        .items
        .iter()
        .any(|i| i.detector == "agent:custom command" && i.reason == "internal codename"));

    // A failing agent aborts the ship instead of looking like a clean review.
    let mut broken = options(&repo, Format::Folder);
    broken.out = repo.parent().unwrap().join("out-broken");
    broken.agent = Some(trove_ship::redact::agent::AgentConfig {
        kind: None,
        command: Some("cat >/dev/null; exit 1".into()),
        timeout: std::time::Duration::from_secs(30),
    });
    assert!(ship(&broken).is_err());
    assert!(!broken.out.exists());

    let _ = fs::remove_dir_all(repo.parent().unwrap());
}

#[test]
fn cli_exit_codes() {
    let seeds = seeds();
    let repo = materialize("cli", &seeds);
    let bin = env!("CARGO_BIN_EXE_trove-ship");
    let out = repo.parent().unwrap().join("cli-out");

    let ok = Command::new(bin)
        .args([
            "--repo",
            repo.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
            "--json",
        ])
        .output()
        .unwrap();
    assert!(
        ok.status.success(),
        "{}",
        String::from_utf8_lossy(&ok.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&ok.stdout).unwrap();
    assert!(report["items"].as_array().unwrap().len() >= 20);
    assert!(out.join("REDACTION_REPORT.md").exists());

    let dry = Command::new(bin)
        .args([
            "--repo",
            repo.to_str().unwrap(),
            "--dry-run",
            "--format",
            "site",
        ])
        .output()
        .unwrap();
    assert!(dry.status.success());
    assert!(String::from_utf8_lossy(&dry.stdout).contains("nothing written"));
    assert!(!repo.join(".trove/ship").exists());

    let claude = repo.join("CLAUDE.md");
    let mut text = fs::read_to_string(&claude).unwrap();
    text.push_str(&format!(
        "- {}\n",
        ["sk\\_live\\_", "9xY8wV7uT6sR5qP4oN3mL2kJ"].concat()
    ));
    fs::write(&claude, text).unwrap();
    let failed = Command::new(bin)
        .args([
            "--repo",
            repo.to_str().unwrap(),
            "--out",
            out.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(failed.status.code(), Some(2));
    assert!(String::from_utf8_lossy(&failed.stderr).contains("refusing to write"));

    let _ = fs::remove_dir_all(repo.parent().unwrap());
}
