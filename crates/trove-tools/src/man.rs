//! One-line summaries from man pages: the NAME section of man(7) and mdoc(7)
//! sources, plain or gzip-compressed. Pages are read directly rather than
//! through `whatis`, whose database is often stale or missing.

use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

/// Sections that document commands.
const COMMAND_SECTIONS: &[&str] = &["man1", "man8", "man6"];
/// Only the head of a page is needed to find NAME.
const READ_LIMIT: u64 = 16 * 1024;

/// Map of command name to its man page, built from a list of man roots.
#[derive(Debug, Default)]
pub struct ManIndex {
    pages: HashMap<String, PathBuf>,
}

impl ManIndex {
    pub fn build(roots: &[PathBuf]) -> Self {
        let mut pages = HashMap::new();
        for root in roots {
            for section in COMMAND_SECTIONS {
                let Ok(rd) = std::fs::read_dir(root.join(section)) else {
                    continue;
                };
                for entry in rd.flatten() {
                    let file = entry.file_name();
                    if let Some(name) = page_command_name(&file.to_string_lossy()) {
                        pages.entry(name).or_insert_with(|| entry.path());
                    }
                }
            }
        }
        Self { pages }
    }

    pub fn len(&self) -> usize {
        self.pages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pages.is_empty()
    }

    pub fn page(&self, name: &str) -> Option<&Path> {
        self.pages.get(name).map(PathBuf::as_path)
    }

    /// Summary line for a command, following one `.so` redirect.
    pub fn summary(&self, name: &str) -> Option<String> {
        let page = self.page(name)?;
        let text = read_page(page)?;
        if let Some(target) = so_redirect(&text) {
            // `.so man1/other.1` is relative to the man root.
            let root = page.parent()?.parent()?;
            let redirected = read_page(&root.join(target)).or_else(|| {
                let gz = root.join(format!("{target}.gz"));
                read_page(&gz)
            })?;
            return parse_name_section(&redirected);
        }
        parse_name_section(&text)
    }
}

/// `git-log.1.gz` -> `git-log`, `openssl.1ssl` -> `openssl`.
pub fn page_command_name(file: &str) -> Option<String> {
    let base = file
        .strip_suffix(".gz")
        .or_else(|| file.strip_suffix(".bz2"))
        .or_else(|| file.strip_suffix(".xz"))
        .or_else(|| file.strip_suffix(".zst"))
        .unwrap_or(file);
    let (name, section) = base.rsplit_once('.')?;
    if name.is_empty() || !section.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(name.to_string())
}

fn read_page(path: &Path) -> Option<String> {
    let file = File::open(path).ok()?;
    let mut buf = Vec::new();
    if path.extension().and_then(|e| e.to_str()) == Some("gz") {
        flate2::read::GzDecoder::new(file)
            .take(READ_LIMIT)
            .read_to_end(&mut buf)
            .ok()?;
    } else if path
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| matches!(e, "bz2" | "xz" | "zst"))
    {
        return None;
    } else {
        file.take(READ_LIMIT).read_to_end(&mut buf).ok()?;
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

fn so_redirect(text: &str) -> Option<&str> {
    let first = text
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty() && !l.starts_with(".\\\"") && !l.starts_with("'\\\""))?;
    first.strip_prefix(".so ").map(str::trim)
}

/// Extract the description from a page's NAME section.
///
/// man(7): `.SH NAME` then `ls \- list directory contents`.
/// mdoc(7): `.Sh NAME` then `.Nm ls` and `.Nd list directory contents`.
pub fn parse_name_section(text: &str) -> Option<String> {
    let mut in_name = false;
    let mut collected: Vec<String> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end();
        if let Some(heading) = section_heading(line) {
            if in_name {
                break;
            }
            in_name = heading.eq_ignore_ascii_case("NAME");
            continue;
        }
        if !in_name {
            continue;
        }
        if let Some(nd) = line.strip_prefix(".Nd ") {
            return non_empty(clean_roff(nd));
        }
        if line.starts_with(".\\\"") || line.starts_with("'\\\"") {
            continue;
        }
        if let Some(rest) = line.strip_prefix('.') {
            // Font macros like `.B ls` carry text; other requests do not.
            let mut parts = rest.splitn(2, ' ');
            let mac = parts.next().unwrap_or("");
            if matches!(mac, "B" | "I" | "BR" | "IR" | "RB" | "RI") {
                if let Some(t) = parts.next() {
                    collected.push(t.replace('"', ""));
                }
            }
            continue;
        }
        collected.push(line.to_string());
    }
    let joined = clean_roff(&collected.join(" "));
    // After cleanup `\-` became a plain hyphen surrounded by spaces.
    let desc = joined
        .split_once(" - ")
        .map(|(_, d)| d)
        .or_else(|| joined.split_once(" -- ").map(|(_, d)| d))?;
    non_empty(desc.trim().to_string())
}

fn section_heading(line: &str) -> Option<&str> {
    let rest = line
        .strip_prefix(".SH")
        .or_else(|| line.strip_prefix(".Sh"))?;
    if !rest.is_empty() && !rest.starts_with(' ') {
        return None;
    }
    Some(rest.trim().trim_matches('"'))
}

