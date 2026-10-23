//! The `trove ship` front door shares one implementation with the
//! `trove-ship` binary: both flatten `trove_ship::cli::ShipCli` and call
//! `run`. These tests exercise that shared path on a temp copy of the seeded
//! fixture: dry runs succeed deterministically without writing, and a
//! surviving secret fails closed (exit 2) with nothing written.

use std::path::{Path, PathBuf};

use clap::Parser;
use trove_ship::cli::{run, ShipCli};

fn fixture_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../trove-ship/tests/fixtures/seeded")
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

fn materialize(name: &str) -> PathBuf {
    let tmp = std::env::temp_dir().join(format!("trove-ship-subcmd-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    copy_dir(&fixture_root(), &tmp);
    tmp
}

#[test]
fn dry_run_exits_zero_deterministically_and_writes_nothing() {
    let repo = materialize("dry");
    // Both front doors parse to the identical ShipCli the subcommand uses.
    let via_subcommand =
        ShipCli::parse_from(["trove", "--repo", repo.to_str().unwrap(), "--dry-run"]);
    let via_binary =
        ShipCli::parse_from(["trove-ship", "--repo", repo.to_str().unwrap(), "--dry-run"]);
    assert_eq!(format!("{via_subcommand:?}"), format!("{via_binary:?}"));

    assert_eq!(run(&via_subcommand, "trove ship"), 0);
    assert_eq!(run(&via_binary, "trove-ship"), 0);
    assert!(
        !repo.join(".trove/ship").exists(),
        "dry runs must not write output"
    );
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn list_detectors_exits_zero() {
    let repo = materialize("detectors");
    let args = ShipCli::parse_from([
        "trove-ship",
        "--repo",
        repo.to_str().unwrap(),
        "--list-detectors",
    ]);
    assert_eq!(run(&args, "trove ship"), 0);
    let _ = std::fs::remove_dir_all(&repo);
}

#[test]
fn surviving_secret_fails_closed_exits_two_and_writes_nothing() {
    let repo = materialize("failclosed");
    // A Markdown-escaped key slips past the detectors but renders as a real
    // key in output — verification must fail closed. Same trick as
    // trove-ship's ship_fixture tests; assembled at runtime.
    let escaped = ["sk\\_live\\_", "9xY8wV7uT6sR5qP4oN3mL2kJ"].concat();
    let claude = repo.join("CLAUDE.md");
    let mut text = std::fs::read_to_string(&claude).unwrap();
    text.push_str(&format!("- Old key, rotated: {escaped}\n"));
    std::fs::write(&claude, text).unwrap();

    let args = ShipCli::parse_from(["trove", "--repo", repo.to_str().unwrap()]);
    assert_eq!(run(&args, "trove ship"), 2, "fail-closed is exit code 2");
    assert!(
        !repo.join(".trove/ship").exists(),
        "nothing may be written on failure"
    );
    let _ = std::fs::remove_dir_all(&repo);
}
