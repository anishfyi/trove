//! A small, dependency-free Markdown to HTML renderer covering what the handoff
//! docs use: headings, paragraphs, lists, tables, block quotes, code fences,
//! inline code, bold, italics and links. All text is HTML-escaped.

pub fn markdown_to_html(md: &str) -> String {
    let lines: Vec<&str> = md.lines().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        let t = line.trim();

        if t.starts_with("```") {
            let lang = t.trim_start_matches('`').trim();
            let mut code = String::new();
            i += 1;
            while i < lines.len() && !lines[i].trim_start().starts_with("```") {
                code.push_str(lines[i]);
                code.push('\n');
                i += 1;
            }
            i += 1;
            let class = if lang.is_empty() {
                String::new()
            } else {
                format!(" class=\"language-{}\"", escape(lang))
            };
            out.push_str(&format!(
                "<pre><code{class}>{}</code></pre>\n",
                escape(&code)
            ));
            continue;
        }
        if t.is_empty() {
            i += 1;
            continue;
        }
        if let Some((level, text)) = heading(t) {
            let id = slug(text);
            out.push_str(&format!(
                "<h{level} id=\"{id}\">{}</h{level}>\n",
                inline(text)
            ));
            i += 1;
            continue;
        }
        if t == "---" || t == "***" {
            out.push_str("<hr>\n");
            i += 1;
            continue;
        }
        if t.starts_with('|') {
            let mut rows = Vec::new();
            while i < lines.len() && lines[i].trim().starts_with('|') {
                rows.push(lines[i].trim());
                i += 1;
            }
            out.push_str(&table(&rows));
            continue;
        }
        if t.starts_with('>') {
            let mut quote = String::new();
            while i < lines.len() && lines[i].trim().starts_with('>') {
                quote.push_str(lines[i].trim().trim_start_matches('>').trim_start());
                quote.push('\n');
                i += 1;
            }
            out.push_str(&format!(
                "<blockquote>\n{}</blockquote>\n",
                markdown_to_html(&quote)
            ));
            continue;
        }
        if let Some(ordered) = list_marker(t) {
            let tag = if ordered { "ol" } else { "ul" };
            out.push_str(&format!("<{tag}>\n"));
            while i < lines.len() {
                let lt = lines[i].trim();
                match list_marker(lt) {
                    Some(o) if o == ordered => {
                        let item = strip_marker(lt);
                        out.push_str(&format!("<li>{}</li>\n", inline(item)));
                        i += 1;
                    }
                    _ => break,
                }
            }
            out.push_str(&format!("</{tag}>\n"));
            continue;
        }
        let mut para = Vec::new();
        while i < lines.len() {
            let pt = lines[i].trim();
            if pt.is_empty()
                || pt.starts_with("```")
                || heading(pt).is_some()
                || pt.starts_with('|')
                || pt.starts_with('>')
                || list_marker(pt).is_some()
            {
                break;
            }
            para.push(pt);
            i += 1;
        }
        out.push_str(&format!("<p>{}</p>\n", inline(&para.join(" "))));
    }
    out
}

fn heading(t: &str) -> Option<(usize, &str)> {
    let n = t.chars().take_while(|&c| c == '#').count();
    ((1..=6).contains(&n) && t[n..].starts_with(' ')).then(|| (n, t[n..].trim()))
}

fn list_marker(t: &str) -> Option<bool> {
    if t.starts_with("- ") || t.starts_with("* ") {
        return Some(false);
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    (digits > 0 && t[digits..].starts_with(". ")).then_some(true)
}

fn strip_marker(t: &str) -> &str {
    if t.starts_with("- ") || t.starts_with("* ") {
        return &t[2..];
    }
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    &t[digits + 2..]
}

fn split_row(row: &str) -> Vec<String> {
    let inner = row.trim().trim_start_matches('|').trim_end_matches('|');
    let mut cells = Vec::new();
    let mut cur = String::new();
    let mut chars = inner.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' if chars.peek() == Some(&'|') => {
                cur.push('|');
                chars.next();
            }
            '|' => {
                cells.push(cur.trim().to_string());
                cur.clear();
            }
            _ => cur.push(c),
        }
    }
    cells.push(cur.trim().to_string());
    cells
}

fn table(rows: &[&str]) -> String {
    let is_sep = |r: &str| r.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let mut out = String::from("<table>\n");
    let has_header = rows.len() > 1 && is_sep(rows[1]);
    for (idx, row) in rows.iter().enumerate() {
        if has_header && idx == 1 {
            continue;
        }
        let tag = if has_header && idx == 0 { "th" } else { "td" };
        out.push_str("<tr>");
        for cell in split_row(row) {
            out.push_str(&format!("<{tag}>{}</{tag}>", inline(&cell)));
        }
        out.push_str("</tr>\n");
    }
    out.push_str("</table>\n");
    out
}

