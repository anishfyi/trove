//! Runs the pi-shaped stdio smoke script (mcp_stdio_smoke.sh) against the
//! real binary: raw pipes, no test harness framing.

use std::path::Path;
use std::process::Command;

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

#[test]
fn pi_shaped_stdio_smoke() {
    let dir = std::env::temp_dir().join(format!("trove-mcp-smoke-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let home = dir.join("home");
    std::fs::create_dir_all(&home).unwrap();
    let repo = dir.join("fixture");
    copy_dir(
        &Path::new(env!("CARGO_MANIFEST_DIR")).join("../../evals/fixture"),
        &repo,
    );
    let out = Command::new("bash")
        .arg(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/mcp_stdio_smoke.sh"
        ))
        .arg(env!("CARGO_BIN_EXE_trove"))
        .arg(&repo)
        .arg(&home)
        .output()
        .unwrap();
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "smoke failed:\nstdout: {stdout}\nstderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("6 checks passed"), "{stdout}");
    let _ = std::fs::remove_dir_all(&dir);
}
