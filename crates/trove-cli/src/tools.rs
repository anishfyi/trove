//! `trove tools`: list, search, describe and open installed CLIs.

use std::process::Command;
use std::time::Duration;

use anyhow::{bail, Result};
use serde_json::json;
use trove_tools::help::{parse_help_summary, probe_help};
use trove_tools::{search, Catalog, DiscoveryEnv, RefreshOptions, SummaryOrigin, ToolEntry};

use crate::paths::tilde;

pub struct ToolsArgs {
    pub args: Vec<String>,
    pub probe: bool,
    pub full: bool,
    pub man: bool,
    pub limit: usize,
    pub json: bool,
}

const HELP_EXCERPT_LINES: usize = 14;

pub fn run(a: ToolsArgs) -> Result<()> {
    let env = DiscoveryEnv::from_process();
    let opts = RefreshOptions {
        full: a.full,
        probe: a.probe,
        ..RefreshOptions::default()
    };
    let path = Catalog::default_path();
    let (mut cat, stats) = Catalog::open_at(&path, &env, &opts)?;

    let first = a.args.first().map(String::as_str);
    match first {
        None | Some("list") => list(&cat, a.limit, a.json),
        Some("search") | Some("find") => {
            let query = a.args[1..].join(" ");
            if query.trim().is_empty() {
                bail!("usage: trove tools search <terms>");
            }
            find(&cat, &query, a.limit, a.json)
        }
        Some("refresh") => {
            if a.json {
                println!("{}", serde_json::to_string_pretty(&stats)?);
            } else {
                println!(
                    "{} tools in {} dirs | {} rescanned, +{} -{} entries, {} man summaries, {} --help probes",
                    stats.tools,
                    stats.dirs,
                    stats.dirs_rescanned,
                    stats.new_entries,
                    stats.removed_entries,
                    stats.man_summaries,
                    stats.help_probed
                );
                println!("-- cache {}", tilde(&path));
            }
            Ok(())
        }
        Some("describe") | Some("show") | Some("open") => {
            let Some(name) = a.args.get(1) else {
                bail!("usage: trove tools describe <name>");
            };
            let man = a.man || first == Some("open");
            describe(&mut cat, &path, name, man, a.json)
        }
        Some(name) => {
            if cat.find(name).is_some() {
                describe(&mut cat, &path, name, a.man, a.json)
            } else {
                if !a.json {
                    println!("no tool named `{name}`; closest matches:");
                }
                find(&cat, &a.args.join(" "), a.limit, a.json)
            }
        }
    }
}

fn line(t: &ToolEntry) -> String {
    let summary = t.summary.as_deref().unwrap_or("");
    let s = format!("{:<22} {:<6} {}", t.name, t.source.label(), summary);
    trove_retrieve::ask::clip(&s, 118)
}

fn list(cat: &Catalog, limit: usize, json: bool) -> Result<()> {
    let tools = cat.tools();
    let shown: Vec<&ToolEntry> = if limit == 0 {
        tools.clone()
    } else {
        tools.iter().take(limit).copied().collect()
    };
    if json {
        println!("{}", serde_json::to_string_pretty(&shown)?);
        return Ok(());
    }
    for t in &shown {
        println!("{}", line(t));
    }
    let with_summary = tools.iter().filter(|t| t.summary.is_some()).count();
    println!(
        "-- {} tools ({} with summaries) in {} dirs{}",
        tools.len(),
        with_summary,
        cat.dirs.len(),
        if shown.len() < tools.len() {
            format!(", showing {}; --limit 0 for all", shown.len())
        } else {
            String::new()
        }
    );
    Ok(())
}

fn find(cat: &Catalog, query: &str, limit: usize, json: bool) -> Result<()> {
    let tools = cat.tools();
    let mut hits = search(&tools, query);
    let total = hits.len();
    if limit > 0 {
        hits.truncate(limit);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&hits)?);
        return Ok(());
    }
    for h in &hits {
        println!("{}", line(h.tool));
    }
    println!(
        "-- {} of {} matches in {} tools",
        hits.len(),
        total,
        tools.len()
    );
    Ok(())
}

fn describe(
    cat: &mut Catalog,
    cache: &std::path::Path,
    name: &str,
    man: bool,
    json: bool,
) -> Result<()> {
    let Some(tool) = cat.find(name).cloned() else {
        bail!("no tool named `{name}` (try: trove tools search {name})");
    };
    if man {
        let status = Command::new("man").arg(&tool.name).status();
        if matches!(status, Ok(s) if s.success()) {
            return Ok(());
        }
        eprintln!("no man page for {}; showing --help", tool.name);
    }

    // The user asked about this tool, so running its --help is expected.
    let help = probe_help(std::path::Path::new(&tool.path), Duration::from_secs(3));
    if tool.summary.is_none() || !tool.probed {
        let summary = help
            .as_deref()
            .and_then(|h| parse_help_summary(h, &tool.name));
        cat.set_summary(&tool.name, summary, SummaryOrigin::Help);
        let _ = cat.save(cache);
    }
    let tool = cat.find(name).cloned().unwrap_or(tool);
    let shadowed: Vec<String> = cat.shadowed(name).iter().map(|t| t.path.clone()).collect();
    let man_page = trove_tools::man::ManIndex::build(
        &DiscoveryEnv::from_process().man_roots(
            &cat.dirs
                .iter()
                .map(|d| d.dir.clone().into())
                .collect::<Vec<_>>(),
        ),
    )
    .page(&tool.name)
    .map(|p| p.to_string_lossy().into_owned());
    let excerpt: Vec<String> = help
        .as_deref()
        .unwrap_or("")
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(HELP_EXCERPT_LINES)
        .map(|l| trove_retrieve::ask::clip(l, 110))
        .collect();

    if json {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "tool": tool,
                "shadowed": shadowed,
                "man_page": man_page,
                "help": excerpt,
            }))?
        );
        return Ok(());
    }
    println!("{}  ({})", tool.name, tool.source.label());
    if let Some(s) = &tool.summary {
        println!("  {s}");
    }
    println!("  path: {}", tool.path);
    for p in &shadowed {
        println!("  shadowed: {p}");
    }
    match &man_page {
        Some(p) => println!("  man: {p}  (trove tools {} --man)", tool.name),
        None => println!("  man: none"),
    }
    if !excerpt.is_empty() {
        println!();
        for l in &excerpt {
            println!("  {l}");
        }
    }
    Ok(())
}
