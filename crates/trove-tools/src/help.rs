//! Summaries from `--help` output, and a guarded way to collect it.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// Cap on captured help output.
const HELP_LIMIT: u64 = 64 * 1024;
const MAX_SUMMARY: usize = 160;

/// Names never executed during probing: they act on the system, start an
/// interactive session, or loop forever on unknown flags.
const NEVER_PROBE: &[&str] = &[
    "yes", "reboot", "shutdown", "halt", "poweroff", "init", "telinit", "kexec", "sudo", "doas",
    "su", "login", "logout", "passwd", "nologin", "dd", "fdisk", "sfdisk", "gdisk", "parted",
    "wipefs", "mkswap", "swapoff", "swapon", "sh", "bash", "zsh", "fish", "dash", "ksh", "csh",
    "tcsh", "screen", "tmux", "top", "htop", "vi", "vim", "nvim", "emacs", "nano", "less", "more",
    "watch",
];

/// Whether `--help` may be run for this tool name.
pub fn probe_allowed(name: &str) -> bool {
    !(NEVER_PROBE.contains(&name)
        || name.starts_with("mkfs")
        || name.starts_with("fsck")
        || name.starts_with('.'))
}

/// Run `<path> --help` with no stdin, pagers disabled, and a timeout.
/// Returns combined stdout and stderr, or `None` if nothing useful came back.
///
/// Some programs ignore `--help` and do their normal work, so the child runs
/// in an empty scratch directory with `HOME` and the XDG dirs pointed there.
/// Whatever it writes lands in the sandbox, which is removed afterwards.
pub fn probe_help(path: &Path, timeout: Duration) -> Option<String> {
    let name = path.file_name()?.to_string_lossy().into_owned();
    if !probe_allowed(&name) {
        return None;
    }
    let sandbox = Sandbox::new()?;
    let mut cmd = Command::new(path);
    cmd.arg("--help")
        .current_dir(&sandbox.0)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env("HOME", &sandbox.0)
        .env("XDG_CONFIG_HOME", sandbox.0.join(".config"))
        .env("XDG_CACHE_HOME", sandbox.0.join(".cache"))
        .env("XDG_DATA_HOME", sandbox.0.join(".local/share"))
        .env("XDG_STATE_HOME", sandbox.0.join(".local/state"))
        .env("PAGER", "cat")
        .env("GIT_PAGER", "cat")
        .env("MANPAGER", "cat")
        .env("NO_COLOR", "1")
        .env("TERM", "dumb");
    // rustup proxies (cargo, rustc) locate toolchains through HOME; keep
    // them pointed at the real install so they can print help.
    if let Some(home) = std::env::var_os("HOME").map(std::path::PathBuf::from) {
        for (var, rel) in [("RUSTUP_HOME", ".rustup"), ("CARGO_HOME", ".cargo")] {
            if std::env::var_os(var).is_none() && home.join(rel).is_dir() {
                cmd.env(var, home.join(rel));
            }
        }
    }
    let mut child = cmd.spawn().ok()?;

    // Readers report over channels so a grandchild that keeps a pipe open
    // after the timeout cannot hang us.
    let out_rx = spawn_reader(child.stdout.take()?);
    let err_rx = spawn_reader(child.stderr.take()?);

    let start = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if start.elapsed() < timeout => std::thread::sleep(Duration::from_millis(15)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let grace = Duration::from_millis(300);
    let mut buf = out_rx.recv_timeout(grace).unwrap_or_default();
    if buf.iter().all(u8::is_ascii_whitespace) {
        buf = err_rx.recv_timeout(grace).unwrap_or_default();
    }
    let text = String::from_utf8_lossy(&buf).into_owned();
    (!text.trim().is_empty()).then_some(text)
}

/// A scratch directory removed on drop.
struct Sandbox(std::path::PathBuf);

impl Sandbox {
    fn new() -> Option<Self> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static N: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "trove-probe-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::SeqCst)
        ));
        std::fs::create_dir_all(&dir).ok()?;
        Some(Self(dir))
    }
}