fn non_empty(s: String) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// Strip common roff escapes and font changes.
pub fn clean_roff(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('-') => out.push('-'),
            Some('e') | Some('\\') => out.push('\\'),
            Some(' ') | Some('~') => out.push(' '),
            Some('&') | Some('|') | Some('^') | Some('%') | Some(',') | Some('/') => {}
            Some('f') => match chars.next() {
                Some('(') => {
                    chars.next();
                    chars.next();
                }
                Some('[') => {
                    for c in chars.by_ref() {
                        if c == ']' {
                            break;
                        }
                    }
                }
                _ => {}
            },
            Some('(') => {
                let a = chars.next().unwrap_or(' ');
                let b = chars.next().unwrap_or(' ');
                out.push_str(special_char(&format!("{a}{b}")));
            }
            Some('[') => {
                let mut name = String::new();
                for c in chars.by_ref() {
                    if c == ']' {
                        break;
                    }
                    name.push(c);
                }
                out.push_str(special_char(&name));
            }
            Some('*') => {
                // String register, e.g. \*(lq or \*R; drop it.
                match chars.next() {
                    Some('(') => {
                        chars.next();
                        chars.next();
                    }
                    Some('[') => {
                        for c in chars.by_ref() {
                            if c == ']' {
                                break;
                            }
                        }
                    }
                    _ => {}
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    // mdoc inline macros that leak into .Nd text.
    out.replace(" Ns ", "")
}

fn special_char(name: &str) -> &'static str {
    match name {
        "em" | "en" | "hy" | "mi" => "-",
        "aq" | "cq" | "oq" => "'",
        "dq" | "lq" | "rq" => "\"",
        "bu" => "*",
        _ => "",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_names() {
        assert_eq!(page_command_name("ls.1"), Some("ls".into()));
        assert_eq!(page_command_name("git-log.1.gz"), Some("git-log".into()));
        assert_eq!(page_command_name("openssl.1ssl"), Some("openssl".into()));
        assert_eq!(page_command_name("node.1.bz2"), Some("node".into()));
        assert_eq!(page_command_name("README"), None);
        assert_eq!(page_command_name("index.db"), None);
    }

    #[test]
    fn parses_man7_name() {
        let src = ".TH LS 1\n.SH NAME\nls \\- list directory contents\n.SH SYNOPSIS\n.B ls\n";
        assert_eq!(
            parse_name_section(src).as_deref(),
            Some("list directory contents")
        );
    }

    #[test]
    fn parses_quoted_heading_and_fonts() {
        // asciidoc output, as shipped by git.
        let src = ".TH \"GIT\" \"1\"\n.SH \"NAME\"\ngit \\- the stupid content tracker\n.SH \"SYNOPSIS\"\n";
        assert_eq!(
            parse_name_section(src).as_deref(),
            Some("the stupid content tracker")
        );
        let src = ".SH NAME\n\\fBjq\\fR \\- Command\\-line JSON processor\n.SH SYNOPSIS\n";
        assert_eq!(
            parse_name_section(src).as_deref(),
            Some("Command-line JSON processor")
        );
    }

    #[test]
    fn parses_multi_name_and_wrapped_lines() {
        let src = ".SH NAME\ngzip, gunzip, zcat \\- compress or\nexpand files\n.SH SYNOPSIS\n";
        assert_eq!(
            parse_name_section(src).as_deref(),
            Some("compress or expand files")
        );
    }

    #[test]
    fn parses_mdoc_nd() {
        let src = ".Dd May 1, 2020\n.Dt LS 1\n.Os\n.Sh NAME\n.Nm ls\n.Nd list directory contents\n.Sh SYNOPSIS\n";
        assert_eq!(
            parse_name_section(src).as_deref(),
            Some("list directory contents")
        );
    }

    #[test]
    fn missing_name_section() {
        assert_eq!(parse_name_section(".TH X 1\n.SH SYNOPSIS\nx\n"), None);
        assert_eq!(
            parse_name_section(".SH NAME\nonlyname\n.SH SYNOPSIS\n"),
            None
        );
    }

    #[test]
    fn roff_escapes() {
        assert_eq!(clean_roff("a\\(emb \\fIit\\fP \\[aq]q\\[aq]"), "a-b it 'q'");
        assert_eq!(clean_roff("x\\&y\\e"), "xy\\");
    }

    #[test]
    fn index_reads_plain_gzip_and_so_pages() {
        use std::io::Write;
        let root = crate::test_dir("man-index");
        let man1 = root.join("man1");
        std::fs::create_dir_all(&man1).unwrap();
        std::fs::write(man1.join("plain.1"), ".SH NAME\nplain \\- a plain page\n").unwrap();
        let gz = File::create(man1.join("zipped.1.gz")).unwrap();
        let mut enc = flate2::write::GzEncoder::new(gz, flate2::Compression::fast());
        enc.write_all(b".Sh NAME\n.Nm zipped\n.Nd a gzip page\n")
            .unwrap();
        enc.finish().unwrap();
        std::fs::write(man1.join("alias.1"), ".so man1/plain.1\n").unwrap();

        let idx = ManIndex::build(std::slice::from_ref(&root));
        assert_eq!(idx.len(), 3);
        assert_eq!(idx.summary("plain").as_deref(), Some("a plain page"));
        assert_eq!(idx.summary("zipped").as_deref(), Some("a gzip page"));
        assert_eq!(idx.summary("alias").as_deref(), Some("a plain page"));
        assert_eq!(idx.summary("absent"), None);
        let _ = std::fs::remove_dir_all(&root);
    }
}
