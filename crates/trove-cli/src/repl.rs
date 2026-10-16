//! Interactive prompt: `trove` with no arguments.

use std::io::{BufRead, IsTerminal, Write};
use std::path::PathBuf;

use anyhow::Result;

use crate::ask::{self, AskOptions, Knowledge};
use crate::llm::Provider;
use crate::tools::{self, ToolsArgs};

const HELP: &str = "\
  <question>        ask your troves, docs, code index and tools
  :tools <terms>    search installed CLIs
  :tool <name>      describe one CLI
  :json             toggle JSON output
  :budget <tokens>  set the context budget
  :provider <name>  none | anthropic | ollama | cmd
  :reload           reload sources (after `trove index` or new entries)
  :help             this help
  :quit             exit (or Ctrl-D)";

pub struct Repl {
    pub repo: PathBuf,
    pub json: bool,
    pub opts: AskOptions,
}

impl Repl {
    pub fn run(mut self) -> Result<()> {
        let interactive = std::io::stdin().is_terminal();
        let mut knowledge = Knowledge::load(&self.repo, true);
        if interactive {
            let counts: Vec<String> = knowledge
                .corpus
                .counts()
                .iter()
                .map(|(k, n)| format!("{n} {}", k.label()))
                .collect();
            println!("trove: ask anything about {}", self.repo.display());
            println!("-- {} | :help for commands", counts.join(", "));
        }
        let stdin = std::io::stdin();
        let mut lines = stdin.lock().lines();
        loop {
            if interactive {
                print!("trove> ");
                std::io::stdout().flush()?;
            }
            let Some(line) = lines.next() else { break };
            let line = line?;
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            let (cmd, rest) = line.split_once(' ').unwrap_or((line, ""));
            let rest = rest.trim();
            let result = match cmd {
                ":q" | ":quit" | ":exit" | "exit" | "quit" => break,
                ":help" | ":h" | "?" => {
                    println!("{HELP}");
                    Ok(())
                }
                ":json" => {
                    self.json = !self.json;
                    println!("json output {}", if self.json { "on" } else { "off" });
                    Ok(())
                }
                ":budget" => match rest.parse::<usize>() {
                    Ok(n) if n > 0 => {
                        self.opts.budget = n;
                        println!("budget {n} tokens");
                        Ok(())
                    }
                    _ => {
                        println!("usage: :budget <tokens>");
                        Ok(())
                    }
                },
                ":provider" => {
                    match Provider::parse(rest) {
                        Some(p) => {
                            self.opts.provider = p;
                            println!("provider {}", p.label());
                        }
                        None => println!("usage: :provider none|anthropic|ollama|cmd"),
                    }
                    Ok(())
                }
                ":reload" => {
                    knowledge = Knowledge::load(&self.repo, true);
                    println!("reloaded {} sources", knowledge.corpus.len());
                    Ok(())
                }
                ":tools" | ":tool" => {
                    let mut args = if cmd == ":tools" && !rest.is_empty() {
                        vec!["search".to_string()]
                    } else {
                        Vec::new()
                    };
                    args.extend(rest.split_whitespace().map(str::to_string));
                    tools::run(ToolsArgs {
                        args,
                        probe: false,
                        full: false,
                        man: false,
                        limit: 15,
                        json: self.json,
                    })
                }
                _ if cmd.starts_with(':') => {
                    println!("unknown command {cmd}; :help for commands");
                    Ok(())
                }
                _ => ask::print(&ask::ask(&knowledge, line, &self.opts), self.json),
            };
            if let Err(e) = result {
                eprintln!("error: {e}");
            }
        }
        Ok(())
    }
}