impl Drop for Sandbox {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn spawn_reader<R: Read + Send + 'static>(mut pipe: R) -> std::sync::mpsc::Receiver<Vec<u8>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = (&mut pipe).take(HELP_LIMIT).read_to_end(&mut buf);
        let _ = tx.send(buf);
    });
    rx
}

/// Pick a one-line description out of `--help` output.
///
/// Skips the usage block, version banners, author lines, option tables and
/// section headers, then takes the first prose paragraph up to its first
/// sentence end.
pub fn parse_help_summary(help: &str, name: &str) -> Option<String> {
    let text = strip_ansi(help);
    let lines: Vec<&str> = text.lines().collect();
    let mut i = 0;
    while i < lines.len() {
        let raw = lines[i];
        let line = raw.trim();
        i += 1;
        if line.is_empty() {
            continue;
        }
        let lower = line.to_lowercase();
        if lower.starts_with("usage") {
            // Swallow continuation lines of the usage block.
            while i < lines.len() {
                let next = lines[i];
                let t = next.trim_start().to_lowercase();
                if next.starts_with(char::is_whitespace) && !next.trim().is_empty()
                    || t.starts_with("or:")
                {
                    i += 1;
                } else {
                    break;
                }
            }
            continue;
        }
        if raw.starts_with(char::is_whitespace) || !looks_like_prose(line, name) {
            continue;
        }
        // Join the rest of this paragraph.
        let mut para = line.to_string();
        while i < lines.len() {
            let next = lines[i].trim();
            if next.is_empty()
                || lines[i].starts_with(char::is_whitespace)
                || next.ends_with(':')
                || next.to_lowercase().starts_with("usage")
                || next.starts_with('-')
                || next.contains("   ")
            {
                break;
            }
            para.push(' ');
            para.push_str(next);
            i += 1;
        }
        return Some(first_sentence(&strip_name_prefix(&para, name)));
    }
    None
}

/// Phrases that mark error output, pointers elsewhere, or boilerplate
/// rather than a description.
const NOT_A_SUMMARY: &[&str] = &[
    "unknown option",
    "invalid option",
    "illegal option",
    "unrecognized option",
    "unrecognised option",
    "unknown argument",
    "unknown command",
    "unable to",
    "no such file",
    "can't",
    "cannot",
    "error:",
    "warning:",
    "not found",
    "--help",
    "for more details",
    "for more information",
    "for more help",
    "supported targets",
    "free software",
    "copyright",
    "all rights reserved",
    "no warranty",
    "failed",
    "aborted",
    "try '",
    "see '",
];

fn looks_like_prose(line: &str, name: &str) -> bool {
    let words: Vec<&str> = line.split_whitespace().collect();
    if words.len() < 3 {
        return false;
    }
    if line.starts_with(|c: char| "-[<=*/#|$>".contains(c)) || line.ends_with(':') {
        return false;
    }
    let lower = line.to_lowercase();
    if lower.contains("http://") || lower.contains("https://") {
        return false;
    }
    if NOT_A_SUMMARY.iter().any(|p| lower.contains(p)) {
        return false;
    }
    if ["version", "options", "valid ", "use:"]
        .iter()
        .any(|p| lower.starts_with(p))
    {
        return false;
    }
    // Option tables and aligned columns: prose rarely has wide gaps.
    if line.contains("   ") || line.contains(" : ") {
        return false;
    }
    // Status output with symbols such as check marks.
    if line
        .chars()
        .any(|c| c as u32 >= 0x2190 && !c.is_alphanumeric())
    {
        return false;
    }
    // Author lines: "Andrew Gallant <jamslam@gmail.com>".
    if line.contains('@') && line.contains('<') {
        return false;
    }
    // Synopsis lines: "fonttosfnt [ -v ] [ -c ] ...".
    let first = words[0].to_lowercase();
    let name = name.to_lowercase();
    if first.starts_with(&name) && (line.contains('[') || line.contains(" <")) {
        return false;
    }
    // Version banners: "ripgrep 14.1.0", "git version 2.44", "LibreOffice 24.2.7.2 420(Build:2)".
    if words.len() <= 6 && words.iter().any(|w| is_version(w)) {
        return false;
    }
    line.chars().any(|c| c.is_alphabetic())
}