/// Inline formatting. Code spans are cut out first so nothing inside them is
/// interpreted.
pub fn inline(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find('`') {
        let Some(len) = rest[start + 1..].find('`') else {
            break;
        };
        out.push_str(&emphasis(&rest[..start]));
        out.push_str(&format!(
            "<code>{}</code>",
            escape(&rest[start + 1..start + 1 + len])
        ));
        rest = &rest[start + 2 + len..];
    }
    out.push_str(&emphasis(rest));
    out
}

fn emphasis(text: &str) -> String {
    let s = links(text);
    let s = pair(&s, "**", "strong");
    pair_underscore(&s)
}

/// `[label](target)` links. Labels and targets are escaped; only relative,
/// anchor and http(s) targets are kept, and `.md` links point at `.html`.
fn links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close_rel) = rest[open..].find("](") else {
            break;
        };
        let close = open + close_rel;
        let Some(end_rel) = rest[close + 2..].find(')') else {
            break;
        };
        let end = close + 2 + end_rel;
        let label = &rest[open + 1..close];
        let target = &rest[close + 2..end];
        if label.contains('[') {
            out.push_str(&escape(&rest[..open + 1]));
            rest = &rest[open + 1..];
            continue;
        }
        out.push_str(&escape(&rest[..open]));
        let safe = target.starts_with("http://")
            || target.starts_with("https://")
            || target.starts_with('#')
            || !(target.contains(':') || target.starts_with("//"));
        if safe {
            let href = match target.strip_suffix(".md") {
                Some(stem) if !target.contains("://") => format!("{stem}.html"),
                _ => target.to_string(),
            };
            out.push_str(&format!(
                "<a href=\"{}\">{}</a>",
                escape(&href),
                escape(label)
            ));
        } else {
            out.push_str(&escape(&rest[open..=end]));
        }
        rest = &rest[end + 1..];
    }
    out.push_str(&escape(rest));
    out
}

fn pair(text: &str, marker: &str, tag: &str) -> String {
    let parts: Vec<&str> = text.split(marker).collect();
    if parts.len() < 3 {
        return text.to_string();
    }
    let mut out = String::new();
    let pairs = (parts.len() - 1) / 2;
    for (i, p) in parts.iter().enumerate() {
        out.push_str(p);
        if i + 1 < parts.len() {
            if i < pairs * 2 {
                let html = if i % 2 == 0 {
                    format!("<{tag}>")
                } else {
                    format!("</{tag}>")
                };
                out.push_str(&html);
            } else {
                out.push_str(marker);
            }
        }
    }
    out
}

/// `_italic_` only when the underscores sit on word edges, so `snake_case`
/// identifiers are left alone.
fn pair_underscore(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '_' && (i == 0 || !chars[i - 1].is_alphanumeric()) {
            if let Some(off) = chars[i + 1..].iter().position(|&c| c == '_') {
                let j = i + 1 + off;
                let after_ok = j + 1 >= chars.len() || !chars[j + 1].is_alphanumeric();
                if j > i + 1 && after_ok {
                    let inner: String = chars[i + 1..j].iter().collect();
                    out.push_str(&format!("<em>{inner}</em>"));
                    i = j + 1;
                    continue;
                }
            }
        }
        out.push(chars[i]);
        i += 1;
    }
    out
}

pub fn escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

pub fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.to_lowercase().chars() {
        if c.is_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_blocks() {
        let html = markdown_to_html(
            "# Title\n\nPara **bold** and `<code>`.\n\n- a\n- b\n\n1. one\n\n```sh\necho <hi>\n```\n\n| A | B |\n|---|---|\n| x \\| y | z |\n\n> quote\n",
        );
        assert!(html.contains("<h1 id=\"title\">Title</h1>"));
        assert!(html.contains("<strong>bold</strong>"));
        assert!(html.contains("<code>&lt;code&gt;</code>"));
        assert!(html.contains("<ul>\n<li>a</li>\n<li>b</li>\n</ul>"));
        assert!(html.contains("<ol>\n<li>one</li>\n</ol>"));
        assert!(html.contains("<pre><code class=\"language-sh\">echo &lt;hi&gt;\n</code></pre>"));
        assert!(html.contains("<th>A</th>"));
        assert!(html.contains("<td>x | y</td>"));
        assert!(html.contains("<blockquote>"));
    }

    #[test]
    fn escapes_html_and_unsafe_links() {
        let html = inline("<script>x</script> [ok](setup.md) [bad](javascript:alert(1))");
        assert!(html.contains("&lt;script&gt;"));
        assert!(html.contains("<a href=\"setup.html\">ok</a>"));
        assert!(!html.contains("href=\"javascript"));
    }

    #[test]
    fn underscores_in_identifiers_stay() {
        assert_eq!(
            inline("snake_case_name and _em_"),
            "snake_case_name and <em>em</em>"
        );
        assert_eq!(inline("[REDACTED:email-1] here"), "[REDACTED:email-1] here");
    }

    #[test]
    fn slugs() {
        assert_eq!(slug("Build and run (Cargo)"), "build-and-run-cargo");
    }
}
