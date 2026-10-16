//! Ranked search over the tool catalog.

use serde::Serialize;

use crate::catalog::ToolEntry;

#[derive(Debug, Clone, Serialize)]
pub struct ToolHit<'a> {
    pub score: u32,
    #[serde(flatten)]
    pub tool: &'a ToolEntry,
}

/// Rank tools against a free-text query. Name matches dominate; summary
/// words break ties and find tools by what they do ("json", "pdf").
pub fn search<'a>(tools: &[&'a ToolEntry], query: &str) -> Vec<ToolHit<'a>> {
    let query = query.trim().to_lowercase();
    let terms: Vec<&str> = query
        .split_whitespace()
        .map(|t| t.trim_matches(|c: char| !c.is_alphanumeric()))
        .filter(|t| !t.is_empty())
        .collect();
    if terms.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<ToolHit<'a>> = tools
        .iter()
        .filter_map(|tool| {
            let score = score_tool(tool, &query, &terms);
            (score > 0).then_some(ToolHit { score, tool })
        })
        .collect();
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| a.tool.name.len().cmp(&b.tool.name.len()))
            .then_with(|| a.tool.name.cmp(&b.tool.name))
    });
    hits
}

pub fn score_tool(tool: &ToolEntry, query: &str, terms: &[&str]) -> u32 {
    let name = tool.name.to_lowercase();
    let summary = tool.summary.as_deref().unwrap_or("").to_lowercase();
    let words: Vec<&str> = summary
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .collect();

    let mut score = 0u32;
    if name == query {
        score += 100;
    }
    let mut matched_all = true;
    for term in terms {
        let mut s = 0u32;
        if name == *term {
            s += 40;
        } else if name.starts_with(term) {
            s += 20;
        } else if name.contains(term) {
            s += 10;
        }
        if words.iter().any(|w| w == term) {
            s += 6;
        } else if words.iter().any(|w| w.starts_with(term)) {
            s += 3;
        }
        if s == 0 {
            matched_all = false;
        }
        score += s;
    }
    // Multi-term queries: prefer tools matching every term.
    if terms.len() > 1 && matched_all {
        score += 15 * terms.len() as u32;
    }
    score
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dirs::ToolSource;

    fn tool(name: &str, summary: Option<&str>) -> ToolEntry {
        ToolEntry {
            name: name.into(),
            path: format!("/usr/bin/{name}"),
            dir: "/usr/bin".into(),
            source: ToolSource::System,
            summary: summary.map(str::to_string),
            summary_from: None,
            probed: false,
            mtime: 0,
            size: 0,
        }
    }

    #[test]
    fn exact_name_beats_prefix_beats_summary() {
        let all = [
            tool("git", Some("the stupid content tracker")),
            tool("git-lfs", Some("Git extension for versioning large files")),
            tool("gitk", Some("The Git repository browser")),
            tool("tig", Some("text-mode interface for git")),
            tool("ls", Some("list directory contents")),
        ];
        let refs: Vec<&ToolEntry> = all.iter().collect();
        let hits = search(&refs, "git");
        let names: Vec<&str> = hits.iter().map(|h| h.tool.name.as_str()).collect();
        assert_eq!(names, ["git", "gitk", "git-lfs", "tig"]);
    }

    #[test]
    fn finds_by_what_it_does() {
        let all = [
            tool("jq", Some("Command-line JSON processor")),
            tool("yq", Some("Command-line YAML processor")),
            tool("json_pp", Some("JSON streaming pretty printer")),
            tool(
                "cat",
                Some("concatenate files and print on the standard output"),
            ),
        ];
        let refs: Vec<&ToolEntry> = all.iter().collect();
        let hits = search(&refs, "json processor");
        assert_eq!(hits[0].tool.name, "jq");
        assert!(hits.iter().all(|h| h.tool.name != "cat"));
        assert!(search(&refs, "  ").is_empty());
    }
}