fn is_version(word: &str) -> bool {
    let w = word.trim_start_matches(|c: char| !c.is_ascii_digit());
    let mut parts = w.split('.');
    let major = parts.next().unwrap_or("");
    let minor = parts.next().unwrap_or("");
    !major.is_empty()
        && major.chars().all(|c| c.is_ascii_digit())
        && minor.starts_with(|c: char| c.is_ascii_digit())
}

fn strip_name_prefix(para: &str, name: &str) -> String {
    for sep in [" - ", ": ", " -- "] {
        if let Some(rest) = para.strip_prefix(&format!("{name}{sep}")) {
            return rest.to_string();
        }
    }
    for label in [
        "OVERVIEW:",
        "DESCRIPTION:",
        "Description:",
        "SUMMARY:",
        "Summary:",
    ] {
        if let Some(rest) = para.strip_prefix(label) {
            return rest.trim_start().to_string();
        }
    }
    para.to_string()
}

fn first_sentence(para: &str) -> String {
    let cut = para
        .find(". ")
        .map(|i| &para[..i + 1])
        .unwrap_or(para)
        .trim();
    truncate(cut, MAX_SUMMARY)
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max.saturating_sub(3)).collect();
    out = out.trim_end().to_string();
    out.push_str("...");
    out
}

fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for c in chars.by_ref() {
                    if c.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        // Backspace overstrike used by some help pagers: "N\bNA\bAM\bME".
        if c == '\u{8}' {
            out.pop();
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clap_style_description_first() {
        let help = "Rust's package manager\n\nUsage: cargo [+toolchain] [OPTIONS] [COMMAND]\n\nOptions:\n  -V, --version  Print version info\n";
        assert_eq!(
            parse_help_summary(help, "cargo").as_deref(),
            Some("Rust's package manager")
        );
    }

    #[test]
    fn gnu_style_usage_then_description() {
        let help = "Usage: ls [OPTION]... [FILE]...\nList information about the FILEs (the current directory by default).\nSort entries alphabetically if none of -cftuvSUX nor --sort is specified.\n\nMandatory arguments to long options are mandatory for short options too.\n  -a, --all                  do not ignore entries starting with .\n";
        assert_eq!(
            parse_help_summary(help, "ls").as_deref(),
            Some("List information about the FILEs (the current directory by default).")
        );
    }

    #[test]
    fn skips_banner_and_author() {
        let help = "ripgrep 14.1.0\n\nAndrew Gallant <jamslam@gmail.com>\n\nripgrep (rg) recursively searches the current directory for a regex pattern.\nBy default, ripgrep will respect gitignore rules.\n\nUSAGE:\n    rg [OPTIONS] PATTERN [PATH ...]\n";
        assert_eq!(
            parse_help_summary(help, "rg").as_deref(),
            Some("ripgrep (rg) recursively searches the current directory for a regex pattern.")
        );
    }

    #[test]
    fn multi_line_usage_block() {
        let help = "Usage:\tjq [OPTIONS] FILTER [FILES...]\n\tjq [OPTIONS] --args FILTER [ARGUMENTS...]\n\njq is a tool for processing JSON inputs, applying the given filter to\nits JSON text inputs and producing the filter's results as JSON on\nstandard output.\n\nExample:\n\n\t$ echo '{\"foo\": 0}' | jq .\n";
        assert_eq!(
            parse_help_summary(help, "jq").as_deref(),
            Some("jq is a tool for processing JSON inputs, applying the given filter to its JSON text inputs and producing the filter's results as JSON on standard output.")
        );
    }

    #[test]
    fn strips_name_prefix_and_ansi() {
        let help = "\u{1b}[1mfd\u{1b}[0m - A simple, fast and user-friendly alternative to find\n\nUsage: fd [OPTIONS]\n";
        assert_eq!(
            parse_help_summary(help, "fd").as_deref(),
            Some("A simple, fast and user-friendly alternative to find")
        );
    }

    #[test]
    fn nothing_useful() {
        assert_eq!(parse_help_summary("", "x"), None);
        assert_eq!(
            parse_help_summary("usage: x [-a] [-b]\n  -a  alpha\n  -b  beta\n", "x"),
            None
        );
        assert_eq!(parse_help_summary("x version 1.2.3\n", "x"), None);
    }

    #[test]
    fn rejects_error_output_and_banners() {
        let cases = [
            (
                "Unknown option: help\nUsage: /usr/bin/json_pp [-V]\n",
                "json_pp",
            ),
            ("/usr/bin/pg_buildext: illegal option -- -\n", "pg_buildext"),
            (
                "prtstat: unrecognized option '--help'\nInvalid option\n",
                "prtstat",
            ),
            (
                "unable to open image `--help': No such file or directory\n",
                "identify",
            ),
            ("LibreOffice 24.2.7.2 420(Build:2)\n", "loweb"),
            ("Debian GNU/Linux ucfr Revision 3.00.\n", "ucfr"),
            (
                "fonttosfnt [ -v ] [ -c ] -o font.ttf font ...\n",
                "fonttosfnt",
            ),
            ("    file   : program read from script file\n", "python"),
            ("=================== Python ===================\n", "check"),
            ("For more details see umount(8).\n", "umount"),
        ];
        for (help, name) in cases {
            assert_eq!(parse_help_summary(help, name), None, "{name}");
        }
    }

    #[test]
    fn strips_overview_label() {
        let help =
            "OVERVIEW: llvm object file dumper\n\nUSAGE: llvm-objdump [options] <input files>\n";
        assert_eq!(
            parse_help_summary(help, "llvm-objdump").as_deref(),
            Some("llvm object file dumper")
        );
    }

    #[test]
    fn long_summaries_are_truncated() {
        let long = format!("{} end", "word ".repeat(60));
        let s = parse_help_summary(&long, "x").unwrap();
        assert!(s.chars().count() <= MAX_SUMMARY);
        assert!(s.ends_with("..."));
    }

    #[test]
    fn denylist() {
        assert!(!probe_allowed("reboot"));
        assert!(!probe_allowed("mkfs.ext4"));
        assert!(!probe_allowed("yes"));
        assert!(probe_allowed("rg"));
    }

    #[cfg(unix)]
    #[test]
    fn probes_a_script_with_timeout() {
        use std::os::unix::fs::PermissionsExt;
        let dir = crate::test_dir("probe");
        let fast = dir.join("fasttool");
        std::fs::write(
            &fast,
            "#!/bin/sh\necho 'Usage: fasttool [opts]'\necho\necho 'Fast tool that prints things.'\n",
        )
        .unwrap();
        std::fs::set_permissions(&fast, std::fs::Permissions::from_mode(0o755)).unwrap();
        let out = probe_help(&fast, Duration::from_secs(5)).unwrap();
        assert_eq!(
            parse_help_summary(&out, "fasttool").as_deref(),
            Some("Fast tool that prints things.")
        );

        // A tool that ignores --help and writes files must not touch our cwd or HOME.
        let writer = dir.join("writertool");
        std::fs::write(
            &writer,
            "#!/bin/sh\ntouch ./left-behind \"$HOME/dotfile\"\necho \"$PWD\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&writer, std::fs::Permissions::from_mode(0o755)).unwrap();
        let ran_in = probe_help(&writer, Duration::from_secs(5)).unwrap();
        let ran_in = std::path::PathBuf::from(ran_in.trim());
        assert!(ran_in
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("trove-probe-"));
        assert!(!ran_in.exists(), "sandbox removed");
        assert!(!std::env::current_dir()
            .unwrap()
            .join("left-behind")
            .exists());

        let slow = dir.join("slowtool");
        std::fs::write(&slow, "#!/bin/sh\nexec sleep 30\n").unwrap();
        std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        let start = Instant::now();
        assert_eq!(probe_help(&slow, Duration::from_millis(200)), None);
        assert!(start.elapsed() < Duration::from_secs(10));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
